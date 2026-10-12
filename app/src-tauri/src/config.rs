use serde::{Deserialize, Serialize};

use crate::paths;

/// User-editable settings, stored in %LOCALAPPDATA%\Notch Code\config.json.
/// The file is created with defaults on first launch.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Whisper language code ("pl", "en", ...) or "auto".
    pub language: String,
    /// Model file name from https://huggingface.co/ggerganov/whisper.cpp
    pub model: String,
    /// Use the GPU (Vulkan) when available.
    pub use_gpu: bool,
    /// CPU threads for whisper.cpp.
    pub threads: i32,
    /// Show live (not yet final) text while someone is still speaking.
    pub live_preview: bool,
    /// Start Notch Code when you sign in to Windows (tray menu: "Start with Windows").
    pub autostart: bool,
    /// Folder Claude works in (Code tab).
    pub project: Option<String>,
    /// Recently used project folders, newest first.
    pub recent_projects: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: "pl".into(),
            model: "ggml-large-v3-turbo-q5_0.bin".into(),
            use_gpu: true,
            threads: std::thread::available_parallelism()
                .map(|n| (n.get() as i32 / 2).clamp(2, 8))
                .unwrap_or(4),
            live_preview: true,
            autostart: true,
            project: None,
            recent_projects: Vec::new(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = paths::data_dir().join("config.json");
        let cfg = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| match serde_json::from_str::<Config>(&s) {
                Ok(c) => Some(c),
                Err(e) => {
                    paths::log(format!("config.json is invalid, using defaults: {e}"));
                    None
                }
            })
            .unwrap_or_default();
        // (re)write so the user can see every available option
        cfg.save();
        cfg
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(paths::data_dir().join("config.json"), json);
        }
    }
}
