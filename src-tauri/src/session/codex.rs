use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use super::{SessionMessage, SessionMeta};

fn sessions_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    vec![
        home.join(".codex").join("sessions"),
        home.join(".codex").join("archived_sessions"),
    ]
}

/// 扫描所有 Codex 会话文件的元数据
pub fn scan_sessions() -> Vec<SessionMeta> {
    let mut sessions = Vec::new();

    for dir in sessions_dirs() {
        if !dir.exists() {
            continue;
        }
        collect_jsonl_files(&dir, &mut sessions, 0);
    }

    sessions
}

/// 递归扫描目录下的 .jsonl 文件（sessions 使用 YYYY/MM/DD 三级目录）
fn collect_jsonl_files(dir: &PathBuf, sessions: &mut Vec<SessionMeta>, depth: usize) {
    const MAX_DEPTH: usize = 4;
    if depth > MAX_DEPTH {
        return;
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, sessions, depth + 1);
            continue;
        }

        let fname = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        if !fname.starts_with("rollout-") || !fname.ends_with(".jsonl") {
            continue;
        }

        if let Some(meta) = extract_session_meta(&path, &fname) {
            sessions.push(meta);
        }
    }
}

fn extract_session_meta(path: &PathBuf, fname: &str) -> Option<SessionMeta> {
    // 用文件 mtime 作为最后活跃时间，避免逐行扫描整个文件
    let mtime = super::file_mtime_millis(path);
    let lines = read_head_lines(path, 20)?;

    let mut session_id: Option<String> = None;
    let mut project_dir: Option<String> = None;
    let mut title: Option<String> = None;

    for line in &lines {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        let line_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

        if line_type == "session_meta" {
            if let Some(payload) = val.get("payload") {
                session_id = payload
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                project_dir = payload
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }
        }

        if title.is_none() && line_type == "response_item" {
            if let Some(payload) = val.get("payload") {
                let payload_type = payload.get("type").and_then(|v| v.as_str());
                let role = payload.get("role").and_then(|v| v.as_str());
                if payload_type == Some("message") && role == Some("user") {
                    let text = extract_content_text(payload.get("content"));
                    let trimmed = text.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with("# AGENTS.md") {
                        title = Some(truncate(trimmed, 80));
                    }
                }
            }
        }
    }

    let session_id = session_id.unwrap_or_else(|| extract_uuid_from_filename(fname));

    let final_title = title.unwrap_or_else(|| {
        project_dir
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name())
            .and_then(|n| n.to_str())
            .unwrap_or(&session_id[..8.min(session_id.len())])
            .to_string()
    });

    Some(SessionMeta {
        agent: "codex".into(),
        resume_command: Some(format!("codex resume {}", session_id)),
        session_id,
        title: final_title,
        project_dir,
        last_active_at: mtime,
        source_path: path.to_string_lossy().to_string(),
    })
}

/// 删除 mtime 早于 cutoff 的会话文件，递归清理空目录
pub fn purge_sessions(cutoff_millis: i64) -> usize {
    let mut count = 0;
    for dir in sessions_dirs() {
        if !dir.exists() {
            continue;
        }
        count += purge_dir_recursive(&dir, cutoff_millis, 0);
    }
    count
}

fn purge_dir_recursive(dir: &PathBuf, cutoff_millis: i64, depth: usize) -> usize {
    const MAX_DEPTH: usize = 4;
    if depth > MAX_DEPTH {
        return 0;
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };

    let mut count = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            count += purge_dir_recursive(&path, cutoff_millis, depth + 1);
            // 清理空目录
            if fs::read_dir(&path)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false)
            {
                let _ = fs::remove_dir(&path);
            }
            continue;
        }

        let fname = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if !fname.starts_with("rollout-") || !fname.ends_with(".jsonl") {
            continue;
        }

        if let Some(mtime) = super::file_mtime_millis(&path) {
            if mtime < cutoff_millis {
                if fs::remove_file(&path).is_ok() {
                    count += 1;
                }
            }
        }
    }

    count
}

/// 解析完整消息列表
pub fn parse_messages(source_path: &str) -> Result<Vec<SessionMessage>, String> {
    let file = fs::File::open(source_path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    let mut messages = Vec::new();

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };

        let timestamp = parse_timestamp(&val);
        let line_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match line_type {
            "response_item" => {
                let Some(payload) = val.get("payload") else {
                    continue;
                };

                let payload_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");

                match payload_type {
                    "message" => {
                        let role = payload
                            .get("role")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let content = extract_content_text(payload.get("content"));
                        append_codex_message(&mut messages, role, content, timestamp);
                    }
                    "function_call" => {
                        let name = payload
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let args = payload
                            .get("arguments")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        append_codex_message(
                            &mut messages,
                            "assistant".into(),
                            format!("[Tool: {}] {}", name, args),
                            timestamp,
                        );
                    }
                    "function_call_output" => {
                        let output = payload.get("output").and_then(|v| v.as_str()).unwrap_or("");
                        append_codex_message(
                            &mut messages,
                            "tool".into(),
                            output.to_string(),
                            timestamp,
                        );
                    }
                    _ => {}
                }
            }
            "event_msg" => {
                let Some(payload) = val.get("payload") else {
                    continue;
                };
                if payload.get("type").and_then(|v| v.as_str()) == Some("agent_message") {
                    let content = payload
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    append_codex_message(&mut messages, "assistant".into(), content, timestamp);
                }
            }
            _ => {}
        }
    }

    Ok(messages)
}

fn append_codex_message(
    messages: &mut Vec<SessionMessage>,
    role: String,
    content: String,
    timestamp: Option<i64>,
) {
    if content.trim().is_empty() {
        return;
    }

    if messages
        .last()
        .is_some_and(|last| is_duplicate_codex_message(last, &role, &content, timestamp))
    {
        return;
    }

    messages.push(SessionMessage {
        role,
        content,
        timestamp,
    });
}

fn is_duplicate_codex_message(
    last: &SessionMessage,
    role: &str,
    content: &str,
    timestamp: Option<i64>,
) -> bool {
    if last.role != role || last.content != content {
        return false;
    }

    match (last.timestamp, timestamp) {
        (Some(left), Some(right)) => (left - right).abs() <= 1000,
        (None, None) => true,
        _ => false,
    }
}

/// 从 content 字段提取文本，支持三种格式：
/// - 字符串: "hello"
/// - 数组: [{"type":"input_text","text":"..."}, {"type":"output_text","text":"..."}]
/// - 其他: 跳过
fn extract_content_text(content: Option<&serde_json::Value>) -> String {
    let Some(val) = content else {
        return String::new();
    };
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            let mut parts = Vec::new();
            for item in arr {
                // 提取 text / input_text / output_text 字段
                let text = item
                    .get("text")
                    .or_else(|| item.get("input_text"))
                    .or_else(|| item.get("output_text"))
                    .and_then(|v| v.as_str());
                if let Some(t) = text {
                    if !t.is_empty() {
                        parts.push(t.to_string());
                    }
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

fn extract_uuid_from_filename(fname: &str) -> String {
    // 从 rollout-2026-03-31T12-58-23-<uuid>.jsonl 中提取 UUID
    let re_pattern = |s: &str| -> Option<String> {
        // 简单匹配：找最后一个看起来像 UUID 的段
        let parts: Vec<&str> = s.trim_end_matches(".jsonl").split('-').collect();
        if parts.len() >= 5 {
            // UUID 的最后 5 段
            let uuid_candidate = parts[parts.len() - 5..].join("-");
            if uuid_candidate.len() >= 32 {
                return Some(uuid_candidate);
            }
        }
        None
    };

    re_pattern(fname).unwrap_or_else(|| fname.trim_end_matches(".jsonl").to_string())
}

fn parse_timestamp(val: &serde_json::Value) -> Option<i64> {
    val.get("timestamp").and_then(|ts| match ts {
        serde_json::Value::String(s) => chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.timestamp_millis()),
        serde_json::Value::Number(n) => n.as_i64(),
        _ => None,
    })
}

fn read_head_lines(path: &PathBuf, n: usize) -> Option<Vec<String>> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    Some(
        reader
            .lines()
            .take(n)
            .filter_map(|l| l.ok())
            .filter(|l| !l.trim().is_empty())
            .collect(),
    )
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max - 3).collect();
        format!("{}...", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn extract_uuid_from_rollout_filename() {
        let uuid = extract_uuid_from_filename(
            "rollout-2026-03-31T12-58-23-019d4241-d505-7d02-b5be-2af94d1313eb.jsonl",
        );
        assert_eq!(uuid, "019d4241-d505-7d02-b5be-2af94d1313eb");
    }

    #[test]
    fn parse_messages_uses_agent_message_event_when_response_item_is_missing() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("codex.jsonl");
        std::fs::write(
            &source,
            [
                r#"{"timestamp":"2026-05-12T15:01:17.006Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Reply with OK only."}]}}"#,
                r#"{"timestamp":"2026-05-12T15:01:20.484Z","type":"event_msg","payload":{"type":"agent_message","message":"OK","phase":"final_answer","memory_citation":null}}"#,
            ]
            .join("\n"),
        )
        .expect("write");

        let messages = parse_messages(source.to_str().expect("path")).expect("parse");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Reply with OK only.");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "OK");
    }

    #[test]
    fn parse_messages_deduplicates_agent_message_event_and_response_item_pair() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("codex.jsonl");
        std::fs::write(
            &source,
            [
                r#"{"timestamp":"2026-05-12T10:46:08.364Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Reply with OK only."}]}}"#,
                r#"{"timestamp":"2026-05-12T10:46:09.447Z","type":"event_msg","payload":{"type":"agent_message","message":"OK","phase":"final_answer","memory_citation":null}}"#,
                r#"{"timestamp":"2026-05-12T10:46:09.448Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"OK"}],"phase":"final_answer"}}"#,
            ]
            .join("\n"),
        )
        .expect("write");

        let messages = parse_messages(source.to_str().expect("path")).expect("parse");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "OK");
    }
}
