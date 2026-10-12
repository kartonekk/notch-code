//! Spoken prompts: say "Claude, …" (while the wake word is on) or press the mic
//! button, and what you say until you stop talking is sent to Claude.
//!
//! Uses its own microphone stream and Whisper state on the already loaded
//! model, so it works next to a running transcription. The microphone is only
//! open while the wake word is on or a spoken prompt is being taken.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use whisper_rs::{WhisperContext, WhisperState};

use crate::audio::{self, CaptureCtx, Input, Level};
use crate::config::Config;
use crate::transcriber::{self, Job, Speaker};
use crate::{paths, AppState};

/// How long the microphone waits for the prompt after "Claude" alone or the mic button.
const ARMED_FOR: Duration = Duration::from_secs(10);
/// Whisper's spellings of the name (English and Polish).
const WAKE: &[&str] = &["claude", "claud", "klaud", "klaude", "clod", "clode", "klod", "klode", "claudzie", "klaudzie"];
/// Allowed before the name: "Hey Claude, …"
const GREETINGS: &[&str] = &["hey", "hej", "hi", "ok", "okay", "okej", "hello"];
/// Steers Whisper towards spelling the name right.
const HINT: &str = "Claude,";

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Microphone closed.
    #[default]
    Off,
    /// Waiting for the wake word.
    Listening,
    /// Waiting for the prompt (after "Claude" alone or the mic button).
    Armed,
    /// Taking the prompt; `text` is what was heard so far.
    Hearing,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct VoiceStatus {
    /// The wake word is on.
    pub wake: bool,
    pub phase: Phase,
    pub text: Option<String>,
    pub error: Option<String>,
}

#[derive(Default)]
struct Inner {
    status: VoiceStatus,
    /// Set while the microphone is open; stops its capture.
    stop: Option<Arc<AtomicBool>>,
    armed_until: Option<Instant>,
    /// The utterance being taken as the prompt.
    hearing: Option<u64>,
}

#[derive(Default)]
pub struct Voice {
    inner: Mutex<Inner>,
}

/// Changes the state and tells the UI.
fn update<T>(app: &AppHandle, f: impl FnOnce(&mut Inner) -> T) -> T {
    let (out, snap) = {
        let voice = app.state::<Voice>();
        let mut inner = voice.inner.lock().unwrap();
        let out = f(&mut inner);
        (out, inner.status.clone())
    };
    let _ = app.emit("voice", snap);
    out
}

fn read<T>(app: &AppHandle, f: impl FnOnce(&Inner) -> T) -> T {
    f(&app.state::<Voice>().inner.lock().unwrap())
}

/// Back to waiting for the wake word (or off) after a prompt is sent or dropped.
fn settle(i: &mut Inner) {
    i.status.phase = if i.status.wake { Phase::Listening } else { Phase::Off };
    i.status.text = None;
    i.hearing = None;
    i.armed_until = None;
}

fn arm(i: &mut Inner) {
    i.status.phase = Phase::Armed;
    i.status.text = None;
    i.hearing = None;
    i.armed_until = Some(Instant::now() + ARMED_FOR);
}

/// What follows the wake word when `text` addresses Claude
/// ("Hey Claude, fix the build." -> "Fix the build."); None when it doesn't.
fn after_wake(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let norm = |w: &str| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    let mut i = 0;
    while i < 2 && words.get(i).is_some_and(|w| GREETINGS.contains(&norm(w).as_str())) {
        i += 1;
    }
    if !WAKE.contains(&norm(words.get(i)?).as_str()) {
        return None;
    }
    let rest = words[i + 1..].join(" ");
    let rest = rest.trim_start_matches(|c: char| !c.is_alphanumeric());
    let mut chars = rest.chars();
    Some(match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    })
}

// ---------------------------------------------------------------- microphone

/// Opens the microphone unless it's open already (or nothing needs it).
fn open(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let model = state.model.lock().unwrap().clone().ok_or("The speech model is still loading.")?;
    let cfg = state.config.lock().unwrap().clone();
    let stop = Arc::new(AtomicBool::new(false));
    let needed = {
        let voice = app.state::<Voice>();
        let mut i = voice.inner.lock().unwrap();
        let needed = i.stop.is_none() && i.status.phase != Phase::Off;
        if needed {
            i.stop = Some(stop.clone());
        }
        needed
    };
    if !needed {
        return Ok(());
    }

    let (tx, rx) = mpsc::channel();
    let ctx = CaptureCtx {
        input: Input::Mic,
        speaker: Speaker::Me,
        jobs: tx,
        stop: stop.clone(),
        paused: Arc::new(AtomicBool::new(false)),
        level: Arc::new(Level::default()),
        ids: Arc::new(AtomicU64::new(1)),
        started: Instant::now(),
        partials: true,
        dictation: true,
    };
    if let Err(e) = audio::spawn_capture(ctx) {
        update(app, |i| {
            i.stop = None;
            i.status.wake = false;
            settle(i);
            i.status.error = Some(e.clone());
        });
        return Err(e);
    }
    let app2 = app.clone();
    std::thread::Builder::new()
        .name("voice".into())
        .spawn(move || work(app2, model, cfg, rx, stop))
        .map_err(|e| e.to_string())?;
    paths::log("voice: microphone open");
    Ok(())
}

/// Turns utterances into prompts until nothing needs the microphone anymore.
fn work(app: AppHandle, model: Arc<WhisperContext>, cfg: Config, rx: Receiver<Job>, stop: Arc<AtomicBool>) {
    let mut state = match model.create_state() {
        Ok(s) => s,
        Err(e) => {
            paths::log(format!("voice: create_state failed: {e:?}"));
            close(&app, &stop, Some("Speech recognition failed to start.".into()));
            return;
        }
    };
    // utterances whose opening was already checked for the wake word
    let mut checked: HashSet<u64> = HashSet::new();
    loop {
        let mut jobs = Vec::new();
        match rx.recv_timeout(Duration::from_millis(150)) {
            Ok(job) => jobs.push(job),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                close(&app, &stop, None);
                return;
            }
        }
        while let Ok(job) = rx.try_recv() {
            jobs.push(job);
        }
        // only the newest preview matters, and none of an utterance that has ended
        let ended: HashSet<u64> = jobs.iter().filter(|j| j.is_final).map(|j| j.id).collect();
        let preview = jobs.iter().rposition(|j| !j.is_final).filter(|&k| !ended.contains(&jobs[k].id));
        for (k, job) in jobs.into_iter().enumerate() {
            if job.is_final {
                checked.remove(&job.id);
                on_final(&app, &mut state, &cfg, job);
            } else if Some(k) == preview {
                on_preview(&app, &mut state, &cfg, job, &mut checked);
            }
        }

        let closing = update(&app, |i| {
            if i.status.phase == Phase::Armed && i.armed_until.is_some_and(|t| Instant::now() >= t) {
                settle(i);
            }
            let ours = i.stop.as_ref().is_some_and(|s| Arc::ptr_eq(s, &stop));
            if i.status.phase == Phase::Off && ours {
                i.stop = None;
            }
            i.status.phase == Phase::Off || !ours
        });
        if closing {
            stop.store(true, Ordering::SeqCst);
            paths::log("voice: microphone closed");
            return;
        }
    }
}

/// The microphone went away (or Whisper failed): back to off.
fn close(app: &AppHandle, stop: &Arc<AtomicBool>, error: Option<String>) {
    stop.store(true, Ordering::SeqCst);
    update(app, |i| {
        if i.stop.as_ref().is_some_and(|s| Arc::ptr_eq(s, stop)) {
            i.stop = None;
            i.status.wake = false;
            settle(i);
            i.status.error = error.or_else(|| Some("The microphone stopped.".into()));
        }
    });
}

fn hear(state: &mut WhisperState, cfg: &Config, job: &Job) -> String {
    transcriber::transcribe(state, &job.audio, cfg, Some(HINT), !job.is_final).unwrap_or_else(|e| {
        paths::log(format!("voice: transcribe error: {e}"));
        String::new()
    })
}

/// Live text while you talk; also catches "Claude…" early, before the sentence ends.
fn on_preview(app: &AppHandle, state: &mut WhisperState, cfg: &Config, job: Job, checked: &mut HashSet<u64>) {
    let (phase, hearing) = read(app, |i| (i.status.phase, i.hearing));
    let wanted = match phase {
        Phase::Off => false,
        Phase::Listening => !checked.contains(&job.id),
        Phase::Armed => true,
        Phase::Hearing => hearing == Some(job.id),
    };
    if !wanted {
        return;
    }
    let text = hear(state, cfg, &job);
    if phase == Phase::Listening {
        checked.insert(job.id);
    }
    let shown = match phase {
        Phase::Listening => match after_wake(&text) {
            Some(rest) => rest,
            None => return,
        },
        _ => after_wake(&text).unwrap_or(text),
    };
    update(app, |i| {
        // the phase may have changed while Whisper ran (e.g. cancelled)
        let still = i.status.phase == phase && (phase != Phase::Hearing || i.hearing == Some(job.id));
        if still {
            i.status.phase = Phase::Hearing;
            i.hearing = Some(job.id);
            i.armed_until = None;
            i.status.text = Some(shown);
        }
    });
}

/// The utterance ended: send it when it was meant for Claude.
fn on_final(app: &AppHandle, state: &mut WhisperState, cfg: &Config, job: Job) {
    let (phase, hearing) = read(app, |i| (i.status.phase, i.hearing));
    if phase == Phase::Off {
        return;
    }
    let text = if job.audio.is_empty() { String::new() } else { hear(state, cfg, &job) };
    let prompt = match phase {
        Phase::Listening => after_wake(&text),
        // after "Claude" or the mic button anything counts; a repeated "Claude, …" is dropped from it
        _ => Some(after_wake(&text).unwrap_or(text)),
    };
    let sent = update(app, |i| {
        let ours = match i.status.phase {
            Phase::Off => false,
            Phase::Listening => phase == Phase::Listening,
            Phase::Armed => true,
            Phase::Hearing => i.hearing == Some(job.id),
        };
        if !ours {
            return None;
        }
        match prompt {
            None if i.status.phase == Phase::Hearing => {
                settle(i);
                None
            }
            None => None,
            // just "Claude" (or nothing heard yet): wait for the prompt
            Some(p) if p.is_empty() => {
                if i.status.phase != Phase::Armed || hearing.is_some() {
                    arm(i);
                }
                None
            }
            Some(p) => {
                settle(i);
                Some(p)
            }
        }
    });
    if let Some(p) = sent {
        paths::log(format!("voice: prompt: {p}"));
        let _ = app.emit("voice-prompt", p);
    }
}

// ---------------------------------------------------------------- lifecycle & commands

/// Reads the wake word setting at startup (the microphone opens once the model is ready).
pub fn init(app: &AppHandle, cfg: &Config) {
    update(app, |i| i.status.wake = cfg.voice_wake);
}

/// The speech model finished loading.
pub fn model_ready(app: &AppHandle) {
    let wake = update(app, |i| {
        if i.status.wake && i.status.phase == Phase::Off {
            i.status.phase = Phase::Listening;
        }
        i.status.wake
    });
    if wake {
        if let Err(e) = open(app) {
            paths::log(format!("voice: {e}"));
        }
    }
}

#[tauri::command]
pub fn voice_status(app: AppHandle) -> VoiceStatus {
    read(&app, |i| i.status.clone())
}

/// Turns voice (the wake word and the mic button) on or off; remembered in config.json.
#[tauri::command]
pub fn voice_set_wake(app: AppHandle, on: bool) -> Result<(), String> {
    let mut cfg = Config::load();
    cfg.voice_wake = on;
    cfg.save();
    app.state::<AppState>().config.lock().unwrap().voice_wake = on;
    update(&app, |i| {
        i.status.wake = on;
        i.status.error = None;
        if !on {
            // voice off: drop any prompt in progress, the microphone closes
            settle(i);
        } else if i.status.phase == Phase::Off {
            i.status.phase = Phase::Listening;
        }
    });
    let loaded = app.state::<AppState>().model.lock().unwrap().is_some();
    // still loading: `model_ready` opens the microphone
    if on && loaded { open(&app) } else { Ok(()) }
}

/// The mic button: take the next thing you say as the prompt, no wake word
/// needed. Pressed again while waiting or listening, it cancels.
#[tauri::command]
pub fn voice_talk(app: AppHandle) -> Result<(), String> {
    let start = update(&app, |i| {
        i.status.error = None;
        match i.status.phase {
            Phase::Armed | Phase::Hearing => {
                settle(i);
                false
            }
            _ => {
                arm(i);
                true
            }
        }
    });
    if !start {
        return Ok(());
    }
    open(&app).inspect_err(|_| update(&app, settle))
}

#[cfg(test)]
mod tests {
    use super::after_wake;

    #[test]
    fn wake_word() {
        assert_eq!(after_wake("Claude, fix the build."), Some("Fix the build.".into()));
        assert_eq!(after_wake("Hey Claude fix it"), Some("Fix it".into()));
        assert_eq!(after_wake("Klaud, napraw testy"), Some("Napraw testy".into()));
        assert_eq!(after_wake("Claude."), Some(String::new()));
        assert_eq!(after_wake("I asked Claude yesterday"), None);
        assert_eq!(after_wake(""), None);
    }
}
