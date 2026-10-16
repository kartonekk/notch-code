//! Whisper worker: owns the whisper state for one recording session and turns
//! utterances (from both audio sources) into transcript segments.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperState};

use crate::config::Config;
use crate::dsp::{self, RATE};
use crate::paths;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    Me,
    Others,
}

impl Speaker {
    pub fn label(self) -> &'static str {
        match self {
            Speaker::Me => "Me",
            Speaker::Others => "Others",
        }
    }
}

pub struct Job {
    pub id: u64,
    pub speaker: Speaker,
    pub start_ms: u64,
    pub audio: Vec<f32>,
    pub is_final: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Segment {
    pub id: u64,
    pub speaker: Speaker,
    pub text: String,
    pub start_ms: u64,
    #[serde(rename = "final")]
    pub is_final: bool,
}

pub fn load_model(path: &std::path::Path, cfg: &Config) -> Result<WhisperContext, String> {
    let mut params = whisper_rs::WhisperContextParameters::default();
    let mut use_gpu = cfg.use_gpu;
    if use_gpu && !cfg.integrated_gpu && first_gpu_type() == Some(DEV_TYPE_IGPU) {
        paths::log("only an integrated GPU found: transcribing on the CPU (\"integrated_gpu\": true in config.json to use it)");
        use_gpu = false;
    }
    params.use_gpu(use_gpu);
    WhisperContext::new_with_params(path, params)
        .map_err(|e| format!("Failed to load model: {e:?}"))
}

/// ggml device types (ggml-backend.h: CPU, GPU, IGPU, ACCEL).
const DEV_TYPE_GPU: whisper_rs_sys::ggml_backend_dev_type = 1;
const DEV_TYPE_IGPU: whisper_rs_sys::ggml_backend_dev_type = 2;

/// Type of the device whisper.cpp would pick with `use_gpu` (the first GPU or
/// integrated GPU, i.e. `gpu_device` 0), or None when there is none.
fn first_gpu_type() -> Option<whisper_rs_sys::ggml_backend_dev_type> {
    // SAFETY: plain queries of ggml's device registry (compiled-in backends).
    unsafe {
        (0..whisper_rs_sys::ggml_backend_dev_count())
            .map(|i| whisper_rs_sys::ggml_backend_dev_type(whisper_rs_sys::ggml_backend_dev_get(i)))
            .find(|&t| t == DEV_TYPE_GPU || t == DEV_TYPE_IGPU)
    }
}

/// Whether this machine has a GPU whisper.cpp can use, and whether it is an integrated one.
pub fn gpu_kind() -> (bool, bool) {
    let t = first_gpu_type();
    (t.is_some(), t == Some(DEV_TYPE_IGPU))
}

/// Running on a dedicated GPU. Anything else (CPU, integrated GPU) needs
/// seconds per utterance, so live previews would only queue up ahead of the final text.
pub fn fast_backend() -> bool {
    paths::GPU_ACTIVE.load(std::sync::atomic::Ordering::Relaxed) && first_gpu_type() == Some(DEV_TYPE_GPU)
}

/// Transcribes 1 s of silence once so the first real utterance is not slow
/// (GPU pipelines/shaders are compiled lazily).
pub fn warm_up(ctx: &WhisperContext, cfg: &Config) -> Result<Duration, String> {
    let mut state = ctx.create_state().map_err(|e| format!("{e:?}"))?;
    let t = Instant::now();
    transcribe(&mut state, &vec![0.0; RATE], cfg, None, true)?;
    Ok(t.elapsed())
}

pub fn run(
    ctx: Arc<WhisperContext>,
    cfg: Config,
    rx: Receiver<Job>,
    mut on_segment: impl FnMut(Segment),
) {
    let mut state = match ctx.create_state() {
        Ok(s) => s,
        Err(e) => {
            paths::log(format!("create_state failed: {e:?}"));
            return;
        }
    };

    let mut finals: VecDeque<Job> = VecDeque::new();
    let mut partials: HashMap<Speaker, Job> = HashMap::new();
    let mut prompts: HashMap<Speaker, String> = HashMap::new();
    let mut disconnected = false;

    loop {
        // Collect everything that is waiting; block briefly only when idle.
        if finals.is_empty() && partials.is_empty() {
            if disconnected {
                break;
            }
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(job) => enqueue(job, &mut finals, &mut partials),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    disconnected = true;
                    continue;
                }
            }
        }
        while let Ok(job) = rx.try_recv() {
            enqueue(job, &mut finals, &mut partials);
        }
        if disconnected {
            // stopping: previews are pointless now
            partials.clear();
        }

        let job = match finals.pop_front() {
            Some(j) => j,
            None => match partials.keys().next().copied() {
                Some(k) => partials.remove(&k).unwrap(),
                None => continue,
            },
        };

        let text = if job.audio.is_empty() {
            String::new()
        } else {
            let prompt = if job.is_final { prompts.get(&job.speaker).map(String::as_str) } else { None };
            let t = Instant::now();
            let result = transcribe(&mut state, &job.audio, &cfg, prompt, !job.is_final);
            let took = t.elapsed();
            match result {
                Ok(text) => {
                    if job.is_final {
                        paths::log(format!(
                            "[{}] {:.1}s audio -> {} ms: {}",
                            job.speaker.label(),
                            job.audio.len() as f32 / RATE as f32,
                            took.as_millis(),
                            text
                        ));
                    }
                    text
                }
                Err(e) => {
                    paths::log(format!("transcribe error: {e}"));
                    String::new()
                }
            }
        };

        if job.is_final && !text.is_empty() {
            let p = prompts.entry(job.speaker).or_default();
            p.push(' ');
            p.push_str(&text);
            if p.len() > 300 {
                let cut = p.char_indices().rev().nth(200).map(|(i, _)| i).unwrap_or(0);
                *p = p[cut..].to_string();
            }
        }

        // An empty partial is not worth showing; an empty final removes the preview.
        if job.is_final || !text.is_empty() {
            on_segment(Segment {
                id: job.id,
                speaker: job.speaker,
                text,
                start_ms: job.start_ms,
                is_final: job.is_final,
            });
        }
    }
}

fn enqueue(job: Job, finals: &mut VecDeque<Job>, partials: &mut HashMap<Speaker, Job>) {
    if job.is_final {
        if partials.get(&job.speaker).is_some_and(|p| p.id == job.id) {
            partials.remove(&job.speaker);
        }
        finals.push_back(job);
    } else {
        // only the newest preview per speaker matters
        partials.insert(job.speaker, job);
    }
}

pub fn transcribe(
    state: &mut WhisperState,
    audio: &[f32],
    cfg: &Config,
    prompt: Option<&str>,
    partial: bool,
) -> Result<String, String> {
    let mut input = audio.to_vec();
    // whisper.cpp needs at least ~1 s of audio
    if input.len() < RATE + RATE / 10 {
        input.resize(RATE + RATE / 10, 0.0);
    }
    // normalise quiet speakers so the model sees a consistent level
    let peak = input.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if peak > 0.0 && peak < 0.5 {
        let g = (0.5 / peak).min(10.0);
        input.iter_mut().for_each(|x| *x *= g);
    }

    let lang = pick_language(state, &input, cfg);
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(lang);
    params.set_translate(false);
    params.set_n_threads(cfg.threads);
    params.set_no_context(true);
    params.set_no_timestamps(true);
    params.set_single_segment(partial);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);
    params.set_temperature(0.0);
    if !fast_backend() {
        // On the CPU / an integrated GPU the encoder dominates and always covers a full 30 s window;
        // fit it to the utterance (50 frames per second, plus slack) instead.
        let frames = input.len() * 50 / RATE + 64;
        params.set_audio_ctx(frames.clamp(384, 1500) as i32);
    }
    if let Some(p) = prompt {
        params.set_initial_prompt(p.trim());
    }

    state.full(params, &input).map_err(|e| format!("{e:?}"))?;

    let mut text = String::new();
    for seg in state.as_iter() {
        if seg.no_speech_probability() > 0.6 && dsp::rms(audio) < 0.02 {
            continue;
        }
        let s = seg.to_str_lossy().map_err(|e| format!("{e:?}"))?;
        text.push_str(&s);
    }
    Ok(clean(&text))
}

/// `language` is "auto", one code ("pl") or a list ("pl,en"). With a list,
/// Whisper's language detection decides per utterance, but only between
/// those (plain "auto" wanders off into Russian, Czech, … on short clips).
fn pick_language<'a>(state: &mut WhisperState, input: &[f32], cfg: &'a Config) -> Option<&'a str> {
    let langs: Vec<&str> = cfg.language.split(',').map(str::trim).filter(|l| !l.is_empty()).collect();
    match langs.as_slice() {
        [] | ["auto"] => return None,
        [one] => return Some(*one),
        _ => {}
    }
    let threads = cfg.threads.max(1) as usize;
    let probs = state
        .pcm_to_mel(input, threads)
        .and_then(|_| state.lang_detect(0, threads))
        .map(|(_, p)| p);
    let probs = match probs {
        Ok(p) => p,
        Err(e) => {
            paths::log(format!("language detection failed: {e:?}"));
            return Some(langs[0]);
        }
    };
    let score = |l: &str| {
        whisper_rs::get_lang_id(l)
            .and_then(|id| probs.get(id as usize).copied())
            .unwrap_or(0.0)
    };
    langs.iter().copied().max_by(|a, b| score(a).total_cmp(&score(b)))
}

/// Whisper likes to invent these on silence/music (especially in Polish).
const HALLUCINATIONS: &[&str] = &[
    "amara.org",
    "napisy stworzone",
    "napisy wykonane",
    "napisy przygotował",
    "dziękuję za obejrzenie",
    "dzięki za obejrzenie",
    "subskrybuj",
    "thanks for watching",
    "thank you for watching",
    "[muzyka]",
    "[music]",
];

fn clean(text: &str) -> String {
    let t = text.trim();
    let lower = t.to_lowercase();
    if HALLUCINATIONS.iter().any(|h| lower.contains(h)) {
        return String::new();
    }
    if !t.chars().any(|c| c.is_alphanumeric()) {
        return String::new();
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}
