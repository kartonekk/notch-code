//! Internet connectivity monitor for the notch indicator.
//!
//! Windows' own "online" flag only reflects the network adapter, so a router
//! that has lost its uplink still looks online. Instead we open a TCP
//! connection to a couple of public anycast hosts and time it.

use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

const HOSTS: [&str; 2] = ["1.1.1.1:443", "8.8.8.8:443"];
const TIMEOUT: Duration = Duration::from_millis(1500);
const INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Serialize)]
struct NetStatus {
    online: bool,
    /// TCP connect time to the first host that answered
    latency_ms: Option<u32>,
}

fn probe() -> NetStatus {
    for host in HOSTS {
        let addr: SocketAddr = host.parse().expect("valid probe address");
        let t = Instant::now();
        if TcpStream::connect_timeout(&addr, TIMEOUT).is_ok() {
            return NetStatus { online: true, latency_ms: Some(t.elapsed().as_millis() as u32) };
        }
    }
    NetStatus { online: false, latency_ms: None }
}

/// Emits a `net` event every couple of seconds for the lifetime of the app.
pub fn spawn_monitor(app: AppHandle) {
    std::thread::spawn(move || loop {
        let _ = app.emit("net", probe());
        std::thread::sleep(INTERVAL);
    });
}
