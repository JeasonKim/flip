pub mod claude;
pub mod codex;
pub mod opencode;

use std::io::{BufRead, BufReader};

use serde::{Deserialize, Serialize};

const RAW_RECORD_LIMIT: usize = 80;
const RAW_STRING_CHAR_LIMIT: usize = 1200;

/// 从文件 mtime 获取毫秒时间戳，用于排序和过滤
pub(crate) fn file_mtime_millis(path: &std::path::Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub agent: String,
    pub session_id: String,
    pub title: String,
    pub project_dir: Option<String>,
    /// 最后活跃时间（毫秒时间戳）
    pub last_active_at: Option<i64>,
    /// JSONL 文件的完整路径
    pub source_path: String,
    /// 恢复会话的 CLI 命令
    pub resume_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String, // user, assistant, tool, system
    pub content: String,
    pub timestamp: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRawContent {
    pub agent: String,
    pub source_path: String,
    pub records: Vec<SessionRawRecord>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRawRecord {
    /// JSONL 会话中为 jsonl；OpenCode 中为 session / message / part。
    pub section: String,
    /// JSONL 行号或表内记录序号，从 1 开始。
    pub index: usize,
    pub value: serde_json::Value,
}

/// 扫描会话列表，支持分页和 agent 筛选
pub fn scan_sessions(agent_filter: Option<&str>, offset: usize, limit: usize) -> Vec<SessionMeta> {
    let mut all = Vec::new();

    let scan_claude = agent_filter.is_none() || agent_filter == Some("claude");
    let scan_codex = agent_filter.is_none() || agent_filter == Some("codex");
    let scan_opencode = agent_filter.is_none() || agent_filter == Some("opencode");

    if scan_claude {
        all.extend(claude::scan_sessions());
    }
    if scan_codex {
        all.extend(codex::scan_sessions());
    }
    if scan_opencode {
        all.extend(opencode::scan_sessions());
    }

    // 按最后活跃时间排序（降序）
    all.sort_unstable_by(|a, b| b.last_active_at.cmp(&a.last_active_at));

    // 分页
    all.into_iter().skip(offset).take(limit).collect()
}

/// 加载单个会话的完整消息
pub fn load_messages(agent: &str, source_path: &str) -> Result<Vec<SessionMessage>, String> {
    match agent {
        "claude" => claude::parse_messages(source_path),
        "codex" => codex::parse_messages(source_path),
        "opencode" => opencode::parse_messages(source_path),
        _ => Err(format!("unknown agent: {}", agent)),
    }
}

/// 加载单个会话的原始结构视图。
pub fn load_raw_content(agent: &str, source_path: &str) -> Result<SessionRawContent, String> {
    let (records, truncated) = match agent {
        "claude" | "codex" => load_jsonl_raw_records(source_path)?,
        "opencode" => {
            opencode::load_raw_records_with_truncation(source_path, RAW_STRING_CHAR_LIMIT)?
        }
        _ => return Err(format!("unknown agent: {}", agent)),
    };

    Ok(SessionRawContent {
        agent: agent.to_string(),
        source_path: source_path.to_string(),
        records,
        truncated,
    })
}

fn load_jsonl_raw_records(source_path: &str) -> Result<(Vec<SessionRawRecord>, bool), String> {
    let file = std::fs::File::open(source_path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    let mut truncated = false;

    for (line_index, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }

        if records.len() >= RAW_RECORD_LIMIT {
            truncated = true;
            break;
        }

        let (section, value) = match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(value) => (
                "jsonl".to_string(),
                truncate_raw_value(value, RAW_STRING_CHAR_LIMIT),
            ),
            Err(err) => (
                "jsonl_parse_error".to_string(),
                serde_json::json!({
                    "error": err.to_string(),
                    "raw": truncate_raw_string(&line, RAW_STRING_CHAR_LIMIT),
                }),
            ),
        };

        records.push(SessionRawRecord {
            section,
            index: line_index + 1,
            value,
        });
    }

    Ok((records, truncated))
}

pub(crate) fn truncate_raw_value(
    value: serde_json::Value,
    max_string_chars: usize,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) => {
            serde_json::Value::String(truncate_raw_string(&text, max_string_chars))
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .into_iter()
                .map(|item| truncate_raw_value(item, max_string_chars))
                .collect(),
        ),
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key, truncate_raw_value(value, max_string_chars)))
                .collect(),
        ),
        other => other,
    }
}

fn truncate_raw_string(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let prefix: String = value.chars().take(max_chars - 3).collect();
    format!("{prefix}...")
}

/// 删除指定天数之前的会话文件
pub fn purge_sessions(older_than_days: u32) -> usize {
    let now_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let cutoff = now_millis - (older_than_days as i64) * 24 * 60 * 60 * 1000;
    claude::purge_sessions(cutoff)
        + codex::purge_sessions(cutoff)
        + opencode::purge_sessions(cutoff)
}

/// 在终端中启动恢复命令
pub fn launch_terminal(command: &str, cwd: Option<&str>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return launch_macos_terminal(command, cwd);

    #[cfg(target_os = "windows")]
    return launch_windows_terminal(command, cwd);

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (command, cwd);
        Err("terminal launch not supported on this platform".into())
    }
}

/// macOS：通过 AppleScript 在 Terminal.app 中执行命令
#[cfg(target_os = "macos")]
fn launch_macos_terminal(command: &str, cwd: Option<&str>) -> Result<(), String> {
    let full_command = build_macos_shell_command(command, cwd);
    let escaped = escape_applescript_string(&full_command);
    let script = format!(
        r#"tell application "Terminal"
    activate
    do script "{}"
end tell"#,
        escaped
    );
    let status = std::process::Command::new("osascript")
        .args(["-e", &script])
        .status()
        .map_err(|e| format!("failed to launch terminal: {}", e))?;
    if !status.success() {
        return Err("osascript exited with error".into());
    }
    Ok(())
}

/// Windows：在 cmd.exe 新窗口中执行命令
#[cfg(target_os = "windows")]
fn launch_windows_terminal(command: &str, cwd: Option<&str>) -> Result<(), String> {
    let script = build_windows_start_process_script(command, cwd);
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .spawn()
        .map_err(|e| format!("failed to launch terminal: {}", e))?;
    Ok(())
}

fn build_macos_shell_command(command: &str, cwd: Option<&str>) -> String {
    match cwd.filter(|dir| !dir.trim().is_empty()) {
        Some(dir) => format!("cd -- {} && {}", shell_quote_posix(dir), command),
        None => command.to_string(),
    }
}

fn shell_quote_posix(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'\''"#))
}

fn escape_applescript_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(any(target_os = "windows", test))]
fn build_windows_start_process_script(command: &str, cwd: Option<&str>) -> String {
    let command_arg = format!("@('/k', {})", powershell_quote(command));
    match cwd.filter(|dir| !dir.trim().is_empty()) {
        Some(dir) => format!(
            "Start-Process -FilePath 'cmd.exe' -ArgumentList {} -WorkingDirectory {}",
            command_arg,
            powershell_quote(dir)
        ),
        None => format!(
            "Start-Process -FilePath 'cmd.exe' -ArgumentList {}",
            command_arg
        ),
    }
}

#[cfg(any(target_os = "windows", test))]
fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn macos_shell_command_quotes_cwd_with_spaces_and_single_quote() {
        let command = build_macos_shell_command("codex resume abc", Some("/Users/a/it's here"));

        assert_eq!(
            command,
            r#"cd -- '/Users/a/it'\''s here' && codex resume abc"#
        );
    }

    #[test]
    fn applescript_string_escapes_backslash_and_double_quote() {
        assert_eq!(
            escape_applescript_string(r#"cd "C:\tmp""#),
            r#"cd \"C:\\tmp\""#
        );
    }

    #[test]
    fn windows_script_uses_working_directory_without_cd_concatenation() {
        let script = build_windows_start_process_script(
            "opencode -s ses_1",
            Some(r#"C:\Users\me\it's here"#),
        );

        assert!(script.contains("Start-Process -FilePath 'cmd.exe'"));
        assert!(script.contains("-ArgumentList @('/k', 'opencode -s ses_1')"));
        assert!(script.contains(r#"-WorkingDirectory 'C:\Users\me\it''s here'"#));
    }

    #[test]
    fn windows_script_omits_empty_cwd() {
        let script = build_windows_start_process_script("claude --resume s1", Some(" "));

        assert_eq!(
            script,
            "Start-Process -FilePath 'cmd.exe' -ArgumentList @('/k', 'claude --resume s1')"
        );
    }

    #[test]
    fn raw_value_truncates_nested_long_strings() {
        let value = serde_json::json!({
            "short": "keep",
            "nested": {
                "long": "abcdefghijklmnopqrstuvwxyz"
            },
            "items": ["abcdefghijklmnopqrstuvwxyz"]
        });

        let truncated = truncate_raw_value(value, 10);

        assert_eq!(truncated["short"], "keep");
        assert_eq!(truncated["nested"]["long"], "abcdefg...");
        assert_eq!(truncated["items"][0], "abcdefg...");
    }

    #[test]
    fn load_raw_content_routes_claude_jsonl_records() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("claude.jsonl");
        std::fs::write(
            &source,
            [
                r#"{"type":"user","message":{"role":"user","content":"hello"}}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":"abcdefghijklmnopqrstuvwxyz"}}"#,
            ]
            .join("\n"),
        )
        .expect("write");

        let raw = load_raw_content("claude", source.to_str().expect("path")).expect("raw");

        assert_eq!(raw.agent, "claude");
        assert_eq!(raw.records.len(), 2);
        assert_eq!(raw.records[0].section, "jsonl");
        assert_eq!(raw.records[0].index, 1);
        assert_eq!(raw.records[0].value["type"], "user");
        assert_eq!(
            raw.records[1].value["message"]["content"],
            "abcdefghijklmnopqrstuvwxyz"
        );
        assert!(!raw.truncated);
    }

    #[test]
    fn load_raw_content_routes_codex_jsonl_records_and_keeps_parse_errors_visible() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("codex.jsonl");
        std::fs::write(
            &source,
            [
                r#"{"type":"session_meta","payload":{"id":"s1"}}"#,
                "not-json",
            ]
            .join("\n"),
        )
        .expect("write");

        let raw = load_raw_content("codex", source.to_str().expect("path")).expect("raw");

        assert_eq!(raw.agent, "codex");
        assert_eq!(raw.records.len(), 2);
        assert_eq!(raw.records[0].value["type"], "session_meta");
        assert_eq!(raw.records[1].section, "jsonl_parse_error");
        assert_eq!(raw.records[1].value["raw"], "not-json");
    }
}
