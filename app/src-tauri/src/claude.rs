//! Claude Code CLI: finds the `claude` binary (installing it with the official
//! installer when it is missing) and watches its sign-in, so the notch can
//! remind the user when Claude has to be signed in again.
//!
//! Sign-in state comes from `claude auth status --json`, checked at startup,
//! every CHECK_INTERVAL and (throttled) whenever the notch is opened.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::paths;

const CHECK_INTERVAL: Duration = Duration::from_secs(30 * 60);
/// Opening the notch re-checks the sign-in at most this often.
const MIN_RECHECK: Duration = Duration::from_secs(5 * 60);
const STATUS_TIMEOUT: Duration = Duration::from_secs(30);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const LOGIN_POLL: Duration = Duration::from_secs(3);
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
const INSTALL_COMMAND: &str = "irm https://claude.ai/install.ps1 | iex";
const PLUGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Caveman plugin: terse answers, far fewer output tokens.
const CAVEMAN_ID: &str = "caveman@caveman";
const CAVEMAN_MARKETPLACE: &str = "JuliusBrussee/caveman";
/// Caveman level for the app's own Claude runs (`CAVEMAN_DEFAULT_MODE`).
pub const CAVEMAN_MODE: &str = "ultra";
const EXPIRED: &str = "Claude's sign-in has expired. Sign in again.";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

#[derive(Clone, Copy, Debug, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    #[default]
    Checking,
    Installing,
    NotInstalled,
    SignedOut,
    SigningIn,
    SignedIn,
    Error,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct ClaudeStatus {
    pub state: AuthState,
    /// What to tell the user, e.g. why the check failed.
    pub message: Option<String>,
    /// Subscription reported by the CLI ("pro", "max", …).
    pub plan: Option<String>,
}

#[derive(Default)]
pub struct Claude {
    status: Mutex<ClaudeStatus>,
    last_check: Mutex<Option<Instant>>,
    /// An install, sign-in or check is running.
    busy: AtomicBool,
    /// A real request failed to authenticate. `auth status` can still say
    /// "signed in" then (a session that expired and can't be refreshed), so
    /// this wins until a sign-in completes or a request succeeds again.
    auth_failed: AtomicBool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthJson {
    logged_in: bool,
    subscription_type: Option<String>,
}

// ---------------------------------------------------------------- process helpers

pub(crate) fn hidden(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Waits for `child` to exit; kills it after `timeout`.
pub(crate) fn wait_for(child: &mut Child, timeout: Duration) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(200)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("timed out".into());
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// The installed CLI: the native installer's location first, then PATH.
pub fn find_cli() -> Option<PathBuf> {
    let exe = "claude.exe";
    let local = dirs::home_dir().map(|h| h.join(".local").join("bin").join(exe));
    let on_path: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join(exe)).collect())
        .unwrap_or_default();
    local.into_iter().chain(on_path).find(|p| p.is_file())
}

// ---------------------------------------------------------------- state

fn set(app: &AppHandle, status: ClaudeStatus) {
    let changed = {
        let claude = app.state::<Claude>();
        let mut cur = claude.status.lock().unwrap();
        let changed = cur.state != status.state;
        *cur = status.clone();
        changed
    };
    if changed {
        paths::log(format!("claude: {:?}", status.state));
        let tooltip = match status.state {
            AuthState::SignedOut | AuthState::Error => "Notch Code — Claude needs you to sign in",
            AuthState::NotInstalled => "Notch Code — Claude Code is not installed",
            _ => "Notch Code",
        };
        if let Some(tray) = app.tray_by_id("main") {
            let _ = tray.set_tooltip(Some(tooltip));
        }
    }
    let _ = app.emit("claude", status);
}

/// Runs `f` unless another install / sign-in / check is already running.
fn exclusive(app: &AppHandle, f: impl FnOnce()) {
    let claude = app.state::<Claude>();
    if claude.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    f();
    claude.busy.store(false, Ordering::SeqCst);
}

// ---------------------------------------------------------------- operations

/// Asks the CLI whether it is signed in. `Ok(None)` = signed out.
fn auth_status(cli: &Path) -> Result<Option<Option<String>>, String> {
    let mut child = hidden(cli)
        .args(["auth", "status", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    wait_for(&mut child, STATUS_TIMEOUT)?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    match serde_json::from_slice::<AuthJson>(&out.stdout) {
        Ok(a) if a.logged_in => Ok(Some(a.subscription_type)),
        Ok(_) => Ok(None),
        Err(_) => {
            let err = String::from_utf8_lossy(&out.stderr);
            let line = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("unreadable output");
            Err(line.trim().to_string())
        }
    }
}

fn check(app: &AppHandle) {
    *app.state::<Claude>().last_check.lock().unwrap() = Some(Instant::now());
    let Some(cli) = find_cli() else {
        set(app, ClaudeStatus {
            state: AuthState::NotInstalled,
            message: Some("Claude Code is not installed.".into()),
            ..Default::default()
        });
        return;
    };
    let status = match auth_status(&cli) {
        Ok(Some(_)) if app.state::<Claude>().auth_failed.load(Ordering::SeqCst) => ClaudeStatus {
            state: AuthState::SignedOut,
            message: Some(EXPIRED.into()),
            ..Default::default()
        },
        Ok(Some(plan)) => {
            crate::agent::refresh_usage(app);
            ClaudeStatus { state: AuthState::SignedIn, plan, ..Default::default() }
        }
        Ok(None) => ClaudeStatus {
            state: AuthState::SignedOut,
            message: Some("Claude needs you to sign in again.".into()),
            ..Default::default()
        },
        Err(e) => {
            paths::log(format!("claude auth status failed: {e}"));
            ClaudeStatus {
                state: AuthState::Error,
                message: Some(format!("Couldn't check Claude's sign-in: {e}")),
                ..Default::default()
            }
        }
    };
    set(app, status);
}

/// Runs the official installer (it puts claude.exe in %USERPROFILE%\.local\bin).
fn install(app: &AppHandle) {
    set(app, ClaudeStatus {
        state: AuthState::Installing,
        message: Some("Installing Claude Code…".into()),
        ..Default::default()
    });
    let log_path = paths::data_dir().join("claude-install.log");
    let result = (|| -> Result<(), String> {
        let log = File::create(&log_path).map_err(|e| e.to_string())?;
        let log_err = log.try_clone().map_err(|e| e.to_string())?;
        let mut child = hidden("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", INSTALL_COMMAND])
            .stdout(log)
            .stderr(log_err)
            .spawn()
            .map_err(|e| e.to_string())?;
        let status = wait_for(&mut child, INSTALL_TIMEOUT)?;
        if !status.success() {
            return Err(format!("installer exited with {status}"));
        }
        Ok(())
    })();
    match result {
        Ok(()) if find_cli().is_some() => {
            paths::log("claude: installed");
            check(app);
        }
        other => {
            let why = other.err().unwrap_or_else(|| "claude.exe not found after install".into());
            paths::log(format!("claude install failed: {why} (see {})", log_path.display()));
            set(app, ClaudeStatus {
                state: AuthState::NotInstalled,
                message: Some(format!("Couldn't install Claude Code: {why}")),
                ..Default::default()
            });
        }
    }
}

/// The caveman level the app's runs should get: the plugin is enabled in
/// `~/.claude/settings.json`. Each run then confirms it (see `caveman_level`).
pub fn caveman_expected() -> Option<String> {
    let settings = dirs::home_dir()?.join(".claude").join("settings.json");
    let settings: Value = serde_json::from_str(&std::fs::read_to_string(settings).ok()?).ok()?;
    let enabled = settings.pointer("/enabledPlugins").and_then(|p| p.get(CAVEMAN_ID)).and_then(Value::as_bool) == Some(true);
    enabled.then(|| CAVEMAN_MODE.to_string())
}

/// The level in a caveman hook's output: "CAVEMAN MODE ACTIVE — level: ultra"
/// (session start) or "CAVEMAN MODE ACTIVE (lite)" (each prompt).
pub fn caveman_level(output: &str) -> Option<String> {
    const MARK: &str = "CAVEMAN MODE ACTIVE";
    let rest = &output[output.find(MARK)? + MARK.len()..];
    let rest = match rest.strip_prefix(" (") {
        Some(r) => r,
        // the dash may arrive mangled ("�", "â€”"), so only its length is checked
        None => rest.split_once("level:").filter(|(before, _)| before.chars().count() <= 8)?.1,
    };
    let level: String = rest.trim_start().chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    (!level.is_empty()).then_some(level)
}

/// Installs the caveman plugin into Claude Code unless it is already there
/// (installed but disabled by the user counts as there).
fn ensure_caveman(cli: &Path) {
    let result = (|| -> Result<bool, String> {
        let mut child = hidden(cli)
            .args(["plugin", "list", "--json"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let mut stdout = child.stdout.take().ok_or("no output")?;
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut stdout, &mut buf);
            buf
        });
        wait_for(&mut child, STATUS_TIMEOUT)?;
        let plugins: Vec<Value> = serde_json::from_slice(&reader.join().unwrap_or_default()).map_err(|e| e.to_string())?;
        if plugins.iter().any(|p| p.get("id").and_then(Value::as_str) == Some(CAVEMAN_ID)) {
            return Ok(false);
        }
        let log = File::create(paths::data_dir().join("caveman-install.log")).map_err(|e| e.to_string())?;
        let log_err = log.try_clone().map_err(|e| e.to_string())?;
        let mut child = hidden(cli)
            .args(["plugin", "install", "caveman", "--marketplace", CAVEMAN_MARKETPLACE, "--scope", "user"])
            .stdout(log)
            .stderr(log_err)
            .spawn()
            .map_err(|e| e.to_string())?;
        let status = wait_for(&mut child, PLUGIN_TIMEOUT)?;
        if !status.success() {
            return Err(format!("plugin install exited with {status}"));
        }
        Ok(true)
    })();
    match result {
        Ok(true) => paths::log("claude: caveman plugin installed"),
        Ok(false) => {}
        Err(e) => paths::log(format!("claude: caveman plugin install failed: {e}")),
    }
}

/// Starts `claude auth login` in its own console window and waits until the CLI
/// reports a sign-in. It needs a real console: without one its UI can't start
/// (no TTY for the code prompt) and the sign-in silently never finishes.
fn login(app: &AppHandle) {
    let Some(cli) = find_cli() else {
        install(app);
        return;
    };
    set(app, ClaudeStatus {
        state: AuthState::SigningIn,
        message: Some("Finish signing in to Claude in your browser…".into()),
        ..Default::default()
    });

    // After an auth failure `auth status` still says signed in. Drop the session
    // that can't be refreshed so it tells the truth again; if that fails, only
    // a finished `auth login` counts.
    let mut stale = app.state::<Claude>().auth_failed.load(Ordering::SeqCst);
    if stale {
        let logout = hidden(&cli).args(["auth", "logout"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
        stale = !logout.is_ok_and(|s| s.success());
    }

    let mut cmd = Command::new(&cli);
    cmd.args(["auth", "login", "--claudeai"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NEW_CONSOLE);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            set(app, ClaudeStatus {
                state: AuthState::Error,
                message: Some(format!("Couldn't start Claude sign-in: {e}")),
                ..Default::default()
            });
            return;
        }
    };

    let deadline = Instant::now() + LOGIN_TIMEOUT;
    let mut exited = false;
    let mut exited_ok = false;
    while Instant::now() < deadline {
        std::thread::sleep(LOGIN_POLL);
        if !exited {
            if let Ok(Some(st)) = child.try_wait() {
                exited = true;
                exited_ok = st.success();
            }
        }
        if let Ok(Some(plan)) = auth_status(&cli) {
            if stale && !exited_ok {
                if exited {
                    break;
                }
                continue;
            }
            let _ = child.kill();
            let _ = child.wait();
            app.state::<Claude>().auth_failed.store(false, Ordering::SeqCst);
            *app.state::<Claude>().last_check.lock().unwrap() = Some(Instant::now());
            set(app, ClaudeStatus { state: AuthState::SignedIn, plan, ..Default::default() });
            return;
        }
        // the CLI gave up (or the user closed the window): one last look, then stop
        if exited {
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    set(app, ClaudeStatus {
        state: AuthState::SignedOut,
        message: Some("Sign-in didn't finish.".into()),
        ..Default::default()
    });
}

fn logout(app: &AppHandle) {
    let Some(cli) = find_cli() else { return check(app) };
    let ok = hidden(&cli).args(["auth", "logout"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    if !ok {
        return set(app, ClaudeStatus {
            state: AuthState::Error,
            message: Some("Couldn't sign out of Claude.".into()),
            ..Default::default()
        });
    }
    app.state::<Claude>().auth_failed.store(false, Ordering::SeqCst);
    check(app);
}

/// A coding request failed to authenticate: remind the user to sign in.
pub fn report_auth_failure(app: &AppHandle) {
    app.state::<Claude>().auth_failed.store(true, Ordering::SeqCst);
    set(app, ClaudeStatus {
        state: AuthState::SignedOut,
        message: Some(EXPIRED.into()),
        ..Default::default()
    });
}

/// A coding request went through: an earlier auth failure no longer applies.
pub fn report_auth_ok(app: &AppHandle) {
    if app.state::<Claude>().auth_failed.swap(false, Ordering::SeqCst) {
        let app = app.clone();
        std::thread::spawn(move || exclusive(&app, || check(&app)));
    }
}

/// `auth status` only says a sign-in is saved, not that it still works: a
/// session that expired and can't be refreshed looks exactly the same. A
/// one-word Haiku request without tools settles it. With a dead session it
/// fails before reaching the API; otherwise its usage report fills in the
/// 5-hour limit right away.
fn probe(app: &AppHandle, cli: &Path) {
    let child = hidden(cli)
        .args(["-p", "--output-format", "stream-json", "--verbose", "--model", "haiku", "--max-turns", "1"])
        .args(["--no-session-persistence", "--strict-mcp-config", "--tools", "", "--system-prompt", "Reply with OK."])
        .current_dir(paths::data_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return paths::log(format!("claude probe failed to start: {e}")),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"ping");
    }
    let Some(stdout) = child.stdout.take() else { return };
    let reader = std::thread::spawn(move || {
        BufReader::new(stdout).lines().map_while(Result::ok).collect::<Vec<_>>()
    });
    let _ = wait_for(&mut child, PROBE_TIMEOUT);
    let lines = reader.join().unwrap_or_default();

    let mut auth_failed = false;
    let mut ok = false;
    for line in &lines {
        let Ok(ev) = serde_json::from_str::<Value>(line) else { continue };
        match ev.get("type").and_then(Value::as_str) {
            Some("assistant") => auth_failed |= ev.get("error").and_then(Value::as_str) == Some("authentication_failed"),
            Some("rate_limit_event") => {
                if let Some(info) = ev.get("rate_limit_info") {
                    crate::agent::record_limit(app, info);
                }
            }
            Some("result") => ok = !ev.get("is_error").and_then(Value::as_bool).unwrap_or(true),
            _ => {}
        }
    }
    paths::log(format!("claude probe: {}", if auth_failed { "sign-in expired" } else if ok { "ok" } else { "no answer" }));
    if auth_failed {
        report_auth_failure(app);
    }
}

/// How much of the plan is used, from the CLI's own `/usage` command: it runs
/// locally (no model request, no tokens) and reads the account's usage.
/// (current 5-hour session, current week), as 0..1.
pub fn usage(cli: &Path) -> Option<(f64, Option<f64>)> {
    let mut child = hidden(cli)
        .args(["-p", "--output-format", "text", "--no-session-persistence", "--strict-mcp-config"])
        .current_dir(paths::data_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"/usage");
    }
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut text);
        text
    });
    let _ = wait_for(&mut child, PROBE_TIMEOUT);
    let text = reader.join().unwrap_or_default();
    let parsed = parse_usage(&text);
    if parsed.is_none() {
        paths::log(format!("claude usage: unreadable: {}", text.lines().next().unwrap_or("").trim()));
    }
    parsed
}

/// "Current session: 37% used · resets …" / "Current week (all models): 42% used · …"
fn parse_usage(text: &str) -> Option<(f64, Option<f64>)> {
    let percent = |prefix: &str| {
        let line = text.lines().map(str::trim).find(|l| l.starts_with(prefix))?;
        let n = line[prefix.len()..].trim_start().split('%').next()?.trim();
        n.parse::<f64>().ok().map(|p| p / 100.0)
    };
    Some((percent("Current session:")?, percent("Current week (all models):")))
}

// ---------------------------------------------------------------- lifecycle

/// Installs the CLI if needed, then re-checks the sign-in for the app's lifetime.
/// At launch the saved sign-in is also tried for real (see `probe`).
pub fn spawn_monitor(app: AppHandle) {
    std::thread::spawn(move || {
        let mut first = true;
        let mut caveman_checked = false;
        loop {
            exclusive(&app, || {
                if find_cli().is_none() {
                    install(&app);
                } else {
                    check(&app);
                }
                let signed_in = app.state::<Claude>().status.lock().unwrap().state == AuthState::SignedIn;
                if let Some(cli) = find_cli() {
                    if !caveman_checked {
                        ensure_caveman(&cli);
                        caveman_checked = true;
                    }
                    if first && signed_in {
                        probe(&app, &cli);
                    }
                }
            });
            first = false;
            std::thread::sleep(CHECK_INTERVAL);
        }
    });
}

#[tauri::command]
pub fn claude_status(state: tauri::State<Claude>) -> ClaudeStatus {
    state.status.lock().unwrap().clone()
}

/// Re-checks the sign-in unless that happened in the last MIN_RECHECK.
#[tauri::command]
pub fn claude_refresh(app: AppHandle) {
    let recent = app
        .state::<Claude>()
        .last_check
        .lock()
        .unwrap()
        .is_some_and(|t| t.elapsed() < MIN_RECHECK);
    if !recent {
        std::thread::spawn(move || exclusive(&app, || check(&app)));
    }
}

#[tauri::command]
pub fn claude_login(app: AppHandle) {
    std::thread::spawn(move || exclusive(&app, || login(&app)));
}

#[tauri::command]
pub fn claude_logout(app: AppHandle) {
    std::thread::spawn(move || exclusive(&app, || logout(&app)));
}

/// Checks the sign-in now (Settings' "Check again").
#[tauri::command]
pub fn claude_recheck(app: AppHandle) {
    std::thread::spawn(move || exclusive(&app, || check(&app)));
}

#[tauri::command]
pub fn claude_install(app: AppHandle) {
    std::thread::spawn(move || exclusive(&app, || install(&app)));
}

#[cfg(test)]
mod tests {
    use super::{caveman_level, parse_usage};

    #[test]
    fn reads_usage() {
        let text = "You are currently using your subscription\n\nCurrent session: 37% used \u{FFFD} resets Oct 13, 5:30pm\nCurrent week (all models): 42% used · resets Oct 19\n";
        assert_eq!(parse_usage(text), Some((0.37, Some(0.42))));
        assert_eq!(parse_usage("Current session: 5% used"), Some((0.05, None)));
        assert_eq!(parse_usage("Not logged in"), None);
    }

    #[test]
    fn reads_caveman_level() {
        assert_eq!(caveman_level("CAVEMAN MODE ACTIVE — level: ultra\n\nRespond terse").as_deref(), Some("ultra"));
        assert_eq!(caveman_level("CAVEMAN MODE ACTIVE \u{FFFD} level: ultra").as_deref(), Some("ultra"));
        assert_eq!(caveman_level("CAVEMAN MODE ACTIVE â€” level: lite").as_deref(), Some("lite"));
        assert_eq!(caveman_level("{\"additionalContext\":\"CAVEMAN MODE ACTIVE (wenyan-full). Drop\"}").as_deref(), Some("wenyan-full"));
        assert_eq!(caveman_level("CAVEMAN MODE ACTIVE. Rules: … level: x"), None);
        assert_eq!(caveman_level("VERDICT-FIRST MODE ACTIVE."), None);
    }
}
