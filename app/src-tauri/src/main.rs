// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `NotchCode.exe --terminal …` (Claude's notch-terminal): open the window and quit
    if let Some(code) = notch_code_lib::terminal_from_args() {
        std::process::exit(code);
    }
    notch_code_lib::run()
}
