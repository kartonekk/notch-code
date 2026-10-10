//! WASAPI: listing apps that play audio, capturing a single app (process
//! loopback, Windows 10 2004+) and the default microphone.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use wasapi::{
    initialize_mta, AudioClient, DeviceEnumerator, Direction, SampleType, SessionState,
    StreamMode, WaveFormat,
};

use crate::dsp::{Downsampler, Segmenter, VadEvent, RATE};
use crate::paths;
use crate::transcriber::{Job, Speaker};

#[derive(Clone, Debug, Serialize)]
pub struct AudioSource {
    pub pid: u32,
    pub name: String,
    pub exe: String,
    pub active: bool,
    pub peak: f32,
    pub suggested: bool,
}

/// Apps known to host meetings; they are preselected in the picker.
const MEETING_APPS: &[&str] = &[
    "ciscocollabhost", "webex", "atmgr", "ms-teams", "teams", "zoom", "slack", "discord",
];

fn friendly_name(exe: &str) -> String {
    let stem = exe.trim_end_matches(".exe").trim_end_matches(".EXE");
    let lower = stem.to_lowercase();
    let known = [
        ("ciscocollabhost", "Webex"),
        ("webex", "Webex"),
        ("atmgr", "Webex Meetings"),
        ("ms-teams", "Microsoft Teams"),
        ("teams", "Microsoft Teams"),
        ("zoom", "Zoom"),
        ("chrome", "Google Chrome"),
        ("msedge", "Microsoft Edge"),
        ("firefox", "Firefox"),
        ("opera", "Opera"),
        ("brave", "Brave"),
        ("spotify", "Spotify"),
        ("discord", "Discord"),
        ("slack", "Slack"),
        ("vlc", "VLC"),
    ];
    for (k, v) in known {
        if lower.starts_with(k) {
            return v.to_string();
        }
    }
    let mut c = stem.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => exe.to_string(),
    }
}

/// Runs `f` on a fresh thread with COM initialised as MTA (Tauri's main
/// thread is STA, which WASAPI activation does not like).
pub fn with_mta<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    std::thread::spawn(move || {
        initialize_mta().ok().map_err(|e| format!("COM: {e}"))?;
        f()
    })
    .join()
    .map_err(|_| "wątek audio uległ awarii".to_string())?
}

pub fn list_sources() -> Result<Vec<AudioSource>, String> {
    with_mta(|| {
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
        let me = std::process::id();

        let enumerator = DeviceEnumerator::new().map_err(|e| e.to_string())?;
        let devices = enumerator.get_device_collection(&Direction::Render).map_err(|e| e.to_string())?;

        // root pid -> source
        let mut found: HashMap<u32, AudioSource> = HashMap::new();
        for dev in &devices {
            let Ok(dev) = dev else { continue };
            let Ok(manager) = dev.get_iaudiosessionmanager() else { continue };
            let Ok(sessions) = manager.get_audiosessionenumerator() else { continue };
            for i in 0..sessions.get_count().unwrap_or(0) {
                let Ok(control) = sessions.get_session(i) else { continue };
                let state = control.get_state().unwrap_or(SessionState::Expired);
                if state == SessionState::Expired {
                    continue;
                }
                let pid = control.get_process_id().unwrap_or(0);
                if pid == 0 || pid == me {
                    continue;
                }
                let peak = control
                    .get_audiometerinformation()
                    .and_then(|m| m.get_peak_value())
                    .unwrap_or(0.0);
                let Some(proc_) = sys.process(Pid::from_u32(pid)) else { continue };
                let exe = proc_.name().to_string_lossy().to_string();
                let root = root_of_same_app(&sys, pid);
                let entry = found.entry(root).or_insert_with(|| {
                    let lower = exe.to_lowercase();
                    AudioSource {
                        pid: root,
                        name: friendly_name(&exe),
                        exe: exe.clone(),
                        active: false,
                        peak: 0.0,
                        suggested: MEETING_APPS.iter().any(|m| lower.starts_with(m)),
                    }
                });
                entry.active |= state == SessionState::Active;
                entry.peak = entry.peak.max(peak);
            }
        }

        let mut list: Vec<AudioSource> = found.into_values().collect();
        list.sort_by(|a, b| {
            b.suggested
                .cmp(&a.suggested)
                .then(b.active.cmp(&a.active))
                .then(a.name.cmp(&b.name))
        });
        Ok(list)
    })
}

/// Process loopback with "include tree" captures a process and its children,
/// so walk up to the top-most ancestor running the same executable
/// (browsers and Webex play audio from helper processes).
fn root_of_same_app(sys: &System, pid: u32) -> u32 {
    let mut cur = Pid::from_u32(pid);
    let name = match sys.process(cur) {
        Some(p) => p.name().to_os_string(),
        None => return pid,
    };
    for _ in 0..16 {
        let Some(parent) = sys.process(cur).and_then(|p| p.parent()) else { break };
        match sys.process(parent) {
            Some(pp) if pp.name() == name => cur = parent,
            _ => break,
        }
    }
    cur.as_u32()
}

// ---------------------------------------------------------------- capture

#[derive(Clone, Copy)]
pub enum Input {
    /// Audio played by this process tree.
    App(u32),
    /// Default recording device.
    Mic,
}

/// Peak level 0..1 for the UI, stored as f32 bits.
#[derive(Default)]
pub struct Level(AtomicU32);
impl Level {
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed)
    }
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

pub struct CaptureCtx {
    pub input: Input,
    pub speaker: Speaker,
    pub jobs: Sender<Job>,
    pub stop: Arc<AtomicBool>,
    /// While set, audio is still read (to keep timestamps in step) but
    /// replaced by silence, so nothing gets transcribed.
    pub paused: Arc<AtomicBool>,
    pub level: Arc<Level>,
    pub ids: Arc<AtomicU64>,
    pub started: Instant,
    pub partials: bool,
}

/// Opens the stream; returns once it is running (so start errors reach the UI)
/// and keeps capturing on a background thread until `stop` is set.
pub fn spawn_capture(ctx: CaptureCtx) -> Result<std::thread::JoinHandle<()>, String> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let handle = std::thread::Builder::new()
        .name("capture".into())
        .spawn(move || {
            if let Err(e) = capture_loop(&ctx, &ready_tx) {
                paths::log(format!("capture error: {e}"));
                let _ = ready_tx.send(Err(e));
            }
        })
        .map_err(|e| e.to_string())?;
    match ready_rx.recv() {
        Ok(Ok(())) => Ok(handle),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("nie udało się uruchomić nagrywania".into()),
    }
}

fn capture_loop(ctx: &CaptureCtx, ready: &std::sync::mpsc::Sender<Result<(), String>>) -> Result<(), String> {
    initialize_mta().ok().map_err(|e| format!("COM: {e}"))?;

    let format = WaveFormat::new(32, 32, &SampleType::Float, 48_000, 2, None);
    let mut client = match ctx.input {
        Input::App(pid) => AudioClient::new_application_loopback_client(pid, true)
            .map_err(|e| format!("Nie można przechwycić dźwięku aplikacji: {e}"))?,
        Input::Mic => {
            let dev = DeviceEnumerator::new()
                .and_then(|en| en.get_default_device(&Direction::Capture))
                .map_err(|e| format!("Brak mikrofonu: {e}"))?;
            dev.get_iaudioclient().map_err(|e| format!("Mikrofon: {e}"))?
        }
    };
    let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 200_000 };
    client
        .initialize_client(&format, &Direction::Capture, &mode)
        .map_err(|e| format!("Inicjalizacja audio: {e}"))?;
    let event = client.set_get_eventhandle().map_err(|e| e.to_string())?;
    let capture = client.get_audiocaptureclient().map_err(|e| e.to_string())?;
    client.start_stream().map_err(|e| e.to_string())?;
    let _ = ready.send(Ok(()));
    paths::log(format!("capture started: {:?}", ctx.speaker));

    let min_threshold = match ctx.input {
        Input::App(_) => 0.004,
        Input::Mic => 0.012,
    };
    let offset_ms = ctx.started.elapsed().as_millis() as u64;
    let mut seg = Segmenter::new(min_threshold, ctx.partials, offset_ms);
    let mut down = Downsampler::new();
    let mut raw: VecDeque<u8> = VecDeque::new();
    let mut floats: Vec<f32> = Vec::new();
    let mut mono: Vec<f32> = Vec::new();
    let mut events: Vec<VadEvent> = Vec::new();
    let mut next_id = || ctx.ids.fetch_add(1, Ordering::Relaxed);
    let mut level = 0.0f32;
    // When the app is silent, process loopback may deliver no packets at all;
    // feed silence so utterances still end and timestamps stay in step.
    let mut fed: u64 = 0;
    let stream_start = Instant::now();
    let mut was_paused = false;

    while !ctx.stop.load(Ordering::Relaxed) {
        let _ = event.wait_for_event(100);
        mono.clear();

        loop {
            match capture.get_next_packet_size() {
                Ok(Some(n)) if n > 0 => {}
                _ => break,
            }
            let before = raw.len();
            match capture.read_from_device_to_deque(&mut raw) {
                // the buffer content is undefined when flagged silent
                Ok(info) if info.flags.silent => raw.range_mut(before..).for_each(|b| *b = 0),
                Ok(_) => {}
                Err(_) => break,
            }
        }
        if !raw.is_empty() {
            let usable = raw.len() - raw.len() % 8; // whole stereo frames
            floats.clear();
            let bytes: Vec<u8> = raw.drain(..usable).collect();
            floats.extend(bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])));
            down.process(&floats, &mut mono);
        }

        let expected = stream_start.elapsed().as_millis() as u64 * RATE as u64 / 1000;
        let have = fed + mono.len() as u64;
        if have + (RATE as u64 / 5) < expected {
            // more than 200 ms behind real time: pad with silence
            mono.resize(mono.len() + (expected - have) as usize, 0.0);
        }
        fed += mono.len() as u64;

        let paused = ctx.paused.load(Ordering::Relaxed);
        if paused {
            if !was_paused {
                // finish the sentence that was in progress when pausing
                seg.flush(&mut events);
            }
            mono.iter_mut().for_each(|x| *x = 0.0);
        }
        was_paused = paused;

        if !mono.is_empty() {
            let peak = mono.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            level = peak.max(level * 0.8); // fast attack, slow release
            ctx.level.set(level);
            seg.push(&mono, &mut next_id, &mut events);
        } else {
            ctx.level.set(ctx.level.get() * 0.7);
        }
        if send(ctx, &mut events).is_err() {
            break;
        }
    }

    seg.flush(&mut events);
    let _ = send(ctx, &mut events);
    let _ = client.stop_stream();
    ctx.level.set(0.0);
    paths::log(format!("capture stopped: {:?}", ctx.speaker));
    Ok(())
}

fn send(ctx: &CaptureCtx, events: &mut Vec<VadEvent>) -> Result<(), ()> {
    for ev in events.drain(..) {
        let job = match ev {
            VadEvent::Partial { utt, start_ms, audio } => Job { id: utt, speaker: ctx.speaker, start_ms, audio, is_final: false },
            VadEvent::Final { utt, start_ms, audio } => Job { id: utt, speaker: ctx.speaker, start_ms, audio, is_final: true },
        };
        ctx.jobs.send(job).map_err(|_| ())?;
    }
    Ok(())
}
