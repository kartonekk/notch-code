//! Coding agent: runs Claude Code headless in a project folder
//! (`claude -p --output-format stream-json`) and turns its event stream into
//! simple steps for the notch ("Editing App.tsx", "Running npm test", …).
//!
//! One process per message; `--resume <session>` continues the conversation.
//! Nothing ever waits for a permission prompt (`--permission-prompts none`):
//! what the chosen mode doesn't allow is denied and shown to the user, who can
//! allow it and continue.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::config::Config;
use crate::{claude, paths, review};

const MAX_RECENT: usize = 8;
const MAX_OUTPUT: usize = 1500;
/// "reckless" = no permission checks at all, and changes are kept without review.
const MODES: [&str; 4] = ["auto", "plan", "acceptEdits", "reckless"];

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug, Serialize, Default)]
pub struct AgentStatus {
    pub running: bool,
    /// Folder Claude works in.
    pub project: Option<String>,
    /// Recently used folders, newest first.
    pub recent: Vec<String>,
    /// A conversation is going on (the next message continues it).
    pub has_session: bool,
    /// The step in progress, for the collapsed bar.
    pub current: Option<String>,
    /// Unix ms when the running message was sent.
    pub started_at: Option<u64>,
    /// The 5-hour usage window, as last reported by Claude.
    pub limit: Option<Limit>,
    /// Model of the last run, as Claude reported it.
    pub model: Option<String>,
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
    Step { run: u64, id: String, icon: &'static str, label: String, detail: Option<String> },
    StepDone { run: u64, id: String, ok: bool, output: Option<String> },
    Done { run: u64, ok: bool, stopped: bool, message: Option<String>, duration_ms: u64, denied: Vec<Denied> },
}

#[derive(Default)]
pub struct Agent {
    status: Mutex<AgentStatus>,
    session: Mutex<Option<String>>,
    pid: Mutex<Option<u32>>,
    stopping: AtomicBool,
    runs: AtomicU64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn update(app: &AppHandle, f: impl FnOnce(&mut AgentStatus)) {
    let snapshot = {
        let agent = app.state::<Agent>();
        let mut s = agent.status.lock().unwrap();
        f(&mut s);
        s.has_session = agent.session.lock().unwrap().is_some();
        s.clone()
    };
    let _ = app.emit("agent-status", snapshot);
}

fn emit(app: &AppHandle, event: AgentEvent) {
    let _ = app.emit("agent", event);
}

/// Restores the project folder and the recent list from config.json.
pub fn init(app: &AppHandle, cfg: &Config) {
    let project = cfg.project.clone().filter(|p| Path::new(p).is_dir());
    review::set_project(app, project.clone());
    update(app, |s| {
        s.project = project;
        s.recent = cfg.recent_projects.clone();
    });
}

pub fn is_running(app: &AppHandle) -> bool {
    app.state::<Agent>().status.lock().unwrap().running
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

fn run(app: AppHandle, prompt: String, mode: String, allow: Vec<String>, model: Option<String>) {
    let agent = app.state::<Agent>();
    let run = agent.runs.fetch_add(1, Ordering::SeqCst) + 1;
    let started = Instant::now();
    agent.stopping.store(false, Ordering::SeqCst);
    emit(&app, AgentEvent::Start { run, prompt: prompt.clone() });
    update(&app, |s| {
        s.running = true;
        s.current = Some("Thinking…".into());
        s.started_at = Some(now_ms());
    });

    let result = execute(&app, run, &prompt, &mode, &allow, model.as_deref());
    if mode == "reckless" {
        // reckless: whatever Claude changed is kept without asking
        let _ = review::accept_all(&app);
    } else {
        review::refresh(&app);
    }

    *agent.pid.lock().unwrap() = None;
    let stopped = agent.stopping.swap(false, Ordering::SeqCst);
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
    emit(&app, AgentEvent::Done {
        run,
        ok,
        stopped,
        message,
        duration_ms: started.elapsed().as_millis() as u64,
        denied,
    });
    update(&app, |s| {
        s.running = false;
        s.current = None;
        s.started_at = None;
    });
}

fn execute(app: &AppHandle, run: u64, prompt: &str, mode: &str, allow: &[String], model: Option<&str>) -> Result<Outcome, String> {
    let agent = app.state::<Agent>();
    let cli = claude::find_cli().ok_or("Claude Code isn't installed yet.")?;
    let project = agent.status.lock().unwrap().project.clone().ok_or("Choose a project folder first.")?;
    let session = agent.session.lock().unwrap().clone();

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
    cmd.current_dir(&project).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start Claude: {e}"))?;
    *agent.pid.lock().unwrap() = Some(child.id());
    paths::log(format!("agent run {run} in {project} ({mode})"));

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
    // steps that change files: refresh the review list when they finish
    let mut edits: HashSet<String> = HashSet::new();
    let stdout = child.stdout.take().ok_or("no output from Claude")?;
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(ev) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(id) = ev.get("session_id").and_then(Value::as_str) {
            let mut s = agent.session.lock().unwrap();
            if s.as_deref() != Some(id) {
                *s = Some(id.to_string());
            }
        }
        // sub-agent chatter stays inside its "Delegating" step
        if ev.get("parent_tool_use_id").is_some_and(|p| !p.is_null()) {
            continue;
        }
        match ev.get("type").and_then(Value::as_str) {
            Some("system") if ev.get("subtype").and_then(Value::as_str) == Some("init") => {
                if let Some(m) = ev.get("model").and_then(Value::as_str) {
                    let m = m.to_string();
                    update(app, |s| s.model = Some(m));
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
                                emit(app, AgentEvent::Text { run, text: text.to_string() });
                            }
                        }
                        Some("tool_use") => {
                            let id = part.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                            let name = part.get("name").and_then(Value::as_str).unwrap_or("tool");
                            let input = part.get("input").cloned().unwrap_or(Value::Null);
                            let (icon, label, detail) = describe(name, &input);
                            tools.insert(id.clone(), label.clone());
                            if icon == "edit" || icon == "create" {
                                edits.insert(id.clone());
                            }
                            update(app, |s| s.current = Some(label.clone()));
                            emit(app, AgentEvent::Step { run, id, icon, label, detail });
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
                    update(app, |s| s.current = Some("Thinking…".into()));
                    let edited = edits.remove(&id);
                    emit(app, AgentEvent::StepDone { run, id, ok, output });
                    if edited && mode != "reckless" {
                        review::refresh(app);
                    }
                }
            }
            Some("rate_limit_event") => {
                if let Some(limit) = ev.get("rate_limit_info").and_then(parse_limit) {
                    update(app, |s| {
                        // keep the last known usage of the same window when this event doesn't carry one
                        let same_window = |l: &&Limit| l.resets_at == limit.resets_at;
                        let used = limit.used.or(s.limit.as_ref().filter(same_window).and_then(|l| l.used));
                        s.limit = Some(Limit { used, ..limit });
                    });
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
    if out.result.is_none() && !agent.stopping.load(Ordering::SeqCst) {
        let tail = stderr_tail.lock().unwrap().clone();
        let last = tail.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        paths::log(format!("agent run {run} ended without result ({status}): {last}"));
        if !last.is_empty() {
            return Err(clip(&last, 300));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- project folder

fn set_project_inner(app: &AppHandle, path: String) -> Result<(), String> {
    if !Path::new(&path).is_dir() {
        return Err("That folder doesn't exist anymore.".into());
    }
    let agent = app.state::<Agent>();
    if agent.status.lock().unwrap().running {
        return Err("Wait until Claude has finished.".into());
    }
    *agent.session.lock().unwrap() = None;
    let mut cfg = Config::load();
    cfg.recent_projects.retain(|p| !p.eq_ignore_ascii_case(&path));
    cfg.recent_projects.insert(0, path.clone());
    cfg.recent_projects.truncate(MAX_RECENT);
    cfg.project = Some(path.clone());
    cfg.save();
    review::set_project(app, Some(path.clone()));
    update(app, |s| {
        s.project = Some(path);
        s.recent = cfg.recent_projects.clone();
    });
    Ok(())
}

// ---------------------------------------------------------------- commands

#[tauri::command]
pub fn agent_status(app: AppHandle) -> AgentStatus {
    let agent = app.state::<Agent>();
    let mut s = agent.status.lock().unwrap().clone();
    s.has_session = agent.session.lock().unwrap().is_some();
    s
}

#[tauri::command]
pub fn agent_send(app: AppHandle, prompt: String, mode: String, allow: Vec<String>, model: Option<String>) -> Result<(), String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Ok(());
    }
    {
        let agent = app.state::<Agent>();
        let status = agent.status.lock().unwrap();
        if status.running {
            return Err("Claude is still working.".into());
        }
        if status.project.is_none() {
            return Err("Choose a project folder first.".into());
        }
    }
    let mode = if MODES.contains(&mode.as_str()) { mode } else { "auto".into() };
    // an alias ("opus", "sonnet", …) or a full model name
    let model = model.filter(|m| !m.is_empty() && m.chars().all(|c| c.is_ascii_alphanumeric() || "-._[]".contains(c)));
    std::thread::spawn(move || run(app, prompt, mode, allow, model));
    Ok(())
}

/// Stops the running message (and everything Claude started for it).
#[tauri::command]
pub fn agent_stop(app: AppHandle) {
    let agent = app.state::<Agent>();
    let Some(pid) = *agent.pid.lock().unwrap() else { return };
    agent.stopping.store(true, Ordering::SeqCst);
    let mut cmd = Command::new("taskkill");
    cmd.args(["/T", "/F", "/PID", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd.status();
}

/// Forgets the conversation; the next message starts a new one.
#[tauri::command]
pub fn agent_new(app: AppHandle) {
    *app.state::<Agent>().session.lock().unwrap() = None;
    update(&app, |_| {});
}

#[tauri::command]
pub fn agent_set_project(app: AppHandle, path: String) -> Result<(), String> {
    set_project_inner(&app, path)
}

/// Opens the system folder picker; returns the chosen folder (already set as project).
#[tauri::command]
pub async fn agent_pick_project(app: AppHandle) -> Result<Option<String>, String> {
    let mut dialog = app.dialog().file().set_title("Choose the folder Claude works in");
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    if let Some(cur) = app.state::<Agent>().status.lock().unwrap().project.clone() {
        dialog = dialog.set_directory(cur);
    }
    let Some(picked) = dialog.blocking_pick_folder() else { return Ok(None) };
    let path = picked.into_path().map_err(|e| e.to_string())?.to_string_lossy().into_owned();
    set_project_inner(&app, path.clone())?;
    Ok(Some(path))
}

#[tauri::command]
pub fn agent_open_project(app: AppHandle) -> Result<(), String> {
    let project = app.state::<Agent>().status.lock().unwrap().project.clone().ok_or("No project folder")?;
    app.opener().open_path(project, None::<&str>).map_err(|e| e.to_string())
}
