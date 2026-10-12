//! Visible terminal windows: the user's "Open terminal" button, and
//! `notch-terminal`, which lets Claude (whose shell is hidden) start something
//! the user can see and that keeps running — a dev server, a watcher, an
//! interactive tool.
//!
//! `notch-terminal` is a pair of tiny shims (a shell script for Claude's Bash,
//! a .cmd for PowerShell / cmd) on the PATH of every Claude run. Both call this
//! exe as `NotchCode.exe --terminal <command…>` (`main` handles that before the
//! app starts), which opens the window in the caller's folder and exits.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::paths;

#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

/// The exe the shims call (set on Claude's environment, so the shims never hold
/// a path that could change or need escaping).
pub const EXE_ENV: &str = "NOTCH_CODE_EXE";

/// What Claude is told about it (`--append-system-prompt`).
pub const PROMPT: &str = "You run in the background inside Notch Code: your shell is hidden from the user. \
To start something the user should see or that keeps running (a dev server, a file watcher, an interactive tool or REPL), \
open a visible terminal window with `notch-terminal <command>` (in PowerShell or cmd: `notch-terminal.cmd <command>`). \
It opens in the current directory, returns immediately, and the window stays open after the command ends, so you won't see its output. \
Use it only for that; run ordinary commands whose output you need as usual. \
`notch-terminal` with no command just opens a terminal there for the user.";

/// Opens a cmd window in `dir`, running `command` first if there is one; the
/// window stays open afterwards.
pub fn open(dir: &Path, command: Option<&str>) -> Result<(), String> {
    let mut cmd = Command::new("cmd.exe");
    cmd.current_dir(dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NEW_CONSOLE);
        if let Some(c) = command.map(str::trim).filter(|c| !c.is_empty()) {
            // /S: cmd strips just the outer quotes and runs the rest as written
            cmd.args(["/S", "/K"]).raw_arg(format!("\"{c}\""));
        }
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("Couldn't open a terminal: {e}"))
}

/// `NotchCode.exe --terminal [command…]`: opens the window and returns the exit
/// code; `None` when the exe was started normally.
pub fn from_args() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--terminal") {
        return None;
    }
    let command = args.map(|a| if a.contains(char::is_whitespace) && !a.starts_with('"') { format!("\"{a}\"") } else { a }).collect::<Vec<_>>().join(" ");
    let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match open(&dir, Some(&command)) {
        Ok(()) => Some(0),
        Err(e) => {
            eprintln!("{e}");
            Some(1)
        }
    }
}

/// Folder with the `notch-terminal` shims (written if missing or outdated).
pub fn shim_dir() -> Result<PathBuf, String> {
    let dir = paths::data_dir().join("bin");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let shims = [
        ("notch-terminal", format!("#!/bin/sh\nexec \"${EXE_ENV}\" --terminal \"$@\"\n")),
        ("notch-terminal.cmd", format!("@\"%{EXE_ENV}%\" --terminal %*\r\n")),
    ];
    for (name, body) in shims {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(body.as_str()) {
            std::fs::write(&path, body).map_err(|e| e.to_string())?;
        }
    }
    Ok(dir)
}

/// The user's button: a terminal in a session's folder.
#[tauri::command]
pub fn open_terminal(path: String) -> Result<(), String> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(format!("{path} doesn't exist anymore."));
    }
    open(&dir, None)
}
