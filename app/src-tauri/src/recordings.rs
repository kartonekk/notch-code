//! Recorded files (a phone recording of a lab, an .mp3 from someone) dropped on
//! the island or picked in the Transcribe tab. They wait in a queue and are
//! transcribed one at a time into the same Markdown files as live meetings.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde::Serialize;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::dsp::{Resampler, Segmenter, VadEvent, RATE};
use crate::transcriber::{self, Job, Speaker};
use crate::transcript::TranscriptFile;
use crate::{paths, AppState};

/// What symphonia is built to read (Cargo.toml features). No Opus.
pub const AUDIO_EXTS: [&str; 9] = ["mp3", "m4a", "mp4", "aac", "wav", "flac", "ogg", "oga", "mkv"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemState {
    Waiting,
    Working,
    Done,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub id: u64,
    /// file name shown in the list
    pub name: String,
    pub path: String,
    pub state: ItemState,
    /// 0..1 while working; null when the length is unknown
    pub progress: Option<f32>,
    pub duration_ms: Option<u64>,
    /// the Markdown file, once created
    pub transcript: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct Status {
    pub items: Vec<Item>,
    /// why the queue isn't moving ("Waits for the live recording to end")
    pub note: Option<String>,
}

#[derive(Default)]
pub struct Recordings {
    status: Mutex<Status>,
    next_id: AtomicU64,
    worker_running: AtomicBool,
    /// cancels the file being transcribed
    cancel: Mutex<Option<Arc<AtomicBool>>>,
}

fn update(app: &AppHandle, f: impl FnOnce(&mut Status)) {
    let r = app.state::<Recordings>();
    let snapshot = {
        let mut s = r.status.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    let _ = app.emit("recordings", snapshot);
}

fn update_item(app: &AppHandle, id: u64, f: impl FnOnce(&mut Item)) {
    update(app, |s| {
        if let Some(item) = s.items.iter_mut().find(|i| i.id == id) {
            f(item);
        }
    });
}

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTS.contains(&e.to_lowercase().as_str()))
}

// ---------------------------------------------------------------- commands

#[tauri::command]
pub fn recordings_status(app: AppHandle) -> Status {
    app.state::<Recordings>().status.lock().unwrap().clone()
}

/// Queues files to transcribe. Fails (queuing nothing) when one of them isn't
/// a recording the app can read.
#[tauri::command]
pub fn recordings_add(app: AppHandle, paths: Vec<String>) -> Result<(), String> {
    let files: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let unsupported: Vec<String> = files
        .iter()
        .filter(|p| !is_audio(p) || !p.is_file())
        .map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
        .collect();
    if !unsupported.is_empty() {
        return Err(format!(
            "Can't transcribe {} — use MP3, M4A, WAV, FLAC or OGG.",
            unsupported.join(", ")
        ));
    }
    let r = app.state::<Recordings>();
    update(&app, |s| {
        for path in files {
            let path_str = path.to_string_lossy().into_owned();
            let queued = s.items.iter().any(|i| {
                i.path.eq_ignore_ascii_case(&path_str) && matches!(i.state, ItemState::Waiting | ItemState::Working)
            });
            if queued {
                continue;
            }
            s.items.push(Item {
                id: r.next_id.fetch_add(1, Ordering::Relaxed) + 1,
                name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                path: path_str,
                state: ItemState::Waiting,
                progress: None,
                duration_ms: None,
                transcript: None,
                error: None,
            });
        }
    });
    spawn_worker(&app);
    Ok(())
}

/// "Browse…": the file dialog, then the same as dropping the files.
#[tauri::command]
pub async fn recordings_pick(app: AppHandle) -> Result<(), String> {
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Recordings to transcribe")
        .add_filter("Recordings", &AUDIO_EXTS);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    let Some(picked) = dialog.blocking_pick_files() else { return Ok(()) };
    let paths = picked
        .into_iter()
        .map(|p| p.into_path().map(|p| p.to_string_lossy().into_owned()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    recordings_add(app, paths)
}

/// Stops the file being transcribed (what's written so far is kept), or takes
/// any other file off the list.
#[tauri::command]
pub fn recordings_remove(app: AppHandle, id: u64) {
    let working = {
        let r = app.state::<Recordings>();
        let s = r.status.lock().unwrap();
        s.items.iter().any(|i| i.id == id && i.state == ItemState::Working)
    };
    if working {
        if let Some(c) = app.state::<Recordings>().cancel.lock().unwrap().as_ref() {
            c.store(true, Ordering::SeqCst);
        }
    } else {
        update(&app, |s| s.items.retain(|i| i.id != id));
    }
}

/// Takes the finished files off the list.
#[tauri::command]
pub fn recordings_clear(app: AppHandle) {
    update(&app, |s| s.items.retain(|i| matches!(i.state, ItemState::Waiting | ItemState::Working)));
}

#[tauri::command(async)]
pub fn recordings_open(app: AppHandle, id: u64) -> Result<(), String> {
    let path = {
        let r = app.state::<Recordings>();
        let s = r.status.lock().unwrap();
        s.items.iter().find(|i| i.id == id).and_then(|i| i.transcript.clone())
    };
    let path = path.ok_or("No transcript yet")?;
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- worker

fn spawn_worker(app: &AppHandle) {
    if app.state::<Recordings>().worker_running.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let _ = std::thread::Builder::new().name("recordings".into()).spawn(move || {
        loop {
            let next = {
                let r = app.state::<Recordings>();
                let s = r.status.lock().unwrap();
                s.items.iter().find(|i| i.state == ItemState::Waiting).cloned()
            };
            let Some(item) = next else { break };
            // the speech model must be loaded and the GPU free of a live meeting
            if let Some(why) = blocked(&app) {
                update(&app, |s| s.note = Some(why.into()));
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            update(&app, |s| s.note = None);
            let cancel = Arc::new(AtomicBool::new(false));
            *app.state::<Recordings>().cancel.lock().unwrap() = Some(cancel.clone());
            update_item(&app, item.id, |i| i.state = ItemState::Working);
            let result = transcribe_file(&app, &item, &cancel);
            *app.state::<Recordings>().cancel.lock().unwrap() = None;
            match result {
                Ok(()) => update_item(&app, item.id, |i| {
                    i.state = if cancel.load(Ordering::SeqCst) { ItemState::Cancelled } else { ItemState::Done };
                    i.progress = None;
                }),
                Err(e) => {
                    paths::log(format!("recording {}: {e}", item.name));
                    update_item(&app, item.id, |i| {
                        i.state = ItemState::Failed;
                        i.progress = None;
                        i.error = Some(e);
                    });
                }
            }
        }
        let r = app.state::<Recordings>();
        r.worker_running.store(false, Ordering::SeqCst);
        // a file added while the loop was ending
        if r.status.lock().unwrap().items.iter().any(|i| i.state == ItemState::Waiting) {
            spawn_worker(&app);
        }
    });
}

fn blocked(app: &AppHandle) -> Option<&'static str> {
    let state = app.state::<AppState>();
    if state.session.lock().unwrap().is_some() {
        Some("Waits for the live recording to end")
    } else if state.model.lock().unwrap().is_none() {
        Some("Waits for the speech model to load")
    } else {
        None
    }
}

/// The recording's start: a date at the start of the file name
/// ("2026-10-17 10-15 Fizyka", "20261017_101500"), else the time the file was
/// last written minus its length. Returns the rest of the name as the title.
fn started_and_title(path: &Path, duration_ms: Option<u64>) -> (DateTime<Local>, String) {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    for fmt in ["%Y-%m-%d %H-%M", "%Y-%m-%d_%H-%M-%S", "%Y%m%d_%H%M%S", "%Y-%m-%d %H.%M"] {
        if let Ok((dt, rest)) = NaiveDateTime::parse_and_remainder(&stem, fmt) {
            if let Some(local) = Local.from_local_datetime(&dt).earliest() {
                let title = rest.trim_matches(|c: char| c.is_whitespace() || c == '-' || c == '_').to_string();
                return (local, if title.is_empty() { "Recording".into() } else { title });
            }
        }
    }
    let written: DateTime<Local> = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(DateTime::from)
        .unwrap_or_else(|_| Local::now());
    let started = written - chrono::Duration::milliseconds(duration_ms.unwrap_or(0) as i64);
    (started, stem)
}

fn transcribe_file(app: &AppHandle, item: &Item, cancel: &Arc<AtomicBool>) -> Result<(), String> {
    let path = PathBuf::from(&item.path);
    let state = app.state::<AppState>();
    let model = state.model.lock().unwrap().clone().ok_or("The speech model is not loaded")?;
    let cfg = state.config.lock().unwrap().clone();

    // open and probe first, so an unreadable file fails before a transcript exists
    let mut decoder = Decoder::open(&path)?;
    let duration_ms = decoder.duration_ms();
    update_item(app, item.id, |i| {
        i.duration_ms = duration_ms;
        i.progress = duration_ms.map(|_| 0.0);
    });

    let (started, title) = started_and_title(&path, duration_ms);
    let mut file = TranscriptFile::create_titled(&title, &format!("File · {}", item.name), started)?;
    let transcript = file.path.to_string_lossy().into_owned();
    update_item(app, item.id, |i| i.transcript = Some(transcript));
    paths::log(format!("transcribing {} ({:?} ms)", item.name, duration_ms));

    // decoding runs ahead of Whisper by at most a few utterances
    let (tx, rx) = mpsc::sync_channel::<Job>(2);
    let (app2, cancel2, id) = (app.clone(), cancel.clone(), item.id);
    let decode = std::thread::Builder::new()
        .name("decode".into())
        .spawn(move || -> Result<u64, String> {
            let mut resampler = Resampler::new(decoder.rate);
            // recordings have a noise floor like a microphone, but are often quieter
            let mut seg = Segmenter::new(0.004, false, 0);
            let mut ids = 0u64;
            let mut next_id = || {
                ids += 1;
                ids
            };
            let (mut mono, mut out, mut events) = (Vec::new(), Vec::new(), Vec::new());
            let mut done_samples = 0u64;
            let mut last_emit = Instant::now();
            while decoder.next(&mut mono)? {
                if cancel2.load(Ordering::Relaxed) {
                    break;
                }
                // a live meeting takes the GPU; carry on once it ends
                while app2.state::<AppState>().session.lock().unwrap().is_some() && !cancel2.load(Ordering::Relaxed) {
                    update(&app2, |s| s.note = Some("Paused for the live recording".into()));
                    std::thread::sleep(Duration::from_millis(500));
                }
                out.clear();
                resampler.process(&mono, &mut out);
                done_samples += out.len() as u64;
                seg.push(&out, &mut next_id, &mut events);
                for ev in events.drain(..) {
                    if let VadEvent::Final { utt, start_ms, audio } = ev {
                        let job = Job { id: utt, speaker: Speaker::Others, start_ms, audio, is_final: true };
                        tx.send(job).map_err(|_| "Transcription stopped".to_string())?;
                    }
                }
                if last_emit.elapsed() > Duration::from_millis(400) {
                    last_emit = Instant::now();
                    let at = done_samples * 1000 / RATE as u64;
                    update(&app2, |s| s.note = None);
                    update_item(&app2, id, |i| {
                        i.progress = i.duration_ms.map(|d| (at as f32 / d.max(1) as f32).min(0.99));
                    });
                }
            }
            seg.flush(&mut events);
            for ev in events.drain(..) {
                if let VadEvent::Final { utt, start_ms, audio } = ev {
                    let _ = tx.send(Job { id: utt, speaker: Speaker::Others, start_ms, audio, is_final: true });
                }
            }
            Ok(done_samples * 1000 / RATE as u64)
        })
        .map_err(|e| e.to_string())?;

    transcriber::run(model, cfg, rx, |seg| {
        if seg.is_final && !seg.text.is_empty() {
            file.append(&seg);
        }
    });
    let decoded = decode.join().map_err(|_| "The decoder crashed".to_string())?;
    match decoded {
        Ok(length_ms) => {
            if cancel.load(Ordering::SeqCst) {
                file.mark("stopped", length_ms);
            }
            file.finish();
            Ok(())
        }
        Err(e) => {
            file.mark(&format!("couldn't read the rest of the file: {e}"), 0);
            file.finish();
            Err(e)
        }
    }
}

// ---------------------------------------------------------------- decoding

/// Any file symphonia reads, as mono f32 at the file's own rate.
struct Decoder {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track: u32,
    rate: u32,
    frames: Option<u64>,
}

impl Decoder {
    fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("Can't open the file: {e}"))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| format!("Not a recording this app can read ({e})"))?;
        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or("The file has no audio")?;
        let track_id = track.id;
        let params = track.codec_params.clone();
        let rate = params.sample_rate.ok_or("Unknown sample rate")?;
        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|e| format!("Unsupported audio format ({e})"))?;
        Ok(Self { track: track_id, format, decoder, rate, frames: params.n_frames })
    }

    fn duration_ms(&self) -> Option<u64> {
        self.frames.map(|n| n * 1000 / self.rate as u64)
    }

    /// The next chunk of audio into `mono` (replaced); false at the end.
    fn next(&mut self, mono: &mut Vec<f32>) -> Result<bool, String> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(DecodeError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
                Err(DecodeError::ResetRequired) => return Ok(false),
                Err(e) => return Err(e.to_string()),
            };
            if packet.track_id() != self.track {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(d) => d,
                // a damaged frame: skip it, like players do
                Err(DecodeError::DecodeError(_)) => continue,
                Err(e) => return Err(e.to_string()),
            };
            let spec = *decoded.spec();
            let channels = spec.channels.count().max(1);
            let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
            buf.copy_interleaved_ref(decoded);
            mono.clear();
            mono.extend(buf.samples().chunks_exact(channels).map(|f| f.iter().sum::<f32>() / channels as f32));
            return Ok(true);
        }
    }
}
