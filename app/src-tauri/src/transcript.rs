//! Markdown transcript file in Documents\Transkrypty, appended as segments finish.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use chrono::{DateTime, Local};

use crate::paths;
use crate::transcriber::Segment;

pub struct TranscriptFile {
    pub path: PathBuf,
    started: DateTime<Local>,
    /// None once the file was closed by `finish`.
    out: Option<BufWriter<File>>,
}

fn safe_file_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_.,()".contains(c) { c } else { '_' })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').trim();
    trimmed.chars().take(80).collect()
}

fn hms(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

impl TranscriptFile {
    pub fn create(source: &str) -> Result<Self, String> {
        let started = Local::now();
        let path = paths::transcripts_dir().join(format!(
            "{} {}.md",
            started.format("%Y-%m-%d %H-%M"),
            safe_file_name(source)
        ));
        let file = File::create(&path).map_err(|e| format!("Nie można utworzyć pliku transkryptu: {e}"))?;
        let mut out = BufWriter::new(file);
        writeln!(out, "# Transkrypt — {source}\n").ok();
        writeln!(out, "Rozpoczęto: {}\n", started.format("%d.%m.%Y %H:%M")).ok();
        out.flush().ok();
        Ok(Self { path, started, out: Some(out) })
    }

    fn write(&mut self, text: &str) {
        if let Some(out) = self.out.as_mut() {
            let _ = out.write_all(text.as_bytes());
            let _ = out.flush();
        }
    }

    pub fn append(&mut self, seg: &Segment) {
        let line = format!("**[{}] {}:** {}\n\n", hms(seg.start_ms), seg.speaker.label(), seg.text);
        self.write(&line);
    }

    /// A note such as a pause, e.g. "*[00:12:03] — pauza —*".
    pub fn mark(&mut self, what: &str, at_ms: u64) {
        let line = format!("*[{}] — {what} —*\n\n", hms(at_ms));
        self.write(&line);
    }

    pub fn finish(&mut self) {
        let line = format!("---\nZakończono: {}\n", Local::now().format("%d.%m.%Y %H:%M"));
        self.write(&line);
        self.out = None; // close the handle so the file can be renamed
    }

    /// Renames the (finished) file to "<date> <subject>.md" and puts the
    /// subject in the heading. Returns the new path.
    pub fn rename_to_subject(&mut self, subject: &str, source: &str) -> Result<PathBuf, String> {
        let name = safe_file_name(subject);
        if name.is_empty() {
            return Ok(self.path.clone());
        }
        let dir = self.path.parent().map(PathBuf::from).unwrap_or_else(paths::transcripts_dir);
        let stem = format!("{} {}", self.started.format("%Y-%m-%d %H-%M"), name);
        let mut target = dir.join(format!("{stem}.md"));
        let mut n = 2;
        while target.exists() && target != self.path {
            target = dir.join(format!("{stem} ({n}).md"));
            n += 1;
        }

        let content = std::fs::read_to_string(&self.path).map_err(|e| e.to_string())?;
        let body = content.split_once('\n').map(|(_, rest)| rest).unwrap_or("");
        let content = format!("# {}\n\nŹródło: {source}\n{body}", subject.trim());
        std::fs::write(&target, content).map_err(|e| e.to_string())?;
        if target != self.path {
            std::fs::remove_file(&self.path).map_err(|e| e.to_string())?;
        }
        self.path = target.clone();
        Ok(target)
    }
}
