use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Moves a folder left by the app's former name ("Transkrypt") to its new
/// location, so the downloaded model, settings and transcripts carry over.
fn migrate_legacy(base: &Path, old: &str, dir: &Path) {
    let old = base.join(old);
    if !dir.exists() && old.is_dir() {
        let _ = std::fs::rename(&old, dir);
    }
}

/// %LOCALAPPDATA%\Notch Code
pub fn data_dir() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    let dir = base.join("Notch Code");
    migrate_legacy(&base, "Transkrypt", &dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn models_dir() -> PathBuf {
    let dir = data_dir().join("models");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Documents\Notch Code
pub fn transcripts_dir() -> PathBuf {
    let base = dirs::document_dir().unwrap_or_else(data_dir);
    let dir = base.join("Notch Code");
    migrate_legacy(&base, "Transkrypty", &dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Set once whisper.cpp reports that it runs on a Vulkan GPU.
pub static GPU_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static GPU_NAME: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Routes whisper.cpp / ggml diagnostics (via the `log` crate) into our log file.
struct WhisperLogger;

impl log::Log for WhisperLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let msg = record.args().to_string();
        let msg = msg.trim_end();
        if msg.is_empty() {
            return;
        }
        // e.g. "ggml_vulkan: 0 = NVIDIA GeForce RTX 3070 (NVIDIA) | uma: 0 | fp16: 1 ..."
        if let Some(rest) = msg.strip_prefix("ggml_vulkan: 0 = ") {
            let name = rest.split(" (").next().unwrap_or(rest).to_string();
            *GPU_NAME.lock().unwrap() = Some(name);
        }
        if msg.contains("using Vulkan") {
            GPU_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        // whisper.cpp is chatty at info level; keep only what helps diagnose setups
        let useful = record.level() <= log::Level::Warn
            || ["backend", "Vulkan", "ggml_vulkan", "model size", "loading model"].iter().any(|k| msg.contains(k));
        if useful {
            log(format!("[whisper] {msg}"));
        }
    }
    fn flush(&self) {}
}

pub fn init_whisper_logging() {
    static LOGGER: WhisperLogger = WhisperLogger;
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    whisper_rs::install_logging_hooks();
}

/// Appends a line to %LOCALAPPDATA%\Notch Code\notch-code.log (the release
/// build has no console, so this is the only place diagnostics end up).
pub fn log(msg: impl AsRef<str>) {
    let line = format!(
        "{} {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        msg.as_ref()
    );
    #[cfg(debug_assertions)]
    eprint!("{line}");
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(data_dir().join("notch-code.log"))
    {
        let _ = f.write_all(line.as_bytes());
    }
}
