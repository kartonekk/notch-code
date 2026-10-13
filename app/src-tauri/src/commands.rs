//! Slash commands for the chat bar ("/review", "/caveman-commit", …): what
//! Claude Code would offer in that folder. `claude -p` expands them itself, so
//! the app only needs the names and descriptions for the menu.
//!
//! Found on disk: custom commands (`.claude/commands/*.md`) and skills
//! (`.claude/skills/*/SKILL.md`), in the project and in `~/.claude`, plus those
//! of enabled plugins. Names Claude reported in its last `init` event that
//! aren't on disk (built-ins, MCP prompts) are added without a description.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
pub struct SlashCommand {
    /// Without the slash; plugin ones as "plugin:name".
    pub name: String,
    pub description: Option<String>,
    /// "[--share|--all]": what goes after the name.
    pub hint: Option<String>,
    /// "project" | "user" | "built-in" | the plugin's name | "claude"
    pub source: String,
}

/// Built-ins that work in a headless run.
const BUILT_IN: [(&str, &str, Option<&str>); 4] = [
    ("compact", "Summarize the conversation so far to free up context", Some("[instructions]")),
    ("init", "Write a CLAUDE.md describing this project", None),
    ("review", "Review a pull request", Some("[PR number]")),
    ("security-review", "Security review of the pending changes", None),
];

/// `key: value` pairs from a Markdown file's YAML front matter (`>` / `|` blocks folded into one line).
fn front_matter(text: &str) -> Vec<(String, String)> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Vec::new();
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        match line.split_once(':') {
            Some((key, value)) if !indented && !key.trim().is_empty() => {
                let value = value.trim();
                let value = if [">", ">-", "|", "|-"].contains(&value) { "" } else { value };
                out.push((key.trim().to_string(), value.to_string()));
            }
            _ if indented => {
                // continuation of a block value
                if let Some((_, value)) = out.last_mut() {
                    if !value.is_empty() {
                        value.push(' ');
                    }
                    value.push_str(line.trim());
                }
            }
            _ => {}
        }
    }
    for (_, value) in &mut out {
        let v = value.trim();
        let unquoted = v
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(v);
        *value = unquoted.to_string();
    }
    out
}

fn get<'a>(meta: &'a [(String, String)], key: &str) -> Option<&'a str> {
    meta.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
}

fn command(name: String, meta: &[(String, String)], source: &str) -> SlashCommand {
    SlashCommand {
        name,
        description: get(meta, "description").map(str::to_string),
        hint: get(meta, "argument-hint").map(str::to_string),
        source: source.to_string(),
    }
}

/// `commands/**/*.md`: the file name is the command.
fn scan_commands(dir: &Path, prefix: &str, source: &str, out: &mut Vec<SlashCommand>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_commands(&path, prefix, source, out);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
            let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
            let meta = std::fs::read_to_string(&path).map(|t| front_matter(&t)).unwrap_or_default();
            out.push(command(format!("{prefix}{stem}"), &meta, source));
        }
    }
}

/// `skills/*/SKILL.md`: skills can be called by name too, unless they opt out.
fn scan_skills(dir: &Path, prefix: &str, source: &str, out: &mut Vec<SlashCommand>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path().join("SKILL.md")) else { continue };
        let meta = front_matter(&text);
        if get(&meta, "user-invocable") == Some("false") {
            continue;
        }
        let name = get(&meta, "name").map(str::to_string).unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned());
        out.push(command(format!("{prefix}{name}"), &meta, source));
    }
}

/// (plugin name, install folder) of every enabled plugin.
fn plugins(home: &Path) -> Vec<(String, PathBuf)> {
    let read = |p: PathBuf| std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let Some(installed) = read(home.join("plugins").join("installed_plugins.json")) else { return Vec::new() };
    let enabled = read(home.join("settings.json")).and_then(|s| s.get("enabledPlugins").cloned()).unwrap_or(Value::Null);
    let mut out = Vec::new();
    for (key, installs) in installed.get("plugins").and_then(Value::as_object).into_iter().flatten() {
        if enabled.get(key).and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let name = key.split('@').next().unwrap_or(key).to_string();
        let path = installs.as_array().and_then(|a| a.last()).and_then(|i| i.get("installPath")).and_then(Value::as_str);
        if let Some(path) = path {
            out.push((name, PathBuf::from(path)));
        }
    }
    out
}

/// Everything "/" can run in `project`; `reported` = names from Claude's last init event.
pub fn list(project: &str, reported: &[String]) -> Vec<SlashCommand> {
    let mut found = Vec::new();
    let project = Path::new(project).join(".claude");
    scan_commands(&project.join("commands"), "", "project", &mut found);
    scan_skills(&project.join("skills"), "", "project", &mut found);
    if let Some(home) = dirs::home_dir().map(|h| h.join(".claude")) {
        scan_commands(&home.join("commands"), "", "user", &mut found);
        scan_skills(&home.join("skills"), "", "user", &mut found);
        for (plugin, dir) in plugins(&home) {
            let prefix = format!("{plugin}:");
            scan_commands(&dir.join("commands"), &prefix, &plugin, &mut found);
            scan_skills(&dir.join("skills"), &prefix, &plugin, &mut found);
        }
    }
    for (name, description, hint) in BUILT_IN {
        found.push(SlashCommand {
            name: name.into(),
            description: Some(description.into()),
            hint: hint.map(str::to_string),
            source: "built-in".into(),
        });
    }
    for name in reported {
        found.push(SlashCommand { name: name.clone(), description: None, hint: None, source: "claude".into() });
    }
    // the first one of a name wins (project over user, a command over a skill)
    let mut seen = HashSet::new();
    found.retain(|c| seen.insert(c.name.to_ascii_lowercase()));
    found.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_front_matter() {
        let meta = front_matter("---\nname: x\ndescription: >\n  Folded\n  text.\nargument-hint: \"[--a|--b]\"\n---\nbody: no\n");
        assert_eq!(get(&meta, "name"), Some("x"));
        assert_eq!(get(&meta, "description"), Some("Folded text."));
        assert_eq!(get(&meta, "argument-hint"), Some("[--a|--b]"));
        assert_eq!(get(&meta, "body"), None);
        assert!(front_matter("no front matter").is_empty());
    }
}
