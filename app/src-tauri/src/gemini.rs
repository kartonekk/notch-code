//! Gemini through Antigravity CLI (`agy`, Google's coding agent), the second
//! agent a session can use. Gemini CLI stopped serving personal Google accounts
//! (AI Pro, Ultra, students) in June 2026; Antigravity CLI took its place.
//! Installed with Google's install script when missing, signed in with a
//! Google account in the browser, and run headless by agent.rs
//! (`--output-format stream-json`), one process per message.
//!
//! Signed in = Antigravity CLI saved its Google session in Windows Credential
//! Manager (a generic credential named like `gemini:antigravity`). It has no
//! sign-in command: its first interactive start signs in, so that runs in a
//! console window until the credential shows up.

use std::fs::File;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::claude::{hidden, wait_for, AuthState};
use crate::paths;

const INSTALL_SCRIPT: &str = "https://antigravity.google/cli/install.ps1";
const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const LOGIN_POLL: Duration = Duration::from_millis(1500);
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
const EXPIRED: &str = "Gemini's sign-in has expired. Sign in again in Settings.";

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

/// `agy.exe`: where the install script puts it, or anywhere on PATH.
fn exe() -> Option<PathBuf> {
    let installed = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("agy").join("bin").join("agy.exe"));
    let on_path = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("agy.exe")).find(|p| p.is_file()));
    installed.filter(|p| p.is_file()).or(on_path)
}

/// A hidden `agy` ready for arguments; Err says what's missing.
pub fn command() -> Result<Command, String> {
    let exe = exe().ok_or("Antigravity CLI isn't installed — install it in Settings.")?;
    Ok(hidden(exe))
}

// ---------------------------------------------------------------- saved sign-in

/// Antigravity's saved Google sessions in Credential Manager: (target, account).
#[cfg(windows)]
fn saved_logins() -> Vec<(Vec<u16>, String)> {
    use windows_sys::Win32::Security::Credentials::{CredEnumerateW, CredFree, CREDENTIALW};
    let wide = |p: *const u16| -> Vec<u16> {
        if p.is_null() {
            return Vec::new();
        }
        let mut n = 0;
        // SAFETY: Credential Manager's strings are NUL-terminated
        unsafe {
            while *p.add(n) != 0 {
                n += 1;
            }
            std::slice::from_raw_parts(p, n).to_vec()
        }
    };
    let mut count = 0u32;
    let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: on success `list` holds `count` credentials until CredFree
    if unsafe { CredEnumerateW(std::ptr::null(), 0, &mut count, &mut list) } == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..count as usize {
        // SAFETY: i < count
        let cred = unsafe { &**list.add(i) };
        let target = wide(cred.TargetName as *const u16);
        if String::from_utf16_lossy(&target).to_ascii_lowercase().contains("antigravity") {
            out.push((target, String::from_utf16_lossy(&wide(cred.UserName as *const u16))));
        }
    }
    // SAFETY: allocated by CredEnumerateW
    unsafe { CredFree(list as *const _) };
    out
}

#[cfg(not(windows))]
fn saved_logins() -> Vec<(Vec<u16>, String)> {
    Vec::new()
}

fn account() -> Option<String> {
    saved_logins().into_iter().map(|(_, user)| user).find(|u| u.contains('@'))
}

pub fn signed_in() -> bool {
    !saved_logins().is_empty()
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
    let status = if exe().is_none() {
        GeminiStatus { state: AuthState::NotInstalled, message: Some("Antigravity CLI isn't installed.".into()), ..Default::default() }
    } else if signed_in() && !app.state::<Gemini>().auth_failed.load(Ordering::SeqCst) {
        GeminiStatus { state: AuthState::SignedIn, account: account(), ..Default::default() }
    } else if signed_in() {
        GeminiStatus { state: AuthState::SignedOut, message: Some(EXPIRED.into()), ..Default::default() }
    } else {
        GeminiStatus { state: AuthState::SignedOut, message: Some("Not signed in.".into()), ..Default::default() }
    };
    set(app, status);
}

fn install(app: &AppHandle) {
    set(app, GeminiStatus { state: AuthState::Installing, message: Some("Installing Antigravity CLI…".into()), ..Default::default() });
    let log_path = paths::data_dir().join("gemini-install.log");
    let result = (|| -> Result<(), String> {
        let log = File::create(&log_path).map_err(|e| e.to_string())?;
        let log_err = log.try_clone().map_err(|e| e.to_string())?;
        let mut child = hidden("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &format!("irm {INSTALL_SCRIPT} | iex")])
            .stdout(log)
            .stderr(log_err)
            .spawn()
            .map_err(|e| e.to_string())?;
        let status = wait_for(&mut child, INSTALL_TIMEOUT)?;
        if !status.success() {
            return Err(format!("the installer exited with {status}"));
        }
        Ok(())
    })();
    match result {
        Ok(()) if exe().is_some() => {
            paths::log("gemini: installed");
            check(app);
        }
        other => {
            let why = other.err().unwrap_or_else(|| "agy not found after install".into());
            paths::log(format!("gemini install failed: {why} (see {})", log_path.display()));
            set(app, GeminiStatus {
                state: AuthState::NotInstalled,
                message: Some(format!("Couldn't install Antigravity CLI: {why}")),
                ..Default::default()
            });
        }
    }
}

/// Signs in with Google: Antigravity CLI's first start opens the browser; its
/// window closes once the session is saved (installs it first if needed).
fn login(app: &AppHandle) {
    if exe().is_none() {
        install(app);
    }
    let Some(exe) = exe() else { return };
    // a session that stopped working: drop it so a fresh sign-in is asked for
    if app.state::<Gemini>().auth_failed.load(Ordering::SeqCst) {
        forget_logins();
    }
    set(app, GeminiStatus {
        state: AuthState::SigningIn,
        message: Some("Finish signing in with Google in your browser…".into()),
        ..Default::default()
    });

    let mut cmd = Command::new(&exe);
    if let Some(home) = dirs::home_dir() {
        cmd.current_dir(home);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NEW_CONSOLE);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            set(app, GeminiStatus { state: AuthState::Error, message: Some(format!("Couldn't start the sign-in: {e}")), ..Default::default() });
            return;
        }
    };

    let deadline = Instant::now() + LOGIN_TIMEOUT;
    let mut done = false;
    while Instant::now() < deadline {
        std::thread::sleep(LOGIN_POLL);
        if signed_in() {
            done = true;
            break;
        }
        // the user closed the window
        if let Ok(Some(_)) = child.try_wait() {
            done = signed_in();
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    if done {
        paths::log("gemini: signed in");
        app.state::<Gemini>().auth_failed.store(false, Ordering::SeqCst);
        check(app);
    } else {
        paths::log("gemini sign-in didn't finish");
        set(app, GeminiStatus { state: AuthState::SignedOut, message: Some("Sign-in didn't finish. Try again.".into()), ..Default::default() });
    }
}

/// Removes Antigravity's saved sessions; false if one couldn't be removed.
#[cfg(windows)]
fn forget_logins() -> bool {
    use windows_sys::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};
    saved_logins().into_iter().all(|(mut target, _)| {
        target.push(0);
        // SAFETY: NUL-terminated target name
        unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) != 0 }
    })
}

#[cfg(not(windows))]
fn forget_logins() -> bool {
    true
}

fn logout(app: &AppHandle) {
    if !forget_logins() {
        return set(app, GeminiStatus { state: AuthState::Error, message: Some("Couldn't sign out.".into()), ..Default::default() });
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
