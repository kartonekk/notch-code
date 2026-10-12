//! Reviewing Claude's file changes.
//!
//! Every coding run gets a Claude Code `PreToolUse` hook that starts this exe
//! with `--snapshot-hook` before Edit/Write touches a file. The hook copies the
//! file (once, before the first edit) into a per-project snapshot folder, so
//! the notch can list what changed, open a live diff in VS Code, and accept
//! (keep) or reject (restore) each change.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::paths;

const MANIFEST: &str = "manifest.json";
const LOCK: &str = "manifest.lock";
/// The hook finds its snapshot folder here (inherited through Claude).
pub const DIR_ENV: &str = "NOTCH_REVIEW_DIR";
const EDIT_TOOLS: &str = "Edit|MultiEdit|Write|NotebookEdit";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// path -> snapshot file name in the folder (None = the file didn't exist yet)
#[derive(Serialize, Deserialize, Default)]
struct Manifest {
    files: BTreeMap<String, Option<String>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileChange {
    pub path: String,
    pub name: String,
    /// "modified" | "created" | "deleted"
    pub status: &'static str,
    pub added: usize,
    pub removed: usize,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct ReviewStatus {
    /// project folder -> Claude's changes there, waiting for accept / reject
    pub projects: HashMap<String, Vec<FileChange>>,
    /// Live diff: every changed file opens in VS Code as it changes.
    pub live: bool,
    /// VS Code's `code` command was found.
    pub vscode: bool,
}

#[derive(Default)]
pub struct Review {
    status: Mutex<ReviewStatus>,
    /// Files whose live diff is already open.
    opened: Mutex<HashSet<String>>,
}

fn fnv(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Snapshot folder of a project.
pub fn dir_for(project: &str) -> PathBuf {
    paths::data_dir().join("review").join(fnv(&project.to_lowercase()))
}

fn load(dir: &Path) -> Manifest {
    std::fs::read_to_string(dir.join(MANIFEST))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(dir: &Path, m: &Manifest) -> Result<(), String> {
    let json = serde_json::to_string_pretty(m).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(MANIFEST), json).map_err(|e| e.to_string())
}

/// Claude may run edits in parallel, each in its own hook process.
fn with_lock<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
    let lock = dir.join(LOCK);
    let deadline = Instant::now() + Duration::from_secs(3);
    let held = loop {
        match OpenOptions::new().write(true).create_new(true).open(&lock) {
            Ok(_) => break true,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break false, // stale lock: go ahead anyway
        }
    };
    let out = f();
    if held {
        let _ = std::fs::remove_file(&lock);
    }
    out
}

fn key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\")
}

// ---------------------------------------------------------------- hook process

/// When started as Claude's hook: snapshot the file about to be edited and
/// return true (the process should exit right away).
pub fn run_hook_if_requested() -> bool {
    if !std::env::args().any(|a| a == "--snapshot-hook") {
        return false;
    }
    if let Some(dir) = std::env::var_os(DIR_ENV) {
        let _ = snapshot_from_stdin(Path::new(&dir));
    }
    true
}

fn snapshot_from_stdin(dir: &Path) -> Result<(), String> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let input = v.get("tool_input").cloned().unwrap_or(Value::Null);
    let file = ["file_path", "notebook_path"]
        .iter()
        .find_map(|k| input.get(*k).and_then(Value::as_str))
        .unwrap_or("");
    if file.is_empty() {
        return Ok(());
    }
    let mut path = PathBuf::from(file);
    if path.is_relative() {
        if let Some(cwd) = v.get("cwd").and_then(Value::as_str) {
            path = Path::new(cwd).join(path);
        }
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    with_lock(dir, || {
        let mut m = load(dir);
        let k = key(&path);
        if m.files.contains_key(&k) {
            return Ok(()); // keep the original from before the first edit
        }
        let snap = if path.is_file() {
            let name = format!(
                "{}-{}",
                m.files.len(),
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            );
            std::fs::copy(&path, dir.join(&name)).map_err(|e| e.to_string())?;
            Some(name)
        } else {
            None
        };
        m.files.insert(k, snap);
        save(dir, &m)
    })
}

/// The --settings file that installs the snapshot hook for a run.
pub fn hook_settings(project: &str) -> Result<PathBuf, String> {
    let dir = dir_for(project);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut exe = short_path(&exe).replace('\\', "/");
    if exe.contains(' ') {
        exe = format!("\"{exe}\"");
    }
    let settings = serde_json::json!({
        "hooks": {
            "PreToolUse": [{
                "matcher": EDIT_TOOLS,
                "hooks": [{ "type": "command", "command": format!("{exe} --snapshot-hook"), "timeout": 30 }]
            }]
        }
    });
    let file = dir.join("hooks.json");
    std::fs::write(&file, settings.to_string()).map_err(|e| e.to_string())?;
    Ok(file)
}

/// 8.3 path without spaces, so the hook command needs no quoting in any shell.
#[cfg(windows)]
fn short_path(p: &Path) -> String {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return p.to_string_lossy().into_owned();
    }
    String::from_utf16_lossy(&buf[..n])
}

#[cfg(not(windows))]
fn short_path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

// ---------------------------------------------------------------- changes

fn count_lines(old: &[u8], new: &[u8]) -> (usize, usize) {
    let (old, new) = (String::from_utf8_lossy(old), String::from_utf8_lossy(new));
    let diff = similar::TextDiff::from_lines(old.as_ref(), new.as_ref());
    let (mut added, mut removed) = (0, 0);
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => added += 1,
            similar::ChangeTag::Delete => removed += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    (added, removed)
}

fn changes(project: &str) -> Vec<FileChange> {
    let dir = dir_for(project);
    let m = load(&dir);
    m.files
        .iter()
        .filter_map(|(path, snap)| {
            let old = snap.as_ref().and_then(|s| std::fs::read(dir.join(s)).ok());
            let new = std::fs::read(path).ok();
            let status = match (&old, &new) {
                (Some(o), Some(n)) if o == n => return None,
                (Some(_), Some(_)) => "modified",
                (None, Some(_)) => "created",
                (Some(_), None) => "deleted",
                (None, None) => return None,
            };
            let (added, removed) = count_lines(old.as_deref().unwrap_or(&[]), new.as_deref().unwrap_or(&[]));
            Some(FileChange {
                name: Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone()),
                path: path.clone(),
                status,
                added,
                removed,
            })
        })
        .collect()
}

fn emit_status(app: &AppHandle) {
    let snapshot = app.state::<Review>().status.lock().unwrap().clone();
    let _ = app.emit("review", snapshot);
}

/// Re-reads the changes of one project and tells the UI; with live diff on,
/// files that just started changing open in VS Code.
pub fn refresh(app: &AppHandle, project: &str) {
    let review = app.state::<Review>();
    let files = changes(project);
    let live = review.status.lock().unwrap().live;
    if live {
        let mut opened = review.opened.lock().unwrap();
        for f in &files {
            if opened.insert(f.path.clone()) {
                let _ = open_diff(project, &f.path);
            }
        }
    }
    {
        let mut s = review.status.lock().unwrap();
        s.projects.insert(project.to_string(), files);
        s.vscode = find_vscode().is_some();
    }
    emit_status(app);
}

/// The projects whose changes are shown (the folders of the open sessions).
pub fn track(app: &AppHandle, projects: Vec<String>) {
    {
        let review = app.state::<Review>();
        review.status.lock().unwrap().projects.retain(|p, _| projects.contains(p));
    }
    for p in &projects {
        refresh(app, p);
    }
    emit_status(app);
}

/// Drops the snapshot of `paths` (all when None); `restore` puts the originals back first.
fn settle(app: &AppHandle, project: &str, only: Option<String>, restore: bool) -> Result<(), String> {
    let dir = dir_for(project);
    with_lock(&dir, || -> Result<(), String> {
        let mut m = load(&dir);
        let targets: Vec<String> = match &only {
            Some(p) => m.files.keys().filter(|k| k.eq_ignore_ascii_case(p)).cloned().collect(),
            None => m.files.keys().cloned().collect(),
        };
        for path in targets {
            let snap = m.files.get(&path).cloned().flatten();
            if restore {
                match &snap {
                    Some(s) => {
                        std::fs::copy(dir.join(s), &path).map_err(|e| format!("Couldn't restore {path}: {e}"))?;
                    }
                    None => {
                        if Path::new(&path).exists() {
                            std::fs::remove_file(&path).map_err(|e| format!("Couldn't remove {path}: {e}"))?;
                        }
                    }
                }
            }
            if let Some(s) = snap {
                let _ = std::fs::remove_file(dir.join(s));
            }
            m.files.remove(&path);
            app.state::<Review>().opened.lock().unwrap().remove(&path);
        }
        save(&dir, &m)
    })?;
    refresh(app, project);
    Ok(())
}

/// Keeps every pending change (reckless mode, after each run).
pub fn accept_all(app: &AppHandle, project: &str) -> Result<(), String> {
    settle(app, project, None, false)
}

// ---------------------------------------------------------------- VS Code

fn find_vscode() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join("code.cmd")).collect::<Vec<_>>())
        .unwrap_or_default();
    let installs = [
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join(r"Programs\Microsoft VS Code\bin\code.cmd")),
        std::env::var_os("ProgramFiles").map(|d| PathBuf::from(d).join(r"Microsoft VS Code\bin\code.cmd")),
    ];
    on_path.into_iter().chain(installs.into_iter().flatten()).find(|p| p.is_file())
}

/// Opens original ↔ current side by side in VS Code. VS Code follows the file
/// on disk, so the right side updates live while Claude keeps editing.
fn open_diff(project: &str, path: &str) -> Result<(), String> {
    let code = find_vscode().ok_or("VS Code not found (install it, or add `code` to PATH).")?;
    let dir = dir_for(project);
    let m = load(&dir);
    let snap = m
        .files
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(path))
        .map(|(_, v)| v.clone())
        .ok_or("No changes recorded for that file")?;
    let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let left = match snap {
        Some(s) => dir.join(s),
        None => {
            // new file: compare against an empty one with the same extension
            let empty = dir.join(format!("empty-{name}"));
            if !empty.exists() {
                std::fs::write(&empty, b"").map_err(|e| e.to_string())?;
            }
            empty
        }
    };
    let mut cmd = Command::new("cmd");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.raw_arg(format!(
            "/s /c \"\"{}\" --reuse-window --diff \"{}\" \"{}\"\"",
            code.display(),
            left.display(),
            path
        ));
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("Couldn't start VS Code: {e}"))
}

// ---------------------------------------------------------------- commands

#[tauri::command]
pub fn review_status(state: tauri::State<Review>) -> ReviewStatus {
    state.status.lock().unwrap().clone()
}

/// Keep the changes (all, or one file).
#[tauri::command]
pub fn review_accept(app: AppHandle, project: String, path: Option<String>) -> Result<(), String> {
    settle(&app, &project, path, false)
}

/// Put the originals back (all, or one file).
#[tauri::command]
pub fn review_reject(app: AppHandle, project: String, path: Option<String>) -> Result<(), String> {
    if crate::agent::is_running_in(&app, &project) {
        return Err("Wait until Claude has finished.".into());
    }
    settle(&app, &project, path, true)
}

#[tauri::command]
pub fn review_diff(app: AppHandle, project: String, path: String) -> Result<(), String> {
    app.state::<Review>().opened.lock().unwrap().insert(path.clone());
    open_diff(&project, &path)
}

/// Live diff on: every changed file opens in VS Code as soon as it changes.
#[tauri::command]
pub fn review_set_live(app: AppHandle, live: bool) -> Result<(), String> {
    if live && find_vscode().is_none() {
        return Err("VS Code not found (install it, or add `code` to PATH).".into());
    }
    let projects: Vec<String> = {
        let review = app.state::<Review>();
        let mut s = review.status.lock().unwrap();
        s.live = live;
        if live {
            review.opened.lock().unwrap().clear();
        }
        s.projects.keys().cloned().collect()
    };
    for p in &projects {
        refresh(&app, p);
    }
    emit_status(&app);
    Ok(())
}
