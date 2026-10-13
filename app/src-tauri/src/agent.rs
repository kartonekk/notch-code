//! Coding agent: runs Claude Code headless in a project folder
//! (`claude -p --output-format stream-json`) and turns its event stream into
//! simple steps for the notch ("Editing App.tsx", "Running npm test", …).
//!
//! Several sessions can be open at once, each with its own folder and
//! conversation, and they can all work at the same time.
//! One process per message; `--resume <session>` continues the conversation.
//! Nothing ever waits for a permission prompt (`--permission-prompts none`):
//! what the chosen mode doesn't allow is denied and shown to the user, who can
//! allow it and continue.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::config::Config;
use crate::{claude, paths, review, terminal};

const MAX_RECENT: usize = 8;
const MAX_SESSIONS: usize = 9;
const MAX_OUTPUT: usize = 1500;
/// "reckless" = no permission checks at all, and changes are kept without review.
const MODES: [&str; 4] = ["auto", "plan", "acceptEdits", "reckless"];

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// One Claude conversation in one folder.
#[derive(Clone, Debug, Serialize, Default)]
pub struct SessionStatus {
    pub id: u64,
    /// Folder Claude works in.
    pub project: String,
    pub running: bool,
    /// A conversation is going on (the next message continues it).
    pub has_session: bool,
    /// The step in progress, for the collapsed bar.
    pub current: Option<String>,
    /// Unix ms when that step started (None while Claude is thinking).
    pub current_since: Option<u64>,
    /// Unix ms when the running message was sent.
    pub started_at: Option<u64>,
    /// Model of the last run, as Claude reported it.
    pub model: Option<String>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct AgentStatus {
    /// Open sessions, in the order they were opened.
    pub sessions: Vec<SessionStatus>,
    /// Recently used folders, newest first.
    pub recent: Vec<String>,
    /// The 5-hour usage window, as last reported by Claude.
    pub limit: Option<Limit>,
}


/// Claude subscription usage window (from `rate_limit_event`).
#[derive(Clone, Debug, Serialize)]
pub struct Limit {
    /// "allowed" | "allowed_warning" | "rejected"
    pub status: String,
    /// 0..1, when Claude reports it
    pub used: Option<f64>,
    /// Unix ms when the window resets.
    pub resets_at: Option<u64>,
}

/// Takes a `rate_limit_info` from any Claude run (also the sign-in probe).
pub fn record_limit(app: &AppHandle, info: &Value) {
    let Some(limit) = parse_limit(info) else { return };
    update(app, |i| {
        // keep the last known usage of the same window when this event doesn't carry one
        let same_window = |l: &&Limit| l.resets_at == limit.resets_at;
        let used = limit.used.or(i.limit.as_ref().filter(same_window).and_then(|l| l.used));
        i.limit = Some(Limit { used, ..limit });
    });
}

fn parse_limit(info: &Value) -> Option<Limit> {
    let kind = info.get("rateLimitType").and_then(Value::as_str).unwrap_or("five_hour");
    if kind != "five_hour" {
        return None;
    }
    let used = info.get("utilization").and_then(Value::as_f64).map(|u| if u > 1.0 { u / 100.0 } else { u });
    // seconds from the API headers; tolerate ms
    let resets_at = info
        .get("resetsAt")
        .and_then(Value::as_f64)
        .map(|r| if r < 1e12 { (r * 1000.0) as u64 } else { r as u64 });
    Some(Limit {
        status: info.get("status").and_then(Value::as_str).unwrap_or("allowed").to_string(),
        used,
        resets_at,
    })
}

/// Something the permission mode didn't allow; `rule` goes to --allowedTools.
#[derive(Clone, Debug, Serialize)]
pub struct Denied {
    pub label: String,
    pub rule: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvent {
    Start { run: u64, prompt: String },
    Text { run: u64, text: String },
    /// `at` = unix ms when it started
    Step { run: u64, id: String, icon: &'static str, label: String, detail: Option<String>, at: u64 },
    StepDone { run: u64, id: String, ok: bool, output: Option<String>, label: String, duration_ms: u64 },
    Done { run: u64, ok: bool, stopped: bool, message: Option<String>, duration_ms: u64, denied: Vec<Denied> },
}

struct Session {
    status: SessionStatus,
    /// Claude Code's session id, for --resume.
    claude: Option<String>,
    pid: Option<u32>,
    stopping: bool,
}

#[derive(Default)]
struct Inner {
    sessions: Vec<Session>,
    recent: Vec<String>,
    limit: Option<Limit>,
    next_id: u64,
}

#[derive(Default)]
pub struct Agent {
    inner: Mutex<Inner>,
    runs: AtomicU64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn snapshot(inner: &Inner) -> AgentStatus {
    AgentStatus {
        sessions: inner
            .sessions
            .iter()
            .map(|s| SessionStatus { has_session: s.claude.is_some(), ..s.status.clone() })
            .collect(),
        recent: inner.recent.clone(),
        limit: inner.limit.clone(),
    }
}

/// Changes the state and tells the UI.
fn update(app: &AppHandle, f: impl FnOnce(&mut Inner)) {
    let snap = {
        let agent = app.state::<Agent>();
        let mut inner = agent.inner.lock().unwrap();
        f(&mut inner);
        snapshot(&inner)
    };
    let _ = app.emit("agent-status", snap);
}

/// Reads or changes one session without telling the UI; None when it's gone.
fn with_session<T>(app: &AppHandle, sid: u64, f: impl FnOnce(&mut Session) -> T) -> Option<T> {
    let agent = app.state::<Agent>();
    let mut inner = agent.inner.lock().unwrap();
    inner.sessions.iter_mut().find(|s| s.status.id == sid).map(f)
}

/// Changes one session and tells the UI.
fn session_update(app: &AppHandle, sid: u64, f: impl FnOnce(&mut Session)) {
    update(app, |i| {
        if let Some(s) = i.sessions.iter_mut().find(|s| s.status.id == sid) {
            f(s);
        }
    });
}

#[derive(Clone, Serialize)]
struct Tagged {
    sid: u64,
    #[serde(flatten)]
    event: AgentEvent,
}

fn emit(app: &AppHandle, sid: u64, event: AgentEvent) {
    let _ = app.emit("agent", Tagged { sid, event });
}

fn add_session(inner: &mut Inner, project: String) -> u64 {
    inner.next_id += 1;
    let id = inner.next_id;
    inner.sessions.push(Session {
        status: SessionStatus { id, project, ..Default::default() },
        claude: None,
        pid: None,
        stopping: false,
    });
    id
}

/// Open sessions' folders and the recent list go to config.json.
fn persist(inner: &mut Inner, used: Option<&str>) {
    let mut cfg = Config::load();
    if let Some(path) = used {
        cfg.recent_projects.retain(|p| !p.eq_ignore_ascii_case(path));
        cfg.recent_projects.insert(0, path.to_string());
        cfg.recent_projects.truncate(MAX_RECENT);
    }
    cfg.sessions = inner.sessions.iter().map(|s| s.status.project.clone()).collect();
    cfg.project = cfg.sessions.first().cloned();
    cfg.save();
    inner.recent = cfg.recent_projects;
}

fn sync_review(app: &AppHandle) {
    let projects: Vec<String> = {
        let agent = app.state::<Agent>();
        let inner = agent.inner.lock().unwrap();
        inner.sessions.iter().map(|s| s.status.project.clone()).collect()
    };
    review::track(app, projects);
}

/// Restores the open sessions and the recent list from config.json.
pub fn init(app: &AppHandle, cfg: &Config) {
    let mut projects = cfg.sessions.clone();
    if projects.is_empty() {
        projects.extend(cfg.project.clone());
    }
    update(app, |i| {
        i.recent = cfg.recent_projects.clone();
        for p in projects.into_iter().filter(|p| Path::new(p).is_dir()).take(MAX_SESSIONS) {
            add_session(i, p);
        }
    });
    sync_review(app);
}

/// Claude is working in this folder (in any session).
pub fn is_running_in(app: &AppHandle, project: &str) -> bool {
    let agent = app.state::<Agent>();
    let inner = agent.inner.lock().unwrap();
    inner.sessions.iter().any(|s| s.status.running && s.status.project.eq_ignore_ascii_case(project))
}

// ---------------------------------------------------------------- describing steps

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max).collect::<String>().trim_end())
    } else {
        s.to_string()
    }
}

fn host(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    rest.split('/').next().unwrap_or(rest).to_string()
}

/// (icon, label, detail) for a tool call, in plain words.
fn describe(name: &str, input: &Value) -> (&'static str, String, Option<String>) {
    let s = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let some = |v: String| (!v.is_empty()).then_some(v);
    match name {
        "Read" => ("read", format!("Reading {}", file_name(&s("file_path"))), some(s("file_path"))),
        "Write" => ("create", format!("Creating {}", file_name(&s("file_path"))), some(s("file_path"))),
        "Edit" | "MultiEdit" => ("edit", format!("Editing {}", file_name(&s("file_path"))), some(s("file_path"))),
        "NotebookEdit" => ("edit", format!("Editing {}", file_name(&s("notebook_path"))), some(s("notebook_path"))),
        "Bash" | "PowerShell" => {
            let command = s("command");
            let description = s("description");
            let label = if description.is_empty() {
                format!("Running {}", clip(command.lines().next().unwrap_or(""), 60))
            } else {
                clip(&description, 70)
            };
            let label = if input.get("run_in_background").and_then(Value::as_bool).unwrap_or(false) {
                format!("{label} (in background)")
            } else {
                label
            };
            ("run", label, some(command))
        }
        "Grep" => ("search", format!("Searching for “{}”", clip(&s("pattern"), 40)), some(s("path"))),
        "Glob" => ("search", format!("Finding {}", clip(&s("pattern"), 50)), some(s("path"))),
        "WebFetch" => ("web", format!("Opening {}", host(&s("url"))), some(s("url"))),
        "WebSearch" => ("web", format!("Searching the web for “{}”", clip(&s("query"), 40)), None),
        "TodoWrite" | "TaskCreate" | "TaskUpdate" => ("plan", "Updating the to-do list".into(), None),
        "ExitPlanMode" => ("plan", "Proposing a plan".into(), some(s("plan"))),
        "Task" | "Agent" => ("agent", format!("Delegating: {}", clip(&s("description"), 50)), some(clip(&s("prompt"), 400))),
        other => ("other", other.trim_start_matches("mcp__").replace("__", " · "), None),
    }
}

/// The --allowedTools rule that lets a denied call through next time.
fn allow_rule(name: &str, input: &Value) -> String {
    match (name, input.get("command").and_then(Value::as_str)) {
        ("Bash" | "PowerShell", Some(cmd)) => format!("{name}({cmd})"),
        _ => name.to_string(),
    }
}

fn result_text(content: &Value) -> Option<String> {
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| clip(text, MAX_OUTPUT))
}

// ---------------------------------------------------------------- running

struct Outcome {
    result: Option<(bool, String)>,
    denied: Vec<Denied>,
    auth_failed: bool,
}

fn run(app: AppHandle, sid: u64, prompt: String, mode: String, allow: Vec<String>, model: Option<String>) {
    let run = app.state::<Agent>().runs.fetch_add(1, Ordering::SeqCst) + 1;
    let started = Instant::now();
    emit(&app, sid, AgentEvent::Start { run, prompt: prompt.clone() });

    let result = execute(&app, sid, run, &prompt, &mode, &allow, model.as_deref());
    let project = with_session(&app, sid, |s| s.status.project.clone());
    if let Some(project) = &project {
        if mode == "reckless" {
            // reckless: whatever Claude changed is kept without asking
            let _ = review::accept_all(&app, project);
        } else {
            review::refresh(&app, project);
        }
    }

    let stopped = with_session(&app, sid, |s| {
        s.pid = None;
        std::mem::take(&mut s.stopping)
    })
    .unwrap_or(true);
    let (ok, message, denied) = match result {
        _ if stopped => (false, Some("Stopped".to_string()), Vec::new()),
        Err(e) => (false, Some(e), Vec::new()),
        Ok(out) if out.auth_failed => {
            claude::report_auth_failure(&app);
            (false, Some("Claude's sign-in has expired. Sign in again, then send your message again.".into()), Vec::new())
        }
        Ok(out) => {
            claude::report_auth_ok(&app);
            match out.result {
                Some((false, _)) => (true, None, out.denied),
                Some((true, text)) => (false, Some(clip(&text, 300)), out.denied),
                None => (false, Some("Claude stopped without an answer.".into()), out.denied),
            }
        }
    };
    // idle first, so a message queued in the UI can go out as soon as it hears "done"
    session_update(&app, sid, |s| {
        s.status.running = false;
        s.status.current = None;
        s.status.current_since = None;
        s.status.started_at = None;
    });
    emit(&app, sid, AgentEvent::Done {
        run,
        ok,
        stopped,
        message,
        duration_ms: started.elapsed().as_millis() as u64,
        denied,
    });
}

fn execute(app: &AppHandle, sid: u64, run: u64, prompt: &str, mode: &str, allow: &[String], model: Option<&str>) -> Result<Outcome, String> {
    let cli = claude::find_cli().ok_or("Claude Code isn't installed yet.")?;
    let (project, session) = with_session(app, sid, |s| (s.status.project.clone(), s.claude.clone())).ok_or("That session was closed.")?;

    let mut cmd = Command::new(&cli);
    cmd.args(["-p", "--output-format", "stream-json", "--verbose", "--permission-prompts", "none"]);
    if mode == "reckless" {
        cmd.arg("--dangerously-skip-permissions");
    } else {
        cmd.args(["--permission-mode", mode]);
    }
    // snapshot files before Claude edits them, for review / reject
    match review::hook_settings(&project) {
        Ok(settings) => {
            cmd.arg("--settings").arg(settings).env(review::DIR_ENV, review::dir_for(&project));
        }
        Err(e) => paths::log(format!("review hook unavailable: {e}")),
    }
    if let Some(id) = &session {
        cmd.args(["--resume", id]);
    }
    if let Some(m) = model {
        cmd.args(["--model", m]);
    }
    if !allow.is_empty() {
        cmd.arg("--allowedTools").args(allow);
    }
    // notch-terminal: lets Claude open a window the user can see
    match terminal::shim_dir() {
        Ok(bin) => {
            let mut path = std::ffi::OsString::from(bin);
            if let Some(old) = std::env::var_os("PATH") {
                path.push(";");
                path.push(old);
            }
            cmd.env("PATH", path).arg("--append-system-prompt").arg(terminal::PROMPT);
            if let Ok(exe) = std::env::current_exe() {
                cmd.env(terminal::EXE_ENV, exe);
            }
        }
        Err(e) => paths::log(format!("notch-terminal unavailable: {e}")),
    }
    // caveman plugin (installed at startup): terse answers save tokens
    cmd.env("CAVEMAN_DEFAULT_MODE", claude::CAVEMAN_MODE);
    cmd.current_dir(&project).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start Claude: {e}"))?;
    let pid = child.id();
    with_session(app, sid, |s| s.pid = Some(pid));
    paths::log(format!("agent run {run} (session {sid}) in {project} ({mode})"));

    // the prompt goes through stdin, so no command-line quoting is involved
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes());
    }
    let stderr_tail = Arc::new(Mutex::new(String::new()));
    if let Some(mut stderr) = child.stderr.take() {
        let tail = stderr_tail.clone();
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf);
            *tail.lock().unwrap() = buf;
        });
    }

    let mut out = Outcome { result: None, denied: Vec::new(), auth_failed: false };
    let mut tools: HashMap<String, String> = HashMap::new();
    let mut step_started: HashMap<String, Instant> = HashMap::new();
    // steps that change files: refresh the review list when they finish
    let mut edits: HashSet<String> = HashSet::new();
    let stdout = child.stdout.take().ok_or("no output from Claude")?;
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(ev) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(id) = ev.get("session_id").and_then(Value::as_str) {
            with_session(app, sid, |s| {
                if s.claude.as_deref() != Some(id) {
                    s.claude = Some(id.to_string());
                }
            });
        }
        // sub-agent chatter stays inside its "Delegating" step
        if ev.get("parent_tool_use_id").is_some_and(|p| !p.is_null()) {
            continue;
        }
        match ev.get("type").and_then(Value::as_str) {
            Some("system") if ev.get("subtype").and_then(Value::as_str) == Some("init") => {
                if let Some(m) = ev.get("model").and_then(Value::as_str) {
                    let m = m.to_string();
                    session_update(app, sid, |s| s.status.model = Some(m));
                }
            }
            Some("assistant") => {
                if ev.get("error").and_then(Value::as_str) == Some("authentication_failed") {
                    out.auth_failed = true;
                }
                let parts = ev.pointer("/message/content").and_then(Value::as_array).cloned().unwrap_or_default();
                for part in parts {
                    match part.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            let text = part.get("text").and_then(Value::as_str).unwrap_or("").trim();
                            if !text.is_empty() && !out.auth_failed {
                                emit(app, sid, AgentEvent::Text { run, text: text.to_string() });
                            }
                        }
                        Some("tool_use") => {
                            let id = part.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                            let name = part.get("name").and_then(Value::as_str).unwrap_or("tool");
                            let input = part.get("input").cloned().unwrap_or(Value::Null);
                            let (icon, label, detail) = describe(name, &input);
                            tools.insert(id.clone(), label.clone());
                            step_started.insert(id.clone(), Instant::now());
                            if icon == "edit" || icon == "create" {
                                edits.insert(id.clone());
                            }
                            let at = now_ms();
                            session_update(app, sid, |s| {
                                s.status.current = Some(label.clone());
                                s.status.current_since = Some(at);
                            });
                            emit(app, sid, AgentEvent::Step { run, id, icon, label, detail, at });
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => {
                let parts = ev.pointer("/message/content").and_then(Value::as_array).cloned().unwrap_or_default();
                for part in parts.iter().filter(|p| p.get("type").and_then(Value::as_str) == Some("tool_result")) {
                    let id = part.get("tool_use_id").and_then(Value::as_str).unwrap_or("").to_string();
                    let ok = !part.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                    let output = part.get("content").and_then(result_text);
                    session_update(app, sid, |s| {
                        s.status.current = Some("Thinking…".into());
                        s.status.current_since = None;
                    });
                    let edited = edits.remove(&id);
                    let label = tools.get(&id).cloned().unwrap_or_default();
                    let duration_ms = step_started.remove(&id).map(|t| t.elapsed().as_millis() as u64).unwrap_or(0);
                    emit(app, sid, AgentEvent::StepDone { run, id, ok, output, label, duration_ms });
                    if edited && mode != "reckless" {
                        review::refresh(app, &project);
                    }
                }
            }
            Some("rate_limit_event") => {
                if let Some(info) = ev.get("rate_limit_info") {
                    record_limit(app, info);
                }
            }
            Some("result") => {
                let is_error = ev.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                let text = ev.get("result").and_then(Value::as_str).unwrap_or("").to_string();
                out.result = Some((is_error, text));
                for d in ev.get("permission_denials").and_then(Value::as_array).into_iter().flatten() {
                    let name = d.get("tool_name").and_then(Value::as_str).unwrap_or("tool");
                    let input = d.get("tool_input").cloned().unwrap_or(Value::Null);
                    let label = d
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .and_then(|id| tools.get(id).cloned())
                        .unwrap_or_else(|| describe(name, &input).1);
                    let rule = allow_rule(name, &input);
                    if !out.denied.iter().any(|x| x.rule == rule) {
                        out.denied.push(Denied { label, rule });
                    }
                }
            }
            _ => {}
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if out.result.is_none() && !with_session(app, sid, |s| s.stopping).unwrap_or(true) {
        let tail = stderr_tail.lock().unwrap().clone();
        let last = tail.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        paths::log(format!("agent run {run} ended without result ({status}): {last}"));
        if !last.is_empty() {
            return Err(clip(&last, 300));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- commands

fn check_dir(path: &str) -> Result<(), String> {
    if Path::new(path).is_dir() {
        Ok(())
    } else {
        Err("That folder doesn't exist anymore.".into())
    }
}

#[tauri::command]
pub fn agent_status(app: AppHandle) -> AgentStatus {
    snapshot(&app.state::<Agent>().inner.lock().unwrap())
}

/// Opens a new session in `path`; returns its id.
#[tauri::command]
pub fn agent_open(app: AppHandle, path: String) -> Result<u64, String> {
    check_dir(&path)?;
    let mut result = Err("You can have up to 9 sessions open.".to_string());
    update(&app, |i| {
        if i.sessions.len() < MAX_SESSIONS {
            result = Ok(add_session(i, path.clone()));
            persist(i, Some(&path));
        }
    });
    sync_review(&app);
    result
}

/// Closes a session (stopping Claude first if it's working).
#[tauri::command]
pub fn agent_close(app: AppHandle, sid: u64) {
    agent_stop(app.clone(), sid);
    update(&app, |i| {
        i.sessions.retain(|s| s.status.id != sid);
        persist(i, None);
    });
    sync_review(&app);
}

/// Moves an idle session to another folder (a new conversation starts there).
#[tauri::command]
pub fn agent_set_project(app: AppHandle, sid: u64, path: String) -> Result<(), String> {
    check_dir(&path)?;
    let mut result = Ok(());
    update(&app, |i| {
        match i.sessions.iter_mut().find(|s| s.status.id == sid) {
            Some(s) if s.status.running => result = Err("Wait until Claude has finished.".to_string()),
            Some(s) => {
                s.status.project = path.clone();
                s.status.model = None;
                s.claude = None;
            }
            None => result = Err("That session was closed.".to_string()),
        }
        if result.is_ok() {
            persist(i, Some(&path));
        }
    });
    sync_review(&app);
    result
}

/// The system folder picker; returns the chosen folder.
#[tauri::command]
pub async fn agent_pick_folder(app: AppHandle, current: Option<String>) -> Result<Option<String>, String> {
    let mut dialog = app.dialog().file().set_title("Choose the folder Claude works in");
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    if let Some(cur) = current.filter(|c| Path::new(c).is_dir()) {
        dialog = dialog.set_directory(cur);
    }
    let Some(picked) = dialog.blocking_pick_folder() else { return Ok(None) };
    Ok(Some(picked.into_path().map_err(|e| e.to_string())?.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn agent_send(app: AppHandle, sid: u64, prompt: String, mode: String, allow: Vec<String>, model: Option<String>) -> Result<(), String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Ok(());
    }
    let mut result = Ok(());
    update(&app, |i| match i.sessions.iter_mut().find(|s| s.status.id == sid) {
        Some(s) if s.status.running => result = Err("Claude is still working.".to_string()),
        Some(s) => {
            s.status.running = true;
            s.status.current = Some("Thinking…".into());
            s.status.started_at = Some(now_ms());
            s.stopping = false;
        }
        None => result = Err("That session was closed.".to_string()),
    });
    result?;
    let mode = if MODES.contains(&mode.as_str()) { mode } else { "auto".into() };
    // an alias ("opus", "sonnet", …) or a full model name
    let model = model.filter(|m| !m.is_empty() && m.chars().all(|c| c.is_ascii_alphanumeric() || "-._[]".contains(c)));
    std::thread::spawn(move || run(app, sid, prompt, mode, allow, model));
    Ok(())
}

fn kill_tree(pid: u32) {
    let mut cmd = Command::new("taskkill");
    cmd.args(["/T", "/F", "/PID", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd.status();
}

/// Stops the running message of a session (and everything Claude started for it).
#[tauri::command]
pub fn agent_stop(app: AppHandle, sid: u64) {
    let pid = with_session(&app, sid, |s| {
        if s.pid.is_some() {
            s.stopping = true;
        }
        s.pid
    })
    .flatten();
    if let Some(pid) = pid {
        kill_tree(pid);
    }
}

/// Stops every session (on quit).
pub fn stop_all(app: &AppHandle) {
    let ids: Vec<u64> = app.state::<Agent>().inner.lock().unwrap().sessions.iter().map(|s| s.status.id).collect();
    for sid in ids {
        agent_stop(app.clone(), sid);
    }
}

/// Forgets the conversation; the next message starts a new one.
#[tauri::command]
pub fn agent_new(app: AppHandle, sid: u64) {
    session_update(&app, sid, |s| s.claude = None);
}

// ---------------------------------------------------------------- images

const IMAGE_EXTS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
/// Pasted images are kept this long (Claude may look at them again later in the conversation).
const KEEP_IMAGES: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 3600);

/// Saves a pasted image (raw bytes; header `x-ext` = file extension) so Claude
/// can read it; returns its path.
#[tauri::command]
pub fn agent_save_image(request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err("No image data.".into());
    };
    let ext = request
        .headers()
        .get("x-ext")
        .and_then(|v| v.to_str().ok())
        .map(str::to_ascii_lowercase)
        .filter(|e| IMAGE_EXTS.contains(&e.as_str()))
        .unwrap_or_else(|| "png".into());
    let dir = paths::data_dir().join("images");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // forget old ones
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let old = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > KEEP_IMAGES);
            if old {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    static N: AtomicU64 = AtomicU64::new(0);
    let name = format!("{}-{}.{ext}", chrono::Local::now().format("%Y%m%d-%H%M%S"), N.fetch_add(1, Ordering::Relaxed));
    let path = dir.join(name);
    std::fs::write(&path, data).map_err(|e| format!("Couldn't save the image: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

/// The system file picker for images; returns the chosen files.
#[tauri::command]
pub async fn agent_pick_images(app: AppHandle) -> Result<Vec<String>, String> {
    let mut dialog = app.dialog().file().set_title("Images for Claude").add_filter("Images", &IMAGE_EXTS);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    let Some(picked) = dialog.blocking_pick_files() else { return Ok(Vec::new()) };
    picked
        .into_iter()
        .map(|p| p.into_path().map(|p| p.to_string_lossy().into_owned()).map_err(|e| e.to_string()))
        .collect()
}

#[tauri::command]
pub fn agent_open_folder(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}
