//! Click-through everywhere except the visible notch.
//!
//! The window always has the size of the expanded panel, because resizing a
//! WebView2 window shows one stale frame cropped from the old layout (the notch
//! visibly jumps sideways). Its transparent pixels would still swallow clicks,
//! so we poll the cursor and let the window ignore mouse input whenever the
//! cursor is outside the notch. Hover is reported to the UI as `notch-hover`,
//! since DOM mouseenter/leave are unreliable while input is ignored.
//!
//! The window never takes the keyboard on its own (`focus: false`), so the open
//! notch borrows it via `focus` and hands it back to the previous window when
//! it closes; otherwise typing and shortcuts would need a click first.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{Emitter, WebviewWindow};
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow, IsWindow, SetForegroundWindow};

const POLL: Duration = Duration::from_millis(16);

#[derive(Clone, Copy)]
struct Geometry {
    /// window origin and width in physical pixels
    x: i32,
    y: i32,
    width: f64,
    scale: f64,
    /// notch size in logical pixels (without the ears)
    notch_w: f64,
    notch_h: f64,
}

static GEOMETRY: Mutex<Geometry> = Mutex::new(Geometry {
    x: 0,
    y: 0,
    width: 0.0,
    scale: 1.0,
    notch_w: 0.0,
    notch_h: 0.0,
});

pub fn set_window(x: i32, y: i32, width: f64, scale: f64) {
    let mut g = GEOMETRY.lock().unwrap();
    (g.x, g.y, g.width, g.scale) = (x, y, width, scale);
}

pub fn set_notch(width: f64, height: f64) {
    let mut g = GEOMETRY.lock().unwrap();
    (g.notch_w, g.notch_h) = (width, height);
}

fn contains(g: &Geometry, p: POINT) -> bool {
    let w = g.notch_w * g.scale;
    let left = g.x as f64 + (g.width - w) / 2.0;
    let (px, py) = (p.x as f64, p.y as f64);
    px >= left && px < left + w && py >= g.y as f64 && py < g.y as f64 + g.notch_h * g.scale
}

/// the window that had the keyboard before the open notch took it
static PREVIOUS: AtomicIsize = AtomicIsize::new(0);

/// Takes the keyboard (`on`) or gives it back to whoever had it before.
pub fn focus(window: &WebviewWindow, on: bool) {
    let Ok(own) = window.hwnd() else { return };
    let own = own.0 as isize;
    let foreground = unsafe { GetForegroundWindow() } as isize;
    if on {
        if foreground != own {
            PREVIOUS.store(foreground, Ordering::Relaxed);
            let _ = window.set_focus();
        }
    } else {
        let previous = PREVIOUS.swap(0, Ordering::Relaxed);
        // only if the notch still has it: a click elsewhere already moved it on
        if foreground == own && previous != 0 && unsafe { IsWindow(previous as HWND) } != 0 {
            unsafe { SetForegroundWindow(previous as HWND) };
        }
    }
}

pub fn spawn(window: WebviewWindow) {
    std::thread::spawn(move || {
        let mut inside: Option<bool> = None;
        loop {
            let mut p = POINT { x: 0, y: 0 };
            // fails e.g. on the secure desktop (UAC prompt); just try again later
            if unsafe { GetCursorPos(&mut p) } != 0 {
                let now = contains(&GEOMETRY.lock().unwrap(), p);
                if inside != Some(now) {
                    inside = Some(now);
                    let _ = window.set_ignore_cursor_events(!now);
                    let _ = window.emit("notch-hover", now);
                }
            }
            std::thread::sleep(POLL);
        }
    });
}
