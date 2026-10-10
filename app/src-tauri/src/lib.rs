mod audio;
mod config;
mod dsp;
mod paths;
mod setup;
mod transcriber;
mod transcript;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Size, LogicalSize, WebviewWindow};
use tauri_plugin_opener::OpenerExt;
use whisper_rs::WhisperContext;

use audio::{AudioSource, CaptureCtx, Input, Level};
use config::Config;
use transcriber::Speaker;
use transcript::TranscriptFile;

/// Width of the inverted "ear" corners drawn on both sides of the notch (CSS --ear).
const EAR: f64 = 12.0;

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "lowercase")]
enum Phase {
    #[default]
    Preparing,
    Ready,
    Recording,
    Error,
}

#[derive(Clone, Debug, Serialize, Default)]
struct AppStatus {
    phase: Phase,
    message: String,
    progress: Option<f32>,
    backend: Option<String>,
    source: Option<String>,
    started_at: Option<u64>,
    transcript_path: Option<String>,
    paused: bool,
}

struct Session {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    captures: Vec<JoinHandle<()>>,
    worker: JoinHandle<()>,
    levels_stop: Arc<AtomicBool>,
    file: Arc<Mutex<TranscriptFile>>,
    started: Instant,
    source: String,
}

#[derive(Default)]
struct AppState {
    status: Mutex<AppStatus>,
    model: Mutex<Option<Arc<WhisperContext>>>,
    session: Mutex<Option<Session>>,
    config: Mutex<Config>,
    setup_running: AtomicBool,
}

fn update_status(app: &AppHandle, f: impl FnOnce(&mut AppStatus)) {
    let state = app.state::<AppState>();
    let snapshot = {
        let mut s = state.status.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    let _ = app.emit("status", snapshot);
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------- setup

fn start_setup(app: AppHandle) {
    let state = app.state::<AppState>();
    if state.setup_running.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let result = prepare(&app);
        let state = app.state::<AppState>();
        state.setup_running.store(false, Ordering::SeqCst);
        if let Err(e) = result {
            paths::log(format!("setup failed: {e}"));
            update_status(&app, |s| {
                s.phase = Phase::Error;
                s.message = e;
                s.progress = None;
            });
        }
    });
}

fn prepare(app: &AppHandle) -> Result<(), String> {
    let cfg = Config::load();
    *app.state::<AppState>().config.lock().unwrap() = cfg.clone();

    update_status(app, |s| {
        s.phase = Phase::Preparing;
        s.message = "Sprawdzam model rozpoznawania mowy…".into();
        s.progress = None;
    });

    let path = setup::ensure_model(&cfg.model, |done, total| {
        update_status(app, |s| {
            s.message = if total > 0 {
                format!("Pobieranie modelu: {} / {} MB", done / 1_048_576, total / 1_048_576)
            } else {
                format!("Pobieranie modelu: {} MB", done / 1_048_576)
            };
            s.progress = (total > 0).then(|| done as f32 / total as f32);
        });
    })?;

    update_status(app, |s| {
        s.message = "Wczytuję model na kartę graficzną…".into();
        s.progress = None;
    });
    let ctx = transcriber::load_model(&path, &cfg)?;
    update_status(app, |s| s.message = "Rozgrzewam silnik…".into());
    let took = transcriber::warm_up(&ctx, &cfg)?;
    let backend = if paths::GPU_ACTIVE.load(Ordering::Relaxed) {
        let name = paths::GPU_NAME.lock().unwrap().clone().unwrap_or_default();
        let short = name.trim_start_matches("NVIDIA ").trim_start_matches("GeForce ").trim_start_matches("AMD ");
        if short.is_empty() { "GPU".to_string() } else { format!("GPU · {short}") }
    } else {
        "CPU".to_string()
    };
    paths::log(format!("model ready ({backend}), warm-up {} ms", took.as_millis()));
    *app.state::<AppState>().model.lock().unwrap() = Some(Arc::new(ctx));

    update_status(app, |s| {
        s.phase = Phase::Ready;
        s.message = "Gotowy".into();
        s.progress = None;
        s.backend = Some(backend);
    });

    // Debug builds: TRANSKRYPT_AUTOSTART=<exe name fragment> starts recording
    // that app right away, for end-to-end tests without clicking.
    #[cfg(debug_assertions)]
    if let Ok(want) = std::env::var("TRANSKRYPT_AUTOSTART") {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            for _ in 0..60 {
                let hit = audio::list_sources().ok().and_then(|l| {
                    l.into_iter().find(|s| s.exe.to_lowercase().contains(&want.to_lowercase()))
                });
                if let Some(src) = hit {
                    paths::log(format!("autostart: {} (pid {})", src.exe, src.pid));
                    if let Err(e) = start_recording(app.clone(), src.pid, false).await {
                        paths::log(format!("autostart failed: {e}"));
                    }
                    return;
                }
                tokio_sleep(1000).await;
            }
            paths::log("autostart: source not found");
        });
    }
    Ok(())
}

#[cfg(debug_assertions)]
async fn tokio_sleep(ms: u64) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(Duration::from_millis(ms))).await;
}

// ---------------------------------------------------------------- commands

#[tauri::command]
fn get_status(state: tauri::State<AppState>) -> AppStatus {
    state.status.lock().unwrap().clone()
}

#[tauri::command]
fn retry_setup(app: AppHandle) {
    start_setup(app);
}

#[tauri::command]
async fn list_sources() -> Result<Vec<AudioSource>, String> {
    audio::list_sources()
}

#[tauri::command]
async fn start_recording(app: AppHandle, pid: u32, include_mic: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    if state.session.lock().unwrap().is_some() {
        return Err("Nagrywanie już trwa".into());
    }
    let model = state.model.lock().unwrap().clone().ok_or("Model nie jest jeszcze gotowy")?;
    let cfg = state.config.lock().unwrap().clone();

    let source_name = audio::list_sources()
        .ok()
        .and_then(|l| l.into_iter().find(|s| s.pid == pid).map(|s| s.name))
        .unwrap_or_else(|| "Aplikacja".into());

    let file = TranscriptFile::create(&source_name)?;
    let path = file.path.to_string_lossy().to_string();
    let file = Arc::new(Mutex::new(file));

    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let ids = Arc::new(AtomicU64::new(1));
    let started = Instant::now();
    let app_level = Arc::new(Level::default());
    let mic_level = Arc::new(Level::default());

    let mut captures = Vec::new();
    let mk = |input, speaker, level: &Arc<Level>| CaptureCtx {
        input,
        speaker,
        jobs: tx.clone(),
        stop: stop.clone(),
        paused: paused.clone(),
        level: level.clone(),
        ids: ids.clone(),
        started,
        partials: cfg.live_preview,
    };
    let fail = |e: String, stop: &AtomicBool| {
        stop.store(true, Ordering::SeqCst);
        e
    };
    captures.push(audio::spawn_capture(mk(Input::App(pid), Speaker::Others, &app_level)).map_err(|e| fail(e, &stop))?);
    if include_mic {
        match audio::spawn_capture(mk(Input::Mic, Speaker::Me, &mic_level)) {
            Ok(h) => captures.push(h),
            // no microphone is not fatal: keep transcribing the app
            Err(e) => paths::log(format!("microphone unavailable: {e}")),
        }
    }
    drop(tx);

    let (app2, file2) = (app.clone(), file.clone());
    let worker = std::thread::Builder::new()
        .name("whisper".into())
        .spawn(move || {
            transcriber::run(model, cfg, rx, |seg| {
                if seg.is_final && !seg.text.is_empty() {
                    file2.lock().unwrap().append(&seg);
                }
                let _ = app2.emit("segment", seg);
            });
        })
        .map_err(|e| e.to_string())?;

    let levels_stop = Arc::new(AtomicBool::new(false));
    {
        let (app3, stop3) = (app.clone(), levels_stop.clone());
        std::thread::spawn(move || {
            while !stop3.load(Ordering::Relaxed) {
                let _ = app3.emit("levels", serde_json::json!({ "app": app_level.get(), "mic": mic_level.get() }));
                std::thread::sleep(Duration::from_millis(70));
            }
        });
    }

    *state.session.lock().unwrap() = Some(Session {
        stop,
        paused,
        captures,
        worker,
        levels_stop,
        file,
        started,
        source: source_name.clone(),
    });
    update_status(&app, |s| {
        s.phase = Phase::Recording;
        s.message = "Nagrywanie".into();
        s.source = Some(source_name);
        s.started_at = Some(now_unix_ms());
        s.transcript_path = Some(path);
        s.paused = false;
    });
    Ok(())
}

/// Pauses or resumes a running transcription; the session (and its file) stays open.
#[tauri::command]
fn pause_recording(app: AppHandle, paused: bool) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let session = state.session.lock().unwrap();
        let s = session.as_ref().ok_or("Nagrywanie nie trwa")?;
        if s.paused.swap(paused, Ordering::SeqCst) == paused {
            return Ok(());
        }
        let at = s.started.elapsed().as_millis() as u64;
        s.file.lock().unwrap().mark(if paused { "pauza" } else { "wznowiono" }, at);
    }
    update_status(&app, |s| {
        s.paused = paused;
        s.message = if paused { "Wstrzymano" } else { "Nagrywanie" }.into();
    });
    Ok(())
}

/// Ends the transcription. A non-empty `title` (the meeting subject) renames
/// the transcript file to "<date> <title>.md".
#[tauri::command]
async fn stop_recording(app: AppHandle, title: Option<String>) -> Result<Option<String>, String> {
    let session = app.state::<AppState>().session.lock().unwrap().take();
    let Some(session) = session else { return Ok(None) };
    update_status(&app, |s| s.message = "Kończę transkrypcję…".into());

    session.stop.store(true, Ordering::SeqCst);
    for h in session.captures {
        let _ = h.join();
    }
    // the worker drains the remaining utterances, then exits
    let _ = session.worker.join();
    session.levels_stop.store(true, Ordering::SeqCst);

    let mut file = session.file.lock().unwrap();
    file.finish();
    let title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    let path = match title {
        Some(t) => match file.rename_to_subject(&t, &session.source) {
            Ok(p) => p,
            Err(e) => {
                paths::log(format!("rename failed: {e}"));
                file.path.clone()
            }
        },
        None => file.path.clone(),
    };
    drop(file);
    let path = path.to_string_lossy().to_string();

    update_status(&app, |s| {
        s.phase = Phase::Ready;
        s.message = "Gotowy".into();
        s.started_at = None;
        s.source = None;
        s.paused = false;
        s.transcript_path = Some(path.clone());
    });
    Ok(Some(path))
}

#[tauri::command]
fn set_notch_size(window: WebviewWindow, width: f64, height: f64) -> Result<(), String> {
    place_notch(&window, width, height).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_transcript(app: AppHandle) -> Result<(), String> {
    let path = app.state::<AppState>().status.lock().unwrap().transcript_path.clone();
    match path {
        Some(p) => app.opener().open_path(p, None::<&str>).map_err(|e| e.to_string()),
        None => open_folder(&app),
    }
}

#[tauri::command]
fn open_transcripts_folder(app: AppHandle) -> Result<(), String> {
    open_folder(&app)
}

fn open_folder(app: &AppHandle) -> Result<(), String> {
    let dir = paths::transcripts_dir();
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn quit_app(app: AppHandle) {
    let _ = stop_recording(app.clone(), None).await;
    app.exit(0);
}

// ---------------------------------------------------------------- window

/// Resizes the window to the notch (+ ears) and pins it to the top centre of
/// the primary monitor.
fn place_notch(window: &WebviewWindow, width: f64, height: f64) -> tauri::Result<()> {
    let monitor = window
        .primary_monitor()?
        .or(window.current_monitor()?);
    let Some(monitor) = monitor else { return Ok(()) };
    let scale = monitor.scale_factor();
    let w = width + EAR * 2.0;
    let phys_w = (w * scale).round();
    let x = monitor.position().x as f64 + (monitor.size().width as f64 - phys_w) / 2.0;
    let y = monitor.position().y;

    // Move and resize in one call: separate set_size + set_position calls show
    // one frame where the window has the new size at the old x, so the centred
    // notch visibly jumps sideways.
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};
        let hwnd = window.hwnd()?.0;
        let phys_h = (height * scale).round();
        unsafe {
            SetWindowPos(
                hwnd as _,
                std::ptr::null_mut(),
                x.round() as i32,
                y,
                phys_w as i32,
                phys_h as i32,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
    #[cfg(not(windows))]
    {
        window.set_size(Size::Logical(LogicalSize::new(w, height)))?;
        window.set_position(PhysicalPosition::new(x.round() as i32, y))?;
    }
    Ok(())
}

// ---------------------------------------------------------------- app

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    std::panic::set_hook(Box::new(|info| paths::log(format!("PANIC: {info}"))));
    paths::log("Transkrypt starting");
    paths::init_whisper_logging();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_status,
            retry_setup,
            list_sources,
            start_recording,
            stop_recording,
            pause_recording,
            set_notch_size,
            open_transcript,
            open_transcripts_folder,
            quit_app,
        ])
        .setup(|app| {
            let window = app.get_webview_window("main").expect("main window");
            place_notch(&window, 240.0, 36.0)?;
            window.show()?;

            let open_item = MenuItem::with_id(app, "folder", "Folder z transkryptami", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Zakończ", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_item, &quit_item])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().expect("icon"))
                .tooltip("Transkrypt")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "folder" => {
                        let _ = open_folder(app);
                    }
                    "quit" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move { quit_app(app).await });
                    }
                    _ => {}
                })
                .build(app)?;

            start_setup(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
