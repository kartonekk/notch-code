//! First-run preparation: download the Whisper model (resumable) and load it.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::paths;

const MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";

/// Downloads `model` into the models folder unless it is already there.
/// `progress(downloaded, total)` is called a few times per second.
pub fn ensure_model(model: &str, mut progress: impl FnMut(u64, u64)) -> Result<PathBuf, String> {
    if model.contains(['/', '\\']) || !model.ends_with(".bin") {
        return Err(format!("Invalid model name in config.json: {model}"));
    }
    let path = paths::models_dir().join(model);
    if path.exists() && is_ggml(&path) {
        return Ok(path);
    }
    let part = path.with_extension("bin.part");
    let url = format!("{MODEL_BASE_URL}{model}");
    paths::log(format!("downloading {url}"));

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(None)
        .user_agent("NotchCode/0.1")
        .build()
        .map_err(|e| e.to_string())?;

    let mut attempts = 0;
    loop {
        attempts += 1;
        match download(&client, &url, &part, &mut progress) {
            Ok(()) => break,
            Err(e) if attempts < 5 => {
                paths::log(format!("download attempt {attempts} failed: {e}; retrying"));
                std::thread::sleep(Duration::from_secs(2 * attempts));
            }
            Err(e) => return Err(format!("Failed to download model: {e}")),
        }
    }

    if !is_ggml(&part) {
        let _ = std::fs::remove_file(&part);
        return Err("Downloaded model file is corrupted".into());
    }
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    paths::log("model downloaded");
    Ok(path)
}

fn download(
    client: &reqwest::blocking::Client,
    url: &str,
    part: &PathBuf,
    progress: &mut impl FnMut(u64, u64),
) -> Result<(), String> {
    let have = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    let mut req = client.get(url);
    if have > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let mut resp = req.send().map_err(|e| e.to_string())?;
    let status = resp.status();
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        return Ok(()); // already complete
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let mut file = if resumed {
        OpenOptions::new().append(true).open(part)
    } else {
        File::create(part)
    }
    .map_err(|e| e.to_string())?;
    let mut done = if resumed { have } else { 0 };
    let total = resp.content_length().map(|l| l + done).unwrap_or(0);

    let mut buf = vec![0u8; 1 << 16];
    let mut last = Instant::now();
    progress(done, total);
    loop {
        let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if last.elapsed() > Duration::from_millis(250) {
            last = Instant::now();
            progress(done, total);
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    progress(done, total);
    if total > 0 && done < total {
        return Err("connection interrupted".into());
    }
    Ok(())
}

/// ggml model files start with the magic 0x67676d6c ("lmgg" little-endian).
fn is_ggml(path: &PathBuf) -> bool {
    let mut magic = [0u8; 4];
    File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map(|_| u32::from_le_bytes(magic) == 0x6767_6d6c)
        .unwrap_or(false)
        && std::fs::metadata(path).map(|m| m.len() > 10_000_000).unwrap_or(false)
}
