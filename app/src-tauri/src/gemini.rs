//! Gemini CLI (Google's coding agent), the second agent a session can use.
//! Installed with npm when missing, signed in with a Google account (the
//! Gemini subscription) in the browser, and run headless by agent.rs
//! (`--output-format stream-json`), one process per message.
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
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
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

// ---------------------------------------------------------------- browser sign-in
//
// The same Google sign-in Gemini CLI does itself (its `authWithWeb`), run here
// so no console window is involved: the browser opens Google's page, Google
// sends the answer to a one-off server on 127.0.0.1, the code is traded for
// tokens, and they're saved where Gemini CLI reads them (oauth_creds.json,
// google_accounts.json). The OAuth client is Gemini CLI's own, read from the
// installed package, so the tokens are exactly the ones it would have made.

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const SCOPES: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile";
const SUCCESS_PAGE: &str = "https://developers.google.com/gemini-code-assist/auth_success_gemini";
const FAILURE_PAGE: &str = "https://developers.google.com/gemini-code-assist/auth_failure_gemini";

/// Gemini CLI's OAuth client (id, secret) from its bundle: `var OAUTH_CLIENT_ID = "…";`.
fn oauth_client() -> Option<(String, String)> {
    let dir = package_dir()?;
    let find = |text: &str, name: &str| {
        let start = text.find(&format!("{name} = \""))? + name.len() + 4;
        let end = text[start..].find('"')?;
        Some(text[start..start + end].to_string())
    };
    let mut stack = vec![dir.join("bundle"), dir.join("dist")];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "js") {
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                if let (Some(id), Some(secret)) = (find(&text, "OAUTH_CLIENT_ID"), find(&text, "OAUTH_CLIENT_SECRET")) {
                    return Some((id, secret));
                }
            }
        }
    }
    None
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn url_decode(s: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h << 4 | l);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `a=1&b=2` → value of `key`.
fn query_get(query: &str, key: &str) -> Option<String> {
    query.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == key).map(|(_, v)| url_decode(v))
}

/// An unguessable `state` for the sign-in (the std hasher is seeded from the OS).
fn random_state() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for n in 0..4u64 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(n);
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

/// Waits for Google's redirect to the local server; returns the query string.
fn wait_for_redirect(listener: &std::net::TcpListener, deadline: Instant) -> Result<(std::net::TcpStream, String), String> {
    use std::io::{BufRead, BufReader};
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                let mut line = String::new();
                let _ = BufReader::new(&stream).read_line(&mut line);
                // "GET /oauth2callback?code=…&state=… HTTP/1.1"
                let target = line.split_whitespace().nth(1).unwrap_or("");
                if let Some(query) = target.strip_prefix("/oauth2callback?") {
                    return Ok((stream, query.to_string()));
                }
                // a favicon or a stray request: answer and keep waiting
                let _ = respond(stream, None);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Err("Sign-in timed out. Try again.".into());
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Sends the browser on to Google's "you're signed in" (or "it failed") page.
fn respond(mut stream: std::net::TcpStream, ok: Option<bool>) -> std::io::Result<()> {
    use std::io::Write;
    let reply = match ok {
        Some(ok) => format!(
            "HTTP/1.1 301 Moved Permanently\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            if ok { SUCCESS_PAGE } else { FAILURE_PAGE }
        ),
        None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
    };
    stream.write_all(reply.as_bytes())
}

/// Saves the tokens the way Gemini CLI's `cacheCredentials` does.
fn save_tokens(tokens: &Value) -> Result<(), String> {
    let dir = home().ok_or("No home folder.")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut creds = serde_json::Map::new();
    for key in ["access_token", "refresh_token", "scope", "token_type", "id_token"] {
        if let Some(v) = tokens.get(key).filter(|v| !v.is_null()) {
            creds.insert(key.into(), v.clone());
        }
    }
    if let Some(secs) = tokens.get("expires_in").and_then(Value::as_u64) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        creds.insert("expiry_date".into(), Value::from(now + secs * 1000));
    }
    let text = serde_json::to_string_pretty(&Value::Object(creds)).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("oauth_creds.json"), text).map_err(|e| format!("Couldn't save the sign-in: {e}"))
}

/// The account Gemini CLI shows ("active"), with the previous one kept in "old" like it does.
fn save_account(email: &str) {
    let Some(path) = home().map(|h| h.join("google_accounts.json")) else { return };
    let mut accounts: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({ "active": null, "old": [] }));
    let previous = accounts.get("active").and_then(Value::as_str).map(str::to_string);
    let mut old: Vec<Value> = accounts.get("old").and_then(Value::as_array).cloned().unwrap_or_default();
    old.retain(|v| v.as_str() != Some(email));
    if let Some(p) = previous.filter(|p| p != email) {
        if !old.iter().any(|v| v.as_str() == Some(&p)) {
            old.push(Value::String(p));
        }
    }
    accounts["active"] = Value::String(email.to_string());
    accounts["old"] = Value::Array(old);
    if let Ok(text) = serde_json::to_string_pretty(&accounts) {
        let _ = std::fs::write(path, text);
    }
}

fn browser_sign_in(app: &AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let (client_id, client_secret) = oauth_client().ok_or("Couldn't read Gemini CLI's sign-in settings — reinstall it from Settings.")?;
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("Couldn't start the sign-in: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}/oauth2callback");
    let state = random_state();
    let url = format!(
        "{AUTH_URL}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&state={state}",
        url_encode(&client_id),
        url_encode(&redirect),
        url_encode(SCOPES),
    );
    app.opener().open_url(&url, None::<&str>).map_err(|e| format!("Couldn't open the browser: {e}"))?;

    let (stream, query) = wait_for_redirect(&listener, Instant::now() + LOGIN_TIMEOUT)?;
    if let Some(err) = query_get(&query, "error") {
        let _ = respond(stream, Some(false));
        let why = query_get(&query, "error_description").unwrap_or_default();
        return Err(if err == "access_denied" { "Sign-in was cancelled.".into() } else { format!("Google said: {err} {why}").trim().to_string() });
    }
    if query_get(&query, "state").as_deref() != Some(state.as_str()) {
        let _ = respond(stream, Some(false));
        return Err("The sign-in answer didn't match. Try again.".into());
    }
    let Some(code) = query_get(&query, "code") else {
        let _ = respond(stream, Some(false));
        return Err("Google sent no sign-in code. Try again.".into());
    };

    let body = format!(
        "code={}&client_id={}&client_secret={}&redirect_uri={}&grant_type=authorization_code",
        url_encode(&code),
        url_encode(&client_id),
        url_encode(&client_secret),
        url_encode(&redirect),
    );
    let http = reqwest::blocking::Client::new();
    let exchanged = http
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .and_then(|r| r.text())
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()));
    let tokens = match exchanged {
        Ok(t) if t.get("access_token").is_some() => t,
        Ok(t) => {
            let _ = respond(stream, Some(false));
            let why = t.get("error_description").or_else(|| t.get("error")).and_then(Value::as_str).unwrap_or("no token");
            return Err(format!("Google refused the sign-in: {why}"));
        }
        Err(e) => {
            let _ = respond(stream, Some(false));
            return Err(format!("Couldn't finish the sign-in: {e}"));
        }
    };
    if let Err(e) = save_tokens(&tokens) {
        let _ = respond(stream, Some(false));
        return Err(e);
    }
    let _ = respond(stream, Some(true));

    let token = tokens.get("access_token").and_then(Value::as_str).unwrap_or("");
    let email = http
        .get(USERINFO_URL)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .and_then(|r| r.text())
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("email").and_then(Value::as_str).map(str::to_string));
    if let Some(email) = email {
        save_account(&email);
    }
    Ok(())
}

/// Signs in to Gemini with Google in the browser (installs Gemini CLI first if needed).
fn login(app: &AppHandle) {
    if node().is_none() {
        return check(app);
    }
    if script().is_none() {
        install(app);
        if script().is_none() {
            return;
        }
    }
    set(app, GeminiStatus {
        state: AuthState::SigningIn,
        message: Some("Finish signing in with Google in your browser…".into()),
        ..Default::default()
    });
    prefer_google_login();
    match browser_sign_in(app) {
        Ok(()) => {
            paths::log("gemini: signed in");
            app.state::<Gemini>().auth_failed.store(false, Ordering::SeqCst);
            check(app);
        }
        Err(e) => {
            paths::log(format!("gemini sign-in failed: {e}"));
            set(app, GeminiStatus { state: AuthState::SignedOut, message: Some(e), ..Default::default() });
        }
    }
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
