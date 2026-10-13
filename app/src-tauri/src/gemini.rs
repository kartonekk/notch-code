//! Gemini CLI (Google's coding agent), the second agent a session can use.
//! Installed with npm when missing, signed in with a Google account (the
//! Gemini subscription) in its own console window, and run headless by
//! agent.rs (`--output-format stream-json`), one process per message.
//!
//! It runs as `node <gemini script>` rather than through npm's `gemini.cmd`, so
//! prompts reach it as plain arguments with no batch-file quoting involved.
//!
//! Signed in = Gemini CLI has saved Google credentials (~/.gemini/oauth_creds.json)
//! or a GEMINI_API_KEY is set.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::claude::{hidden, wait_for, AuthState};
use crate::paths;

const PACKAGE: &str = "@google/gemini-cli";
const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const LOGIN_POLL: Duration = Duration::from_secs(2);
const EXPIRED: &str = "Gemini's sign-in has expired. Sign in again in Settings.";

#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

#[derive(Clone, Debug, Serialize, Default)]
pub struct GeminiStatus {
    pub state: AuthState,
    pub message: Option<String>,
    /// The Google account it is signed in with.
    pub account: Option<String>,
}

#[derive(Default)]
pub struct Gemini {
    status: Mutex<GeminiStatus>,
    busy: AtomicBool,
    /// A run failed to authenticate although credentials are saved.
    auth_failed: AtomicBool,
}

// ---------------------------------------------------------------- finding it

fn on_path(exe: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(exe)).find(|p| p.is_file()))
}

fn node() -> Option<PathBuf> {
    on_path("node.exe").or_else(|| {
        let pf = std::env::var_os("ProgramFiles").map(PathBuf::from)?;
        Some(pf.join("nodejs").join("node.exe")).filter(|p| p.is_file())
    })
}

/// The installed package's folder: npm's global folder, or next to a `gemini.cmd` on PATH.
fn package_dir() -> Option<PathBuf> {
    let rel = Path::new("node_modules").join("@google").join("gemini-cli");
    let global = std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("npm").join(&rel));
    let beside = on_path("gemini.cmd").and_then(|c| c.parent().map(|d| d.join(&rel)));
    global.into_iter().chain(beside).find(|d| d.join("package.json").is_file())
}

/// The script `gemini` runs, from the package's `bin`.
fn script() -> Option<PathBuf> {
    let dir = package_dir()?;
    let pkg: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("package.json")).ok()?).ok()?;
    let bin = match pkg.get("bin")? {
        Value::String(s) => s.clone(),
        Value::Object(m) => m.get("gemini").or_else(|| m.values().next())?.as_str()?.to_string(),
        _ => return None,
    };
    Some(dir.join(bin)).filter(|p| p.is_file())
}

/// A hidden `node <gemini>` ready for arguments; Err says what's missing.
pub fn command() -> Result<Command, String> {
    let node = node().ok_or("Gemini needs Node.js — install it from nodejs.org, then sign in to Gemini in Settings.")?;
    let script = script().ok_or("Gemini CLI isn't installed — install it in Settings.")?;
    let mut cmd = hidden(node);
    cmd.arg(script);
    Ok(cmd)
}

fn home() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".gemini"))
}

fn creds() -> Option<PathBuf> {
    home().map(|h| h.join("oauth_creds.json"))
}

fn api_key() -> bool {
    std::env::var("GEMINI_API_KEY").is_ok_and(|k| !k.trim().is_empty())
}

fn account() -> Option<String> {
    let text = std::fs::read_to_string(home()?.join("google_accounts.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("active").and_then(Value::as_str).map(str::to_string)
}

pub fn signed_in() -> bool {
    api_key() || creds().is_some_and(|c| c.is_file())
}

// ---------------------------------------------------------------- state

fn set(app: &AppHandle, status: GeminiStatus) {
    {
        let g = app.state::<Gemini>();
        let mut cur = g.status.lock().unwrap();
        if cur.state != status.state {
            paths::log(format!("gemini: {:?}", status.state));
        }
        *cur = status.clone();
    }
    let _ = app.emit("gemini", status);
}

fn exclusive(app: &AppHandle, f: impl FnOnce()) {
    let g = app.state::<Gemini>();
    if g.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    f();
    g.busy.store(false, Ordering::SeqCst);
}

fn check(app: &AppHandle) {
    let status = if node().is_none() {
        GeminiStatus {
            state: AuthState::NotInstalled,
            message: Some("Gemini CLI needs Node.js. Install it from nodejs.org, then come back here.".into()),
            ..Default::default()
        }
    } else if script().is_none() {
        GeminiStatus { state: AuthState::NotInstalled, message: Some("Gemini CLI isn't installed.".into()), ..Default::default() }
    } else if signed_in() && !app.state::<Gemini>().auth_failed.load(Ordering::SeqCst) {
        GeminiStatus {
            state: AuthState::SignedIn,
            account: account().or_else(|| api_key().then(|| "API key".to_string())),
            ..Default::default()
        }
    } else if signed_in() {
        GeminiStatus { state: AuthState::SignedOut, message: Some(EXPIRED.into()), ..Default::default() }
    } else {
        GeminiStatus { state: AuthState::SignedOut, message: Some("Not signed in.".into()), ..Default::default() }
    };
    set(app, status);
}

fn install(app: &AppHandle) {
    if node().is_none() {
        return check(app);
    }
    set(app, GeminiStatus { state: AuthState::Installing, message: Some("Installing Gemini CLI…".into()), ..Default::default() });
    let log_path = paths::data_dir().join("gemini-install.log");
    let result = (|| -> Result<(), String> {
        let log = File::create(&log_path).map_err(|e| e.to_string())?;
        let log_err = log.try_clone().map_err(|e| e.to_string())?;
        let mut child = hidden("cmd.exe")
            .args(["/C", "npm", "install", "-g", PACKAGE])
            .stdout(log)
            .stderr(log_err)
            .spawn()
            .map_err(|e| e.to_string())?;
        let status = wait_for(&mut child, INSTALL_TIMEOUT)?;
        if !status.success() {
            return Err(format!("npm exited with {status}"));
        }
        Ok(())
    })();
    match result {
        Ok(()) if script().is_some() => {
            paths::log("gemini: installed");
            check(app);
        }
        other => {
            let why = other.err().unwrap_or_else(|| "gemini not found after install".into());
            paths::log(format!("gemini install failed: {why} (see {})", log_path.display()));
            set(app, GeminiStatus {
                state: AuthState::NotInstalled,
                message: Some(format!("Couldn't install Gemini CLI: {why}")),
                ..Default::default()
            });
        }
    }
}

/// Makes Gemini CLI sign in with a Google account without asking how first.
fn prefer_google_login() {
    let Some(dir) = home() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("settings.json");
    let mut settings: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !settings.is_object() || settings.pointer("/security/auth/selectedType").is_some() {
        return;
    }
    let obj = settings.as_object_mut().unwrap();
    let security = obj.entry("security").or_insert_with(|| serde_json::json!({}));
    if let Some(sec) = security.as_object_mut() {
        let auth = sec.entry("auth").or_insert_with(|| serde_json::json!({}));
        if let Some(auth) = auth.as_object_mut() {
            auth.insert("selectedType".into(), Value::String("oauth-personal".into()));
        }
    }
    if let Ok(text) = serde_json::to_string_pretty(&settings) {
        let _ = std::fs::write(path, text);
    }
}

/// Opens Gemini CLI in a console window: it sends the user to Google's sign-in
/// in the browser. Done once its credentials are saved; the window is closed then.
fn login(app: &AppHandle) {
    let Some(node) = node() else { return check(app) };
    if script().is_none() {
        install(app);
    }
    let Some(script) = script() else { return };
    set(app, GeminiStatus {
        state: AuthState::SigningIn,
        message: Some("Finish signing in with Google in your browser…".into()),
        ..Default::default()
    });
    prefer_google_login();
    let before = creds().and_then(|c| c.metadata().ok()).and_then(|m| m.modified().ok());
    let mut cmd = Command::new(node);
    cmd.arg(script);
    if let Some(h) = dirs::home_dir() {
        cmd.current_dir(h);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NEW_CONSOLE);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return set(app, GeminiStatus {
                state: AuthState::Error,
                message: Some(format!("Couldn't start Gemini sign-in: {e}")),
                ..Default::default()
            });
        }
    };
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    while Instant::now() < deadline {
        std::thread::sleep(LOGIN_POLL);
        let now = creds().and_then(|c| c.metadata().ok()).and_then(|m| m.modified().ok());
        if now.is_some() && now != before {
            // give it a moment to finish writing the account file
            std::thread::sleep(Duration::from_secs(1));
            crate::agent::kill_tree(child.id());
            let _ = child.wait();
            app.state::<Gemini>().auth_failed.store(false, Ordering::SeqCst);
            return check(app);
        }
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
    }
    crate::agent::kill_tree(child.id());
    let _ = child.wait();
    set(app, GeminiStatus { state: AuthState::SignedOut, message: Some("Sign-in didn't finish.".into()), ..Default::default() });
}

fn logout(app: &AppHandle) {
    if let Some(c) = creds().filter(|c| c.is_file()) {
        if let Err(e) = std::fs::remove_file(&c) {
            return set(app, GeminiStatus {
                state: AuthState::Error,
                message: Some(format!("Couldn't sign out: {e}")),
                ..Default::default()
            });
        }
    }
    app.state::<Gemini>().auth_failed.store(false, Ordering::SeqCst);
    check(app);
}

/// A run failed to authenticate.
pub fn report_auth_failure(app: &AppHandle) {
    app.state::<Gemini>().auth_failed.store(true, Ordering::SeqCst);
    set(app, GeminiStatus { state: AuthState::SignedOut, message: Some(EXPIRED.into()), ..Default::default() });
}

/// A run went through.
pub fn report_auth_ok(app: &AppHandle) {
    if app.state::<Gemini>().auth_failed.swap(false, Ordering::SeqCst) {
        let app = app.clone();
        std::thread::spawn(move || exclusive(&app, || check(&app)));
    }
}

pub fn spawn_monitor(app: AppHandle) {
    std::thread::spawn(move || exclusive(&app, || check(&app)));
}

#[tauri::command]
pub fn gemini_status(state: tauri::State<Gemini>) -> GeminiStatus {
    state.status.lock().unwrap().clone()
}

#[tauri::command]
pub fn gemini_refresh(app: AppHandle) {
    std::thread::spawn(move || exclusive(&app, || check(&app)));
}

#[tauri::command]
pub fn gemini_login(app: AppHandle) {
    std::thread::spawn(move || exclusive(&app, || login(&app)));
}

#[tauri::command]
pub fn gemini_logout(app: AppHandle) {
    std::thread::spawn(move || exclusive(&app, || logout(&app)));
}

#[tauri::command]
pub fn gemini_install(app: AppHandle) {
    std::thread::spawn(move || exclusive(&app, || install(&app)));
}
