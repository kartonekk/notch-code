//! Coding agent: runs Claude Code headless in a project folder
//! (`claude -p --output-format stream-json`) and turns its event stream into
//! simple steps for the notch ("Editing App.tsx", "Running npm test", …).
//!
//! Several sessions can be open at once, each with its own folder and
//! conversation, and they can all work at the same time.
//! One process per session, kept between messages (`--input-format
//! stream-json`), so Claude Code starts and connects its MCP servers once, not
//! for every message. What the user writes while Claude works reaches it right
//! away; Claude echoes each message when it takes it up
//! (`--replay-user-messages`), so a run ends at the answer that comes after
//! the last one. A message that needs other start-up flags (model, allowed
//! tools) restarts it with `--resume <session>`.
//! Nothing ever waits for a permission prompt (`--permission-prompts none`):
//! what the chosen mode doesn't allow is denied and shown to the user, who can
//! allow it and continue.
//!
//! A session can use Gemini through Antigravity CLI instead (see gemini.rs). It
//! runs one process per message (`--conversation` continues the conversation)
//! and its stream-json events become the same steps; Claude and Gemini
//! sessions work side by side.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::config::Config;
use crate::{claude, commands, gemini, paths, review, terminal};

const PROVIDERS: [&str; 2] = ["claude", "gemini"];
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
    /// The agent: "claude" | "gemini".
    pub provider: String,
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
    /// Caveman level Claude answers with ("ultra", …), as its hooks last
    /// reported it; expected from the plugin's settings until the first run.
    pub caveman: Option<String>,
    /// Tokens Claude has written for the running (or last) message, counted
    /// live like Claude Code's spinner does; 0 = none reported.
    pub tokens: u64,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct AgentStatus {
    /// Open sessions, in the order they were opened.
    pub sessions: Vec<SessionStatus>,
    /// Recently used folders, newest first.
    pub recent: Vec<String>,
    /// The 5-hour usage window, as last reported by Claude.
    pub limit: Option<Limit>,
    /// How much of the plan is used, from Claude Code's `/usage`.
    pub usage: Option<Usage>,
    /// The agent new sessions use.
    pub default_provider: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Usage {
    /// Current 5-hour session, 0..1.
    pub session: f64,
    /// Current week (all models), 0..1.
    pub week: Option<f64>,
    /// Unix ms when it was read.
    pub at: u64,
}

/// Don't ask `/usage` more often than this.
const USAGE_EVERY: std::time::Duration = std::time::Duration::from_secs(20);

/// Reads the plan usage again in the background (after a reply, on the sign-in
/// check). Asks made while a read is under way get one more read after it.
pub fn refresh_usage(app: &AppHandle) {
    use std::sync::atomic::AtomicBool;
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
            if let Some((session, week)) = claude::find_cli().and_then(|cli| claude::usage(&cli)) {
                update(&app, |i| i.usage = Some(Usage { session, week, at: now_ms() }));
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
    /// The skills Claude was given when it started.
    Init { run: u64, skills: Vec<String> },
    /// A message written while Claude worked, passed on to it right away.
    Followup { run: u64, prompt: String },
    Text { run: u64, text: String },
    /// `at` = unix ms when it started
    Step { run: u64, id: String, icon: &'static str, label: String, detail: Option<String>, at: u64 },
    StepDone { run: u64, id: String, ok: bool, output: Option<String>, label: String, duration_ms: u64 },
    Done { run: u64, ok: bool, stopped: bool, message: Option<String>, duration_ms: u64, denied: Vec<Denied>, tokens: u64 },
}

struct Session {
    status: SessionStatus,
    /// The agent's session id (Claude Code's, or Antigravity's conversation id), for resuming.
    claude: Option<String>,
    /// Claude Code for this session, kept between messages.
    proc: Option<Proc>,
    stopping: bool,
    /// The running message.
    run: u64,
    /// When it was sent.
    run_started: Option<Instant>,
    /// The process working on it (Proc::serial); 0 = none (yet), or it has answered.
    run_proc: u64,
    /// Its messages Claude hasn't taken up yet.
    pending: u32,
    /// Claude's permission mode (one of MODES); can change while Claude works.
    mode: String,
    /// Slash commands Claude listed when it last started here.
    slash: Vec<String>,
}

/// A running Claude Code (or Antigravity CLI) process.
struct Proc {
    serial: u64,
    pid: u32,
    /// None for Gemini: it takes its one message on the command line.
    stdin: Option<ChildStdin>,
    /// What it was started with; a message that needs others restarts it.
    allow: Vec<String>,
    model: Option<String>,
}

#[derive(Default)]
struct Inner {
    sessions: Vec<Session>,
    recent: Vec<String>,
    limit: Option<Limit>,
    usage: Option<Usage>,
    default_provider: String,
    next_id: u64,
}

#[derive(Default)]
pub struct Agent {
    inner: Mutex<Inner>,
    runs: AtomicU64,
    procs: AtomicU64,
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
        usage: inner.usage.clone(),
        default_provider: provider_or_default(&inner.default_provider).to_string(),
    }
}

/// One of PROVIDERS; anything else is Claude.
fn provider_or_default(p: &str) -> &str {
    if PROVIDERS.contains(&p) { p } else { "claude" }
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

fn add_session(inner: &mut Inner, project: String, provider: &str) -> u64 {
    inner.next_id += 1;
    let id = inner.next_id;
    let provider = provider_or_default(provider).to_string();
    let caveman = if provider == "claude" { claude::caveman_expected() } else { None };
    inner.sessions.push(Session {
        status: SessionStatus { id, project, provider, caveman, ..Default::default() },
        claude: None,
        proc: None,
        stopping: false,
        run: 0,
        run_started: None,
        run_proc: 0,
        pending: 0,
        mode: String::new(),
        slash: Vec::new(),
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
    cfg.session_providers = inner.sessions.iter().map(|s| s.status.provider.clone()).collect();
    cfg.project = cfg.sessions.first().cloned();
    cfg.default_provider = provider_or_default(&inner.default_provider).to_string();
    cfg.save();
    inner.recent = cfg.recent_projects;
}

/// Re-reads the open sessions' changes in the background: that reads and diffs
/// every pending file, and commands run on the UI thread.
fn sync_review(app: &AppHandle) {
    static TRACKING: Mutex<()> = Mutex::new(());
    let app = app.clone();
    std::thread::spawn(move || {
        let _one_at_a_time = TRACKING.lock().unwrap();
        // read now, not when asked: the last one to run sees the latest sessions
        let projects: Vec<String> = {
            let agent = app.state::<Agent>();
            let inner = agent.inner.lock().unwrap();
            inner.sessions.iter().map(|s| s.status.project.clone()).collect()
        };
        review::track(&app, projects);
    });
}

/// Restores the open sessions and the recent list from config.json.
pub fn init(app: &AppHandle, cfg: &Config) {
    let mut projects = cfg.sessions.clone();
    if projects.is_empty() {
        projects.extend(cfg.project.clone());
    }
    update(app, |i| {
        i.recent = cfg.recent_projects.clone();
        i.default_provider = provider_or_default(&cfg.default_provider).to_string();
        let opened = projects.into_iter().enumerate().filter(|(_, p)| Path::new(p).is_dir()).take(MAX_SESSIONS);
        for (n, p) in opened {
            let provider = cfg.session_providers.get(n).map(String::as_str).unwrap_or("claude");
            add_session(i, p, provider);
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

#[derive(Default)]
struct Outcome {
    result: Option<(bool, String)>,
    denied: Vec<Denied>,
    auth_failed: bool,
}

/// "/cost", "/review 12": Claude Code doesn't echo these back when it takes them up.
fn is_command(prompt: &str) -> bool {
    prompt.strip_prefix('/').and_then(|r| r.chars().next()).is_some_and(|c| !c.is_whitespace())
}

fn run(app: AppHandle, sid: u64, prompt: String, mode: String, allow: Vec<String>, model: Option<String>) {
    let run = app.state::<Agent>().runs.fetch_add(1, Ordering::SeqCst) + 1;
    emit(&app, sid, AgentEvent::Start { run, prompt: prompt.clone() });
    let project = with_session(&app, sid, |s| {
        s.run = run;
        s.run_started = Some(Instant::now());
        s.run_proc = 0;
        s.status.project.clone()
    });
    paths::log(format!("agent run {run} (session {sid}) in {} ({mode})", project.unwrap_or_default()));
    if let Err(e) = deliver(&app, sid, &prompt, &mode, &allow, model.as_deref()) {
        finish(&app, sid, run, Err(e), true);
    }
}

/// Hands a message to the session's Claude; starts one when there's none that
/// fits (another model, or tools it wasn't allowed).
fn deliver(app: &AppHandle, sid: u64, prompt: &str, mode: &str, allow: &[String], model: Option<&str>) -> Result<(), String> {
    let pending = u32::from(!is_command(prompt));
    // None = sent; Some = start a new one, after ending the old one (its pid)
    let stale = with_session(app, sid, |s| -> Result<(bool, Option<Option<u32>>), String> {
        if s.stopping {
            return Err("Stopped".into());
        }
        let gemini = s.status.provider == "gemini";
        let fits = |p: &&mut Proc| p.model.as_deref() == model && allow.iter().all(|a| p.allow.contains(a));
        if let Some(p) = s.proc.as_mut().filter(fits) {
            // the mode goes first, so the message already runs in it
            let mut msg = if s.mode != mode { mode_request(mode) } else { String::new() };
            msg.push_str(&user_message(prompt));
            if let Some(stdin) = p.stdin.as_mut() {
                if stdin.write_all(msg.as_bytes()).and_then(|_| stdin.flush()).is_ok() {
                    s.run_proc = p.serial;
                    s.pending = pending;
                    s.mode = mode.to_string();
                    return Ok((gemini, None));
                }
            }
        }
        Ok((gemini, Some(s.proc.take().map(|p| p.pid))))
    })
    .ok_or("That session was closed.")??;
    let (gemini, Some(old)) = stale else { return Ok(()) };
    if let Some(pid) = old {
        kill_tree(pid);
    }
    if gemini {
        start_gemini(app, sid, prompt, mode, model)
    } else {
        start(app, sid, prompt, mode, allow, model, pending)
    }
}

/// Starts Claude Code for a session, with its first message.
fn start(app: &AppHandle, sid: u64, prompt: &str, mode: &str, allow: &[String], model: Option<&str>, pending: u32) -> Result<(), String> {
    let cli = claude::find_cli().ok_or("Claude Code isn't installed yet.")?;
    let (project, session) = with_session(app, sid, |s| (s.status.project.clone(), s.claude.clone())).ok_or("That session was closed.")?;

    let mut cmd = Command::new(&cli);
    cmd.args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--replay-user-messages"]);
    // the answer as it's written, for the live token count
    cmd.arg("--include-partial-messages");
    cmd.args(["--permission-prompts", "none"]);
    // "allow" only makes "reckless" reachable, so the mode can be switched to it
    // while Claude works; the mode itself decides
    cmd.args(["--allow-dangerously-skip-permissions", "--permission-mode", cli_mode(mode)]);
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
    let serial = app.state::<Agent>().procs.fetch_add(1, Ordering::SeqCst) + 1;
    paths::log(format!("agent session {sid}: Claude started in {project} ({mode})"));

    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        kill_tree(pid);
        return Err("no output from Claude".into());
    };
    // the prompt goes through stdin, so no command-line quoting is involved;
    // it stays open for what the user writes later
    if let Err(e) = stdin.write_all(user_message(prompt).as_bytes()).and_then(|_| stdin.flush()) {
        kill_tree(pid);
        return Err(format!("Couldn't reach Claude: {e}"));
    }
    let stderr_tail = Arc::new(Mutex::new(String::new()));
    if let Some(mut stderr) = child.stderr.take() {
        let tail = stderr_tail.clone();
        // the process lives long: keep only the end
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut t = tail.lock().unwrap();
                t.push_str(&String::from_utf8_lossy(&buf[..n]));
                if t.len() > 8192 {
                    let cut = (t.len() - 4096..t.len()).find(|&i| t.is_char_boundary(i)).unwrap_or(0);
                    t.drain(..cut);
                }
            }
        });
    }

    let proc = Proc { serial, pid, stdin: Some(stdin), allow: allow.to_vec(), model: model.map(str::to_string) };
    let kept = with_session(app, sid, move |s| {
        // stopped while it was starting
        if s.stopping {
            return false;
        }
        s.proc = Some(proc);
        s.run_proc = serial;
        s.pending = pending;
        s.mode = mode.to_string();
        true
    })
    .unwrap_or(false);
    if !kept {
        kill_tree(pid);
        let _ = child.wait();
        return Err("Stopped".into());
    }
    let app = app.clone();
    std::thread::spawn(move || read(app, sid, serial, child, stdout, stderr_tail, project));
    Ok(())
}

/// How often the live token count goes to the UI while Claude writes.
const TOKENS_EVERY: std::time::Duration = std::time::Duration::from_millis(250);

/// Output tokens of one message, from Claude's partial messages: the exact
/// count of each finished API call, plus an estimate for the one being written
/// (about 4 characters a token, as Claude Code's own spinner counts).
#[derive(Default)]
struct Tokens {
    run: u64,
    /// finished calls
    done: u64,
    /// characters of the call being written
    chars: u64,
    /// partial messages came at all
    streamed: bool,
    shown: u64,
    shown_at: Option<Instant>,
}

impl Tokens {
    /// Takes one `stream_event`; returns the count when the UI should get it.
    fn take(&mut self, event: &Value) -> Option<u64> {
        self.streamed = true;
        let mut now = false;
        match event.get("type").and_then(Value::as_str) {
            Some("message_start") => self.chars = 0,
            Some("content_block_delta") => {
                let delta = event.get("delta");
                let piece = ["text", "thinking", "partial_json"].iter().find_map(|k| delta.and_then(|d| d.get(*k)).and_then(Value::as_str));
                self.chars += piece.map_or(0, |p| p.chars().count() as u64);
            }
            // the call is over: its real count replaces the estimate
            Some("message_delta") => {
                if let Some(n) = event.pointer("/usage/output_tokens").and_then(Value::as_u64) {
                    self.done += n;
                    self.chars = 0;
                    now = true;
                }
            }
            _ => {}
        }
        let total = self.done + self.chars / 4;
        let due = now || self.shown_at.is_none_or(|t| t.elapsed() >= TOKENS_EVERY);
        if total == self.shown || !due {
            return None;
        }
        self.shown = total;
        self.shown_at = Some(Instant::now());
        Some(total)
    }
}

/// Follows a Claude process until it exits: its events become steps of the
/// running message, and the answer to that message's last part ends it.
fn read(app: AppHandle, sid: u64, serial: u64, child: Child, stdout: ChildStdout, stderr_tail: Arc<Mutex<String>>, project: String) {
    let app = &app;
    let mut out = Outcome::default();
    // a caveman hook ran (and reported its level); they run when Claude starts
    let mut caveman = false;
    let mut tools: HashMap<String, String> = HashMap::new();
    let mut step_started: HashMap<String, Instant> = HashMap::new();
    // steps that change files: refresh the review list when they finish
    let mut edits: HashSet<String> = HashSet::new();
    // Claude wrote something in this turn
    let mut said = false;
    let mut tokens = Tokens::default();
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(ev) = serde_json::from_str::<Value>(&line) else { continue };
        // only what belongs to a message this process works on
        let run = with_session(app, sid, |s| {
            if !s.status.running || s.run_proc != serial {
                return None;
            }
            // not once it's being ended (a new conversation may be starting)
            let current = s.proc.as_ref().is_some_and(|p| p.serial == serial);
            if let Some(id) = ev.get("session_id").and_then(Value::as_str).filter(|_| current) {
                if s.claude.as_deref() != Some(id) {
                    s.claude = Some(id.to_string());
                }
            }
            Some(s.run)
        });
        let Some(run) = run.flatten() else { continue };
        // sub-agent chatter stays inside its "Delegating" step
        if ev.get("parent_tool_use_id").is_some_and(|p| !p.is_null()) {
            continue;
        }
        if tokens.run != run {
            tokens = Tokens { run, ..Default::default() };
        }
        match ev.get("type").and_then(Value::as_str) {
            Some("system") if ev.get("subtype").and_then(Value::as_str) == Some("init") => {
                if let Some(m) = ev.get("model").and_then(Value::as_str) {
                    let m = m.to_string();
                    session_update(app, sid, |s| s.status.model = Some(m));
                }
                let skills: Vec<String> = ev
                    .get("skills")
                    .and_then(Value::as_array)
                    .map(|l| l.iter().filter_map(Value::as_str).map(str::to_string).collect())
                    .unwrap_or_default();
                if !skills.is_empty() {
                    emit(app, sid, AgentEvent::Init { run, skills });
                }
                if let Some(list) = ev.get("slash_commands").and_then(Value::as_array) {
                    let slash: Vec<String> = list.iter().filter_map(Value::as_str).map(|n| n.trim_start_matches('/').to_string()).collect();
                    with_session(app, sid, |s| s.slash = slash);
                }
            }
            // Claude took up one of the messages
            Some("user") if ev.get("isReplay").and_then(Value::as_bool) == Some(true) => {
                with_session(app, sid, |s| s.pending = s.pending.saturating_sub(1));
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
                                said = true;
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
                    if edited && with_session(app, sid, |s| s.mode != "reckless").unwrap_or(true) {
                        review::refresh(app, &project);
                    }
                }
            }
            // caveman's hooks say which level Claude answers with
            Some("system") if ev.get("subtype").and_then(Value::as_str) == Some("hook_response") => {
                if let Some(level) = ev.get("output").and_then(Value::as_str).and_then(claude::caveman_level) {
                    caveman = true;
                    session_update(app, sid, |s| s.status.caveman = Some(level));
                }
            }
            // Claude confirms a mode switch (or says why it refused one)
            Some("system") if ev.get("subtype").and_then(Value::as_str) == Some("status") => {
                if let Some(m) = ev.get("permissionMode").and_then(Value::as_str) {
                    let m = app_mode(m).to_string();
                    with_session(app, sid, |s| s.mode = m);
                }
            }
            Some("control_response") => {
                if let Some(e) = ev.pointer("/response/error").and_then(Value::as_str) {
                    paths::log(format!("agent run {run}: {e}"));
                }
            }
            Some("rate_limit_event") => {
                if let Some(info) = ev.get("rate_limit_info") {
                    record_limit(app, info);
                }
            }
            Some("stream_event") => {
                if let Some(n) = ev.get("event").and_then(|e| tokens.take(e)) {
                    session_update(app, sid, |s| s.status.tokens = n);
                }
            }
            Some("result") => {
                // no partial messages (an older Claude Code): the turn's total, at its end
                if !tokens.streamed {
                    if let Some(n) = ev.pointer("/usage/output_tokens").and_then(Value::as_u64).filter(|n| *n > 0) {
                        tokens.done += n;
                        session_update(app, sid, |s| s.status.tokens = tokens.done);
                    }
                }
                let is_error = ev.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                let text = ev.get("result").and_then(Value::as_str).unwrap_or("").to_string();
                // some slash commands (/context, /cost, …) answer only here
                if !is_error && !said && !text.trim().is_empty() {
                    emit(app, sid, AgentEvent::Text { run, text: text.trim().to_string() });
                }
                said = false;
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
                // a message Claude got but hasn't taken up yet still gets its turn
                // first; otherwise the run is over, and what the user writes from
                // now on goes out as the next one
                let last = with_session(app, sid, |s| {
                    let last = s.pending == 0;
                    if last {
                        s.run_proc = 0;
                    }
                    last
                })
                .unwrap_or(true);
                if last {
                    finish(app, sid, run, Ok(std::mem::take(&mut out)), caveman);
                    tools.clear();
                    step_started.clear();
                    edits.clear();
                }
            }
            _ => {}
        }
    }
    exited(app, sid, serial, child, out, &stderr_tail, caveman, "Claude");
}

/// What a dead process's stderr says went wrong: the last error message, not
/// the stack trace after it ("at process.processTicksAndRejections …").
fn last_words(tail: &str) -> String {
    let lines: Vec<&str> = tail.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let error = lines.iter().rev().find_map(|l| l.rfind("Error: ").map(|i| l[i + 7..].trim()));
    let plain = || lines.iter().rev().find(|l| !l.starts_with("at ") && !matches!(**l, "}" | "]" | "^")).copied();
    error.or_else(plain).or(lines.last().copied()).unwrap_or("").to_string()
}

/// The agent's process is gone; if that was in the middle of a message, that ends it.
#[allow(clippy::too_many_arguments)]
fn exited(app: &AppHandle, sid: u64, serial: u64, mut child: Child, out: Outcome, stderr_tail: &Mutex<String>, caveman: bool, who: &str) {
    let status = child.wait().map(|s| s.to_string()).unwrap_or_else(|e| e.to_string());
    let tail = stderr_tail.lock().unwrap().clone();
    let last = last_words(&tail);
    paths::log(format!("agent session {sid}: {who} exited ({status}) {last}"));
    // gone in the middle of a message: that ends it
    let ended = with_session(app, sid, |s| {
        if s.proc.as_ref().is_some_and(|p| p.serial == serial) {
            s.proc = None;
        }
        let ended = s.status.running && s.run_proc == serial;
        if ended {
            s.run_proc = 0;
        }
        ended.then_some((s.run, s.stopping))
    });
    let Some((run, stopping)) = ended.flatten() else { return };
    let result = if out.result.is_none() && !stopping && !last.is_empty() { Err(clip(&last, 300)) } else { Ok(out) };
    finish(app, sid, run, result, caveman);
}

// ---------------------------------------------------------------- gemini (Antigravity CLI)

/// Antigravity CLI's flags for one of MODES. Nobody can answer a prompt in
/// headless mode, so what a mode doesn't allow simply isn't run.
fn gemini_mode(mode: &str) -> &'static [&'static str] {
    match mode {
        "reckless" => &["--dangerously-skip-permissions"],
        "plan" => &["--mode", "plan"],
        _ => &["--mode", "accept-edits"],
    }
}

/// (icon, label, detail) for an Antigravity tool call, via its Claude Code twin.
/// Its parameter names vary (`AbsolutePath`, `TargetFile`, `CommandLine`, …),
/// so they're matched loosely.
fn describe_gemini(name: &str, p: &Value) -> (&'static str, String, Option<String>) {
    let s = |keys: &[&str]| {
        let obj = p.as_object();
        keys.iter()
            .find_map(|k| obj?.iter().find(|(n, v)| v.is_string() && n.replace('_', "").eq_ignore_ascii_case(k)).and_then(|(_, v)| v.as_str()))
            .unwrap_or("")
            .to_string()
    };
    let path = s(&["absolutepath", "filepath", "targetfile", "path", "dirpath", "directorypath", "searchpath", "searchdirectory"]);
    let (twin, input) = match name {
        "view_file" | "view_file_outline" | "read_file" => ("Read", serde_json::json!({ "file_path": path })),
        "write_to_file" | "write_file" => ("Write", serde_json::json!({ "file_path": path })),
        "replace_file_content" | "multi_replace_file_content" | "replace" | "edit" | "edit_file" => ("Edit", serde_json::json!({ "file_path": path })),
        "run_command" | "run_shell_command" => {
            ("Bash", serde_json::json!({ "command": s(&["commandline", "command"]), "description": s(&["description"]) }))
        }
        "find_by_name" | "glob" => ("Glob", serde_json::json!({ "pattern": s(&["pattern"]), "path": path })),
        "grep_search" | "search_file_content" | "grep" => ("Grep", serde_json::json!({ "pattern": s(&["query", "pattern"]), "path": path })),
        "search_web" | "google_web_search" => ("WebSearch", serde_json::json!({ "query": s(&["query"]) })),
        "read_url_content" | "web_fetch" => ("WebFetch", serde_json::json!({ "url": s(&["url"]) })),
        "list_dir" | "list_directory" => return ("search", format!("Listing {}", file_name(&path)), Some(path).filter(|d| !d.is_empty())),
        "codebase_search" => return ("search", format!("Searching the code for “{}”", clip(&s(&["query"]), 40)), None),
        other => return ("other", other.replace('_', " "), None),
    };
    describe(twin, &input)
}

/// Keeps the end of a long-lived process's stderr.
fn tail_of(mut stderr: impl Read + Send + 'static) -> Arc<Mutex<String>> {
    let tail = Arc::new(Mutex::new(String::new()));
    let t = tail.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            let mut t = t.lock().unwrap();
            t.push_str(&String::from_utf8_lossy(&buf[..n]));
            if t.len() > 8192 {
                let cut = (t.len() - 4096..t.len()).find(|&i| t.is_char_boundary(i)).unwrap_or(0);
                t.drain(..cut);
            }
        }
    });
    tail
}

/// Starts Antigravity CLI for one message (it continues the session's conversation).
fn start_gemini(app: &AppHandle, sid: u64, prompt: &str, mode: &str, model: Option<&str>) -> Result<(), String> {
    if !gemini::signed_in() {
        return Err("Gemini isn't signed in — sign in from Settings (the gear icon, Ctrl+,).".into());
    }
    let mut cmd = gemini::command()?;
    let (project, session) = with_session(app, sid, |s| (s.status.project.clone(), s.claude.clone())).ok_or("That session was closed.")?;
    cmd.args(["--output-format", "stream-json"]).args(gemini_mode(mode));
    if let Some(id) = &session {
        cmd.args(["--conversation", id]);
    }
    // Claude's model names mean nothing to Gemini
    if let Some(m) = model.filter(|m| m.starts_with("gemini")) {
        cmd.args(["--model", m]);
    }
    cmd.arg("--print").arg(prompt);
    cmd.current_dir(&project).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start Gemini: {e}"))?;
    let pid = child.id();
    let serial = app.state::<Agent>().procs.fetch_add(1, Ordering::SeqCst) + 1;
    paths::log(format!("agent session {sid}: Gemini started in {project} ({mode})"));
    let Some(stdout) = child.stdout.take() else {
        kill_tree(pid);
        return Err("no output from Gemini".into());
    };
    let stderr_tail = child.stderr.take().map(tail_of).unwrap_or_default();
    let proc = Proc { serial, pid, stdin: None, allow: Vec::new(), model: model.map(str::to_string) };
    let kept = with_session(app, sid, move |s| {
        if s.stopping {
            return false;
        }
        s.proc = Some(proc);
        s.run_proc = serial;
        s.pending = 0;
        s.mode = mode.to_string();
        true
    })
    .unwrap_or(false);
    if !kept {
        kill_tree(pid);
        let _ = child.wait();
        return Err("Stopped".into());
    }
    let app = app.clone();
    std::thread::spawn(move || read_gemini(app, sid, serial, child, stdout, stderr_tail, project));
    Ok(())
}

/// Gemini's errors that mean "sign in again".
fn is_auth_error(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    ["unauthenticated", "authentication", "invalid_grant", "status 401", "code: 401", "login required", "please log in", "please login", "reauthenticate"]
        .iter()
        .any(|k| t.contains(k))
}

/// A tool step Antigravity is running, by its step_index.
struct GeminiTool {
    id: String,
    label: String,
    started: Instant,
    edits: bool,
    done: bool,
}

/// Follows an Antigravity CLI process: its step updates become the message's steps and text.
fn read_gemini(app: AppHandle, sid: u64, serial: u64, child: Child, stdout: ChildStdout, stderr_tail: Arc<Mutex<String>>, project: String) {
    let app = &app;
    let mut out = Outcome::default();
    let mut tools: HashMap<u64, GeminiTool> = HashMap::new();
    // replies that arrived in pieces while being written (their DONE repeats nothing new)
    let mut streamed: HashSet<u64> = HashSet::new();
    // the answer arrives in pieces; it is shown in one go before the next step
    let mut text = String::new();
    let mut said = false;
    let flush = |text: &mut String, run: u64| {
        let t = text.trim();
        let any = !t.is_empty();
        if any {
            emit(app, sid, AgentEvent::Text { run, text: t.to_string() });
        }
        text.clear();
        any
    };
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(ev) = serde_json::from_str::<Value>(&line) else { continue };
        let run = with_session(app, sid, |s| {
            if !s.status.running || s.run_proc != serial {
                return None;
            }
            let id = ["/conversation_id", "/step_update/conversation_id", "/result/conversation_id"].iter().find_map(|p| ev.pointer(p).and_then(Value::as_str));
            if let Some(id) = id.filter(|id| !id.is_empty()) {
                s.claude = Some(id.to_string());
            }
            Some(s.run)
        });
        let Some(run) = run.flatten() else { continue };
        match ev.get("event").and_then(Value::as_str) {
            Some("init") => {
                if let Some(m) = ev.pointer("/init/model").and_then(Value::as_str).filter(|m| !m.is_empty()) {
                    let m = m.to_string();
                    session_update(app, sid, |s| s.status.model = Some(m));
                }
            }
            Some("step_update") => {
                let Some(u) = ev.get("step_update") else { continue };
                let index = u.get("step_index").and_then(Value::as_u64).unwrap_or(0);
                let done = u.get("state").and_then(Value::as_str) == Some("DONE");
                match u.get("step_type").and_then(Value::as_str) {
                    Some("agent_response") => {
                        let delta = u.get("text_delta").and_then(Value::as_str).unwrap_or("");
                        if !done && !delta.is_empty() {
                            streamed.insert(index);
                            text.push_str(delta);
                        } else if done && !streamed.contains(&index) {
                            text.push_str(delta);
                        }
                    }
                    Some("tool") => {
                        if !tools.contains_key(&index) {
                            said |= flush(&mut text, run);
                            let name = u.get("tool_name").or_else(|| u.pointer("/tool_info/name")).and_then(Value::as_str).unwrap_or("tool");
                            let params = u.pointer("/tool_info/parameters").cloned().unwrap_or(Value::Null);
                            let (icon, label, detail) = describe_gemini(name, &params);
                            let id = format!("agy-{index}");
                            let at = now_ms();
                            tools.insert(index, GeminiTool {
                                id: id.clone(),
                                label: label.clone(),
                                started: Instant::now(),
                                edits: icon == "edit" || icon == "create",
                                done: false,
                            });
                            session_update(app, sid, |s| {
                                s.status.current = Some(label.clone());
                                s.status.current_since = Some(at);
                            });
                            emit(app, sid, AgentEvent::Step { run, id, icon, label, detail, at });
                        }
                        let Some(tool) = tools.get_mut(&index).filter(|t| done && !t.done) else { continue };
                        tool.done = true;
                        let error = u.pointer("/tool_info/error/message").and_then(Value::as_str);
                        let output = u
                            .pointer("/tool_info/output")
                            .and_then(Value::as_str)
                            .filter(|o| !o.is_empty())
                            .or(error)
                            .map(|o| clip(o, MAX_OUTPUT))
                            .filter(|o| !o.is_empty());
                        session_update(app, sid, |s| {
                            s.status.current = Some("Thinking…".into());
                            s.status.current_since = None;
                        });
                        emit(app, sid, AgentEvent::StepDone {
                            run,
                            id: tool.id.clone(),
                            ok: error.is_none(),
                            output,
                            label: tool.label.clone(),
                            duration_ms: tool.started.elapsed().as_millis() as u64,
                        });
                        if tool.edits && with_session(app, sid, |s| s.mode != "reckless").unwrap_or(true) {
                            review::refresh(app, &project);
                        }
                    }
                    _ => {}
                }
            }
            Some("result") => {
                let r = ev.get("result").cloned().unwrap_or(Value::Null);
                let status = r.get("status").and_then(Value::as_str).unwrap_or("");
                let response = r.get("response").and_then(Value::as_str).unwrap_or("");
                // nothing came in pieces: the whole answer is here
                if !said && text.trim().is_empty() {
                    text.push_str(response);
                }
                said |= flush(&mut text, run);
                let answered = said || !response.trim().is_empty();
                // a resumed conversation can end "INTERRUPTED" right after a complete reply
                let ok = status == "SUCCESS" || (status == "INTERRUPTED" && answered);
                let err = r
                    .get("error")
                    .and_then(Value::as_str)
                    .filter(|e| !e.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("Gemini stopped ({})", status.to_ascii_lowercase()));
                if !ok {
                    paths::log(format!("agent run {run}: gemini: {err}"));
                    out.auth_failed |= is_auth_error(&err);
                }
                out.result = Some((!ok, if ok { String::new() } else { err }));
                with_session(app, sid, |s| s.run_proc = 0);
                finish(app, sid, run, Ok(std::mem::take(&mut out)), false);
            }
            _ => {}
        }
    }
    // gone without a result (signed out, a crash): its last words explain it
    if out.result.is_none() && is_auth_error(&stderr_tail.lock().unwrap()) {
        out.auth_failed = true;
        out.result = Some((true, String::new()));
    }
    exited(app, sid, serial, child, out, &stderr_tail, false, "Gemini");
}

/// The running message is over: review what changed, go idle, tell the UI.
fn finish(app: &AppHandle, sid: u64, run: u64, result: Result<Outcome, String>, caveman: bool) {
    // the user may have switched modes while Claude worked
    let mode = with_session(app, sid, |s| s.mode.clone()).unwrap_or_default();
    let project = with_session(app, sid, |s| s.status.project.clone());
    if let Some(project) = &project {
        if mode == "reckless" {
            // reckless: whatever Claude changed is kept without asking
            let _ = review::accept_all(app, project);
        } else {
            review::refresh(app, project);
        }
    }

    let (stopped, started, tokens) =
        with_session(app, sid, |s| (std::mem::take(&mut s.stopping), s.run_started.take(), s.status.tokens)).unwrap_or((true, None, 0));
    let gemini = with_session(app, sid, |s| s.status.provider == "gemini").unwrap_or(false);
    let who = if gemini { "Gemini" } else { "Claude" };
    // Claude answered without caveman's hooks: the plugin is off
    let no_caveman = !gemini && !caveman && matches!(&result, Ok(o) if o.result.is_some());
    let (ok, message, denied) = match result {
        _ if stopped => (false, Some("Stopped".to_string()), Vec::new()),
        Err(e) => (false, Some(e), Vec::new()),
        Ok(out) if out.auth_failed => {
            if gemini {
                gemini::report_auth_failure(app);
            } else {
                claude::report_auth_failure(app);
            }
            (false, Some(format!("{who}'s sign-in has expired. Sign in again in Settings, then send your message again.")), Vec::new())
        }
        Ok(out) => {
            if gemini {
                gemini::report_auth_ok(app);
            } else {
                claude::report_auth_ok(app);
            }
            match out.result {
                Some((false, _)) => (true, None, out.denied),
                Some((true, text)) => (false, Some(clip(&text, 300)), out.denied),
                None => (false, Some(format!("{who} stopped without an answer.")), out.denied),
            }
        }
    };
    // idle first, so a message queued in the UI can go out as soon as it hears "done"
    session_update(app, sid, |s| {
        s.status.running = false;
        s.status.current = None;
        s.status.current_since = None;
        s.status.started_at = None;
        if no_caveman {
            s.status.caveman = None;
        }
    });
    emit(app, sid, AgentEvent::Done {
        run,
        ok,
        stopped,
        message,
        duration_ms: started.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0),
        denied,
        tokens,
    });
    // the reply used some of the plan
    if !gemini {
        refresh_usage(app);
    }
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

/// Opens a new session in `path` with `provider` (default: the chosen default agent); returns its id.
#[tauri::command]
pub fn agent_open(app: AppHandle, path: String, provider: Option<String>) -> Result<u64, String> {
    check_dir(&path)?;
    let mut result = Err("You can have up to 9 sessions open.".to_string());
    update(&app, |i| {
        if i.sessions.len() < MAX_SESSIONS {
            let provider = provider.unwrap_or_else(|| i.default_provider.clone());
            result = Ok(add_session(i, path.clone(), &provider));
            persist(i, Some(&path));
        }
    });
    sync_review(&app);
    result
}

/// Switches an idle session to the other agent (a new conversation starts).
#[tauri::command]
pub fn agent_set_provider(app: AppHandle, sid: u64, provider: String) -> Result<(), String> {
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err("Unknown agent.".into());
    }
    let mut result = Ok(());
    update(&app, |i| {
        match i.sessions.iter_mut().find(|s| s.status.id == sid) {
            Some(s) if s.status.running => result = Err("Wait until it has finished.".to_string()),
            Some(s) => {
                s.status.caveman = if provider == "claude" { claude::caveman_expected() } else { None };
                s.status.provider = provider.clone();
                s.status.model = None;
                s.claude = None;
                s.slash.clear();
            }
            None => result = Err("That session was closed.".to_string()),
        }
        if result.is_ok() {
            persist(i, None);
        }
    });
    if result.is_ok() {
        end_proc(&app, sid, false);
    }
    result
}

/// The agent new sessions use.
#[tauri::command]
pub fn agent_set_default_provider(app: AppHandle, provider: String) -> Result<(), String> {
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err("Unknown agent.".into());
    }
    update(&app, |i| {
        i.default_provider = provider;
        persist(i, None);
    });
    Ok(())
}

/// Closes a session (stopping Claude first if it's working).
#[tauri::command]
pub fn agent_close(app: AppHandle, sid: u64) {
    end_proc(&app, sid, false);
    update(&app, |i| {
        i.sessions.retain(|s| s.status.id != sid);
        persist(i, None);
    });
    sync_review(&app);
}

/// Drops a folder from the recent list (its pill on Home).
#[tauri::command]
pub fn agent_forget(app: AppHandle, path: String) {
    update(&app, |i| {
        let mut cfg = Config::load();
        cfg.recent_projects.retain(|p| !p.eq_ignore_ascii_case(&path));
        cfg.save();
        i.recent = cfg.recent_projects;
    });
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
    if result.is_ok() {
        // the one there works in the old folder
        end_proc(&app, sid, false);
    }
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
            s.status.tokens = 0;
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

pub(crate) fn kill_tree(pid: u32) {
    let mut cmd = Command::new("taskkill");
    cmd.args(["/T", "/F", "/PID", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd.status();
}

/// One line of `--input-format stream-json`.
fn user_message(text: &str) -> String {
    let msg = serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": text },
        "parent_tool_use_id": null,
    });
    format!("{msg}\n")
}

/// Claude Code's name for one of MODES.
fn cli_mode(mode: &str) -> &str {
    if mode == "reckless" { "bypassPermissions" } else { mode }
}

/// The other way round.
fn app_mode(mode: &str) -> &str {
    if mode == "bypassPermissions" { "reckless" } else { mode }
}

/// One line asking Claude to switch its permission mode.
fn mode_request(mode: &str) -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let msg = serde_json::json!({
        "type": "control_request",
        "request_id": format!("mode-{}", N.fetch_add(1, Ordering::Relaxed)),
        "request": { "subtype": "set_permission_mode", "mode": cli_mode(mode) },
    });
    format!("{msg}\n")
}

/// Switches the permission mode of every running session right away (the
/// next message gets it anyway). Claude confirms with a `status` event.
#[tauri::command]
pub fn agent_set_mode(app: AppHandle, mode: String) {
    if !MODES.contains(&mode.as_str()) {
        return;
    }
    let agent = app.state::<Agent>();
    let mut inner = agent.inner.lock().unwrap();
    for s in inner.sessions.iter_mut().filter(|s| s.status.running && !s.stopping && s.mode != mode) {
        let run_proc = s.run_proc;
        let Some(stdin) = s.proc.as_mut().filter(|p| p.serial == run_proc).and_then(|p| p.stdin.as_mut()) else { continue };
        match stdin.write_all(mode_request(&mode).as_bytes()).and_then(|_| stdin.flush()) {
            Ok(()) => paths::log(format!("agent session {}: mode -> {mode}", s.status.id)),
            Err(e) => paths::log(format!("agent session {}: couldn't switch mode: {e}", s.status.id)),
        }
    }
}

/// Hands a message to Claude while it works; it reads it at its next step.
/// Errs when Claude isn't taking messages (idle, stopping, or already done);
/// the UI then sends it as the next message instead.
#[tauri::command]
pub fn agent_inject(app: AppHandle, sid: u64, prompt: String) -> Result<(), String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Ok(());
    }
    with_session(&app, sid, |s| -> Result<(), String> {
        let run = s.run;
        // run_proc is 0 once Claude has answered the last message
        let taking = s.status.running && !s.stopping;
        let run_proc = s.run_proc;
        let stdin = s
            .proc
            .as_mut()
            .filter(|p| taking && p.serial == run_proc)
            .and_then(|p| p.stdin.as_mut())
            .ok_or("Claude isn't taking messages right now.")?;
        stdin
            .write_all(user_message(&prompt).as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("Couldn't reach Claude: {e}"))?;
        s.pending += u32::from(!is_command(&prompt));
        // still under the lock, so it always comes before the run's "done"
        emit(&app, sid, AgentEvent::Followup { run, prompt: prompt.clone() });
        Ok(())
    })
    .unwrap_or_else(|| Err("That session was closed.".into()))
}

/// Stops the running message of a session (and everything Claude started for it).
#[tauri::command]
pub fn agent_stop(app: AppHandle, sid: u64) {
    end_proc(&app, sid, true);
}

/// Ends the session's Claude process, stopping its message if it's working
/// on one; `busy_only`: leave an idle one running.
fn end_proc(app: &AppHandle, sid: u64, busy_only: bool) {
    // taskkill takes a while, and commands run on the UI thread; the session
    // has already let go of the process, so nothing waits for it to be gone
    if let Some(pid) = take_proc(app, sid, busy_only) {
        std::thread::spawn(move || kill_tree(pid));
    }
}

/// Detaches the session's process (see `end_proc`); returns its pid.
fn take_proc(app: &AppHandle, sid: u64, busy_only: bool) -> Option<u32> {
    with_session(app, sid, |s| {
        if s.status.running {
            s.stopping = true;
        } else if busy_only {
            return None;
        }
        s.proc.take().map(|p| p.pid)
    })
    .flatten()
}

/// Ends every session's Claude (on quit), before the app exits.
pub fn stop_all(app: &AppHandle) {
    let ids: Vec<u64> = app.state::<Agent>().inner.lock().unwrap().sessions.iter().map(|s| s.status.id).collect();
    for sid in ids {
        if let Some(pid) = take_proc(app, sid, false) {
            kill_tree(pid);
        }
    }
}

/// What "/" can run in this session's folder, for the chat bar's menu.
/// (`async`: off the UI thread, it reads skill and command folders.)
#[tauri::command(async)]
pub fn agent_commands(app: AppHandle, sid: u64) -> Result<Vec<commands::SlashCommand>, String> {
    let (project, slash) = with_session(&app, sid, |s| (s.status.project.clone(), s.slash.clone())).ok_or("That session was closed.")?;
    Ok(commands::list(&project, &slash))
}

/// Forgets the conversation; the next message starts a new one.
#[tauri::command]
pub fn agent_new(app: AppHandle, sid: u64) {
    end_proc(&app, sid, false);
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

#[tauri::command(async)]
pub fn agent_open_folder(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}
