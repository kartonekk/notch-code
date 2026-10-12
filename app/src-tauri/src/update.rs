//! Updates from GitHub Releases. CI publishes every push to main as release
//! `build-<run number>` with NotchCode.exe and stamps that number into the exe
//! (NOTCH_BUILD). The app only *looks* for a newer build and tells the user;
//! downloading and installing happen only when they click "Update".
//!
//! Installing: the new exe is downloaded next to the running one and checked,
//! the running exe is renamed to `.old` (Windows allows renaming a running
//! exe, not overwriting it), the new one takes its name and is started with
//! `--wait-pid <old pid>`, and this process exits.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::paths;

const REPO: &str = "kartonekk/notch-code";
const ASSET: &str = "NotchCode.exe";
const FIRST_CHECK: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// Build number of this exe; None for local builds, which never update.
pub fn current_build() -> Option<u64> {
    option_env!("NOTCH_BUILD").and_then(|s| s.trim().parse().ok())
}

#[derive(Clone, Copy, Debug, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateState {
    /// Nothing to offer (up to date, local build, or not checked yet).
    #[default]
    Idle,
    Available,
    Downloading,
    Error,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct UpdateStatus {
    pub state: UpdateState,
    pub current: Option<u64>,
    /// Build number of the newer release.
    pub available: Option<u64>,
    /// 0..1 while downloading.
    pub progress: Option<f32>,
    pub message: Option<String>,
}

#[derive(Default)]
pub struct Updater {
    status: Mutex<UpdateStatus>,
    asset: Mutex<Option<Asset>>,
    busy: AtomicBool,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    assets: Vec<Asset>,
}

#[derive(Clone, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    /// "sha256:<hex>", provided by GitHub for uploaded assets
    digest: Option<String>,
}

fn set(app: &AppHandle, f: impl FnOnce(&mut UpdateStatus)) {
    let snapshot = {
        let updater = app.state::<Updater>();
        let mut s = updater.status.lock().unwrap();
        f(&mut s);
        s.current = current_build();
        s.clone()
    };
    let _ = app.emit("update", snapshot);
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(None)
        .user_agent(format!("NotchCode/{}", current_build().unwrap_or(0)))
        .build()
        .map_err(|e| e.to_string())
}

/// Newest `build-<n>` release that has the exe attached.
fn latest(client: &reqwest::blocking::Client) -> Result<Option<(u64, Asset)>, String> {
    let resp = client
        .get(format!("https://api.github.com/repos/{REPO}/releases?per_page=30"))
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .timeout(Duration::from_secs(30))
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub: HTTP {}", resp.status()));
    }
    let body = resp.text().map_err(|e| e.to_string())?;
    let releases: Vec<Release> = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok(releases
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let n = r.tag_name.strip_prefix("build-")?.parse::<u64>().ok()?;
            let asset = r.assets.into_iter().find(|a| a.name == ASSET)?;
            Some((n, asset))
        })
        .max_by_key(|(n, _)| *n))
}

fn check(app: &AppHandle) {
    let Some(current) = current_build() else { return };
    let found = client().and_then(|c| latest(&c));
    let updater = app.state::<Updater>();
    match found {
        Ok(Some((n, asset))) if n > current => {
            paths::log(format!("update available: build {n} (running {current})"));
            *updater.asset.lock().unwrap() = Some(asset);
            set(app, |s| {
                *s = UpdateStatus { state: UpdateState::Available, available: Some(n), ..Default::default() };
            });
        }
        Ok(_) => {
            *updater.asset.lock().unwrap() = None;
            set(app, |s| *s = UpdateStatus::default());
        }
        // a failed check isn't worth bothering the user about; try again later
        Err(e) => paths::log(format!("update check failed: {e}")),
    }
}

/// Looks for a newer build shortly after launch and then every 15 minutes.
pub fn spawn_checker(app: AppHandle) {
    match current_build() {
        Some(n) => paths::log(format!("build {n}")),
        None => {
            paths::log("local build: update check off");
            return;
        }
    }
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        loop {
            let updater = app.state::<Updater>();
            if !updater.busy.swap(true, Ordering::SeqCst) {
                check(&app);
                updater.busy.store(false, Ordering::SeqCst);
            }
            std::thread::sleep(CHECK_INTERVAL);
        }
    });
}

fn download(client: &reqwest::blocking::Client, asset: &Asset, dest: &Path, progress: impl Fn(f32)) -> Result<(), String> {
    let mut resp = client.get(&asset.browser_download_url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let mut file = File::create(dest).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    let mut last = Instant::now();
    loop {
        let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hasher.update(&buf[..n]);
        done += n as u64;
        if last.elapsed() > Duration::from_millis(200) && asset.size > 0 {
            last = Instant::now();
            progress(done as f32 / asset.size as f32);
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);

    if done != asset.size {
        return Err(format!("download incomplete ({done} of {} bytes)", asset.size));
    }
    if let Some(expected) = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
        let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if !got.eq_ignore_ascii_case(expected) {
            return Err("downloaded file doesn't match its checksum".into());
        }
    }
    let mut magic = [0u8; 2];
    File::open(dest).and_then(|mut f| f.read_exact(&mut magic)).map_err(|e| e.to_string())?;
    if &magic != b"MZ" {
        return Err("downloaded file isn't a Windows program".into());
    }
    Ok(())
}

fn install(app: &AppHandle, recording: &dyn Fn() -> bool) -> Result<(), String> {
    let asset = app.state::<Updater>().asset.lock().unwrap().clone().ok_or("No update available")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let new = exe.with_extension("exe.new");
    let old = exe.with_extension("exe.old");

    set(app, |s| {
        s.state = UpdateState::Downloading;
        s.progress = Some(0.0);
        s.message = None;
    });
    let result = client().and_then(|c| {
        download(&c, &asset, &new, |p| set(app, |s| s.progress = Some(p)))
    });
    if let Err(e) = result {
        let _ = std::fs::remove_file(&new);
        return Err(e);
    }
    if recording() {
        let _ = std::fs::remove_file(&new);
        return Err("Finish the recording first".into());
    }

    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("can't replace the app: {e}"))?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("can't replace the app: {e}"));
    }
    paths::log("update installed, restarting");
    std::process::Command::new(&exe)
        .args(["--wait-pid", &std::process::id().to_string()])
        .spawn()
        .map_err(|e| format!("updated, but couldn't restart: {e}"))?;
    app.exit(0);
    Ok(())
}

/// Runs first thing on launch: after an update, waits for the previous
/// process to exit (so single-instance doesn't stop us) and removes its exe.
pub fn finish_pending() {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--wait-pid" {
            if let Some(pid) = args.next().and_then(|p| p.parse::<u32>().ok()) {
                wait_for_exit(pid);
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("exe.old"));
        let _ = std::fs::remove_file(exe.with_extension("exe.new"));
    }
}

#[cfg(windows)]
fn wait_for_exit(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !handle.is_null() {
            WaitForSingleObject(handle, 20_000);
            CloseHandle(handle);
        }
    }
}

#[cfg(not(windows))]
fn wait_for_exit(_pid: u32) {
    std::thread::sleep(Duration::from_secs(2));
}

// ---------------------------------------------------------------- commands

#[tauri::command]
pub fn update_status(state: tauri::State<Updater>) -> UpdateStatus {
    let mut s = state.status.lock().unwrap().clone();
    s.current = current_build();
    s
}

/// "Check for updates" from the tray.
#[tauri::command]
pub fn update_check(app: AppHandle) {
    std::thread::spawn(move || {
        let updater = app.state::<Updater>();
        if !updater.busy.swap(true, Ordering::SeqCst) {
            check(&app);
            updater.busy.store(false, Ordering::SeqCst);
        }
    });
}

/// The user clicked "Update": download, swap the exe and restart.
#[tauri::command]
pub fn update_install(app: AppHandle) -> Result<(), String> {
    let recording = |app: &AppHandle| app.state::<crate::AppState>().session.lock().unwrap().is_some();
    if recording(&app) {
        return Err("Finish the recording first".into());
    }
    std::thread::spawn(move || {
        let updater = app.state::<Updater>();
        if updater.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Err(e) = install(&app, &|| recording(&app)) {
            paths::log(format!("update failed: {e}"));
            set(&app, |s| {
                s.state = UpdateState::Error;
                s.progress = None;
                s.message = Some(e);
            });
        }
        updater.busy.store(false, Ordering::SeqCst);
    });
    Ok(())
}
