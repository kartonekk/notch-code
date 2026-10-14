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
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::claude::{hidden, wait_for, AuthState};
use crate::paths;

const INSTALL_SCRIPT: &str = "https://antigravity.google/cli/install.ps1";
const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const LOGIN_POLL: Duration = Duration::from_millis(1500);
const MODELS_TIMEOUT: Duration = Duration::from_secs(15);
const USAGE_TIMEOUT: Duration = Duration::from_secs(30);
/// Don't ask `/usage` more often than this.
const USAGE_EVERY: Duration = Duration::from_secs(20);
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
const EXPIRED: &str = "Gemini's sign-in has expired. Sign in again in Settings.";

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GeminiModel {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct GeminiStatus {
    pub state: AuthState,
    pub message: Option<String>,
    /// The Google account it is signed in with.
    pub account: Option<String>,
    pub models: Vec<GeminiModel>,
    /// How much of the plan is used, from Antigravity's `/usage` (while signed in).
    pub usage: Option<GeminiUsage>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GeminiUsage {
    pub quotas: Vec<GeminiQuota>,
    /// Unix ms when it was read.
    pub at: u64,
}

/// One of the plan's limits. Models in a group share it.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct GeminiQuota {
    /// "Gemini Models", "Claude and GPT models"
    pub group: String,
    /// "Gemini Flash, Gemini Pro"
    pub models: Option<String>,
    /// "Weekly Limit Remaining"
    pub name: String,
    /// "weekly", "daily", …
    pub window: Option<String>,
    /// 0..1
    pub used: f64,
    /// Unix ms when it refills.
    pub resets_at: Option<u64>,
}

#[derive(Default)]
pub struct Gemini {
    status: Mutex<GeminiStatus>,
    usage: Mutex<Option<GeminiUsage>>,
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

fn set(app: &AppHandle, mut status: GeminiStatus) {
    {
        let g = app.state::<Gemini>();
        status.usage = if status.state == AuthState::SignedIn { g.usage.lock().unwrap().clone() } else { None };
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

pub fn parse_models(output: &str) -> Vec<GeminiModel> {
    let mut models = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (id, label) = if let Some((id, label)) = line.split_once('\t') {
            (id.trim(), label.trim())
        } else if let Some((id, label)) = line.split_once(char::is_whitespace) {
            (id.trim(), label.trim())
        } else {
            (line, line)
        };
        if id.ends_with(':') || id.eq_ignore_ascii_case("fetching") || id.eq_ignore_ascii_case("available") || id.eq_ignore_ascii_case("usage") {
            continue;
        }
        if !id.is_empty() {
            models.push(GeminiModel {
                id: id.to_string(),
                label: if label.is_empty() { id.to_string() } else { label.to_string() },
                description: Some(id.to_string()),
            });
        }
    }
    models
}

pub fn fetch_models() -> Vec<GeminiModel> {
    let Ok(mut cmd) = command() else {
        return Vec::new();
    };
    cmd.arg("models");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            paths::log(format!("gemini models spawn failed: {e}"));
            return Vec::new();
        }
    };
    let stdout = child.stdout.take();
    let status = match wait_for(&mut child, MODELS_TIMEOUT) {
        Ok(s) => s,
        Err(e) => {
            paths::log(format!("gemini models failed: {e}"));
            return Vec::new();
        }
    };
    if !status.success() {
        paths::log(format!("gemini models exited with {status}"));
        return Vec::new();
    }
    let Some(mut out) = stdout else {
        return Vec::new();
    };
    let mut buf = String::new();
    if out.read_to_string(&mut buf).is_err() {
        return Vec::new();
    }
    parse_models(&buf)
}

// ---------------------------------------------------------------- usage

/// The plan's limits, from Antigravity's own `/usage`: print mode answers it
/// without starting an agent turn (no quota spent, no conversation left behind).
pub fn usage() -> Option<Vec<GeminiQuota>> {
    let mut cmd = command().ok()?;
    cmd.args(["--print", "/usage", "--output-format", "json"]);
    if let Some(home) = dirs::home_dir() {
        cmd.current_dir(home);
    }
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let _ = wait_for(&mut child, USAGE_TIMEOUT);
    let text = reader.join().unwrap_or_default();
    let parsed = parse_usage(&text);
    if parsed.is_none() {
        paths::log(format!("gemini usage: unreadable: {}", text.lines().next().unwrap_or("").trim()));
    }
    parsed
}

/// `{"command":{"name":"usage","data":{"groups":[{"name","description","buckets":[…]}]}}}`
pub fn parse_usage(text: &str) -> Option<Vec<GeminiQuota>> {
    let groups = text.lines().rev().find_map(|l| {
        let v: serde_json::Value = serde_json::from_str(l.trim()).ok()?;
        v.pointer("/command/data/groups").and_then(|g| g.as_array()).cloned()
    })?;
    let str_of = |v: &serde_json::Value, key: &str| v.get(key).and_then(|s| s.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let mut quotas = Vec::new();
    for group in &groups {
        let name = str_of(group, "name").unwrap_or_else(|| "Models".into());
        let models = str_of(group, "description").map(|d| d.trim_start_matches("Models within this group:").trim().to_string()).filter(|m| !m.is_empty());
        for bucket in group.get("buckets").and_then(|b| b.as_array()).into_iter().flatten() {
            // a used-up limit leaves the fraction out (0 is the default value)
            let remaining = bucket.get("remaining_fraction").and_then(|f| f.as_f64()).unwrap_or(0.0);
            let resets_at = str_of(bucket, "reset_time")
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(&t).ok())
                .map(|t| t.timestamp_millis().max(0) as u64);
            quotas.push(GeminiQuota {
                group: name.clone(),
                models: models.clone(),
                name: str_of(bucket, "name").unwrap_or_else(|| "Limit".into()),
                window: str_of(bucket, "window"),
                used: (1.0 - remaining).clamp(0.0, 1.0),
                resets_at,
            });
        }
    }
    (!quotas.is_empty()).then_some(quotas)
}

/// Reads the plan usage again in the background (after a reply, on the sign-in
/// check). Asks made while a read is under way get one more read after it.
pub fn refresh_usage(app: &AppHandle) {
    static BUSY: AtomicBool = AtomicBool::new(false);
    static AGAIN: AtomicBool = AtomicBool::new(false);
    AGAIN.store(true, Ordering::SeqCst);
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || loop {
        while AGAIN.swap(false, Ordering::SeqCst) {
            let started = Instant::now();
            if let Some(quotas) = signed_in().then(usage).flatten() {
                let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
                let g = app.state::<Gemini>();
                *g.usage.lock().unwrap() = Some(GeminiUsage { quotas, at });
                let cur = g.status.lock().unwrap().clone();
                set(&app, cur);
            }
            std::thread::sleep(USAGE_EVERY.saturating_sub(started.elapsed()));
        }
        BUSY.store(false, Ordering::SeqCst);
        // an ask that came in just now, after the last check
        if !AGAIN.load(Ordering::SeqCst) || BUSY.swap(true, Ordering::SeqCst) {
            break;
        }
    });
}

fn check(app: &AppHandle) {
    if exe().is_none() {
        set(app, GeminiStatus { state: AuthState::NotInstalled, message: Some("Antigravity CLI isn't installed.".into()), ..Default::default() });
        return;
    }
    if !signed_in() {
        set(app, GeminiStatus { state: AuthState::SignedOut, message: Some("Not signed in.".into()), ..Default::default() });
        return;
    }
    if app.state::<Gemini>().auth_failed.load(Ordering::SeqCst) {
        set(app, GeminiStatus { state: AuthState::SignedOut, message: Some(EXPIRED.into()), ..Default::default() });
        return;
    }
    let prev_models = app.state::<Gemini>().status.lock().unwrap().models.clone();
    set(app, GeminiStatus { state: AuthState::SignedIn, account: account(), models: prev_models.clone(), ..Default::default() });
    refresh_usage(app);

    let models = fetch_models();
    if !models.is_empty() || prev_models.is_empty() {
        set(app, GeminiStatus { state: AuthState::SignedIn, account: account(), models, ..Default::default() });
    }
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

#[tauri::command(async)]
pub async fn gemini_models(app: AppHandle) -> Vec<GeminiModel> {
    let existing = app.state::<Gemini>().status.lock().unwrap().models.clone();
    if !existing.is_empty() {
        return existing;
    }
    if !signed_in() {
        return Vec::new();
    }
    let models = tauri::async_runtime::spawn_blocking(fetch_models).await.unwrap_or_default();
    if !models.is_empty() {
        let g = app.state::<Gemini>();
        let mut cur = g.status.lock().unwrap();
        cur.models = models.clone();
        let snap = cur.clone();
        drop(cur);
        let _ = app.emit("gemini", snap);
    }
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_agy_models() {
        let sample = "gemini-3.8-flash-high\tGemini 3.8 Flash (High)\r\nclaude-sonnet-4-6\tClaude Sonnet 4.6 (Thinking)\r\n";
        let parsed = parse_models(sample);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "gemini-3.8-flash-high");
        assert_eq!(parsed[0].label, "Gemini 3.8 Flash (High)");
        assert_eq!(parsed[1].id, "claude-sonnet-4-6");
        assert_eq!(parsed[1].label, "Claude Sonnet 4.6 (Thinking)");
    }

    #[test]
    fn parses_agy_usage() {
        let sample = r#"{"status":"SUCCESS","command":{"name":"usage","data":{"groups":[{"name":"Gemini Models","description":"Models within this group: Gemini Flash, Gemini Pro","buckets":[{"id":"gemini-weekly","name":"Weekly Limit Remaining","window":"weekly","remaining_fraction":0.25,"reset_time":"2026-10-21T07:47:06Z"}]},{"name":"Claude and GPT models","buckets":[{"id":"3p-weekly","name":"Weekly Limit Remaining","window":"weekly"}]}]}}}"#;
        let q = parse_usage(sample).unwrap();
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].group, "Gemini Models");
        assert_eq!(q[0].models.as_deref(), Some("Gemini Flash, Gemini Pro"));
        assert_eq!(q[0].used, 0.75);
        assert_eq!(q[0].resets_at, Some(1_792_568_826_000));
        assert_eq!(q[1].used, 1.0);
        assert_eq!(q[1].resets_at, None);
        assert_eq!(parse_usage("jetski: no output produced"), None);
    }
}
