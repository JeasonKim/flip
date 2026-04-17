use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;

use super::{SessionMessage, SessionMeta};

fn projects_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("projects")
}

/// 扫描所有 Claude Code 会话文件的元数据
pub fn scan_sessions() -> Vec<SessionMeta> {
    let base = projects_dir();
    if !base.exists() {
        return vec![];
    }

    let mut sessions = Vec::new();
    let Ok(project_dirs) = fs::read_dir(&base) else {
        return vec![];
    };

    for project_entry in project_dirs.flatten() {
        if !project_entry.path().is_dir() {
            continue;
        }
        let project_path = project_entry.path();
        // 解码项目目录名（- 替换回 /）
        let project_dir = decode_project_dir(project_entry.file_name().to_string_lossy().as_ref());

        let Ok(files) = fs::read_dir(&project_path) else {
            continue;
        };

        for file_entry in files.flatten() {
            let path = file_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            // 跳过 agent- 开头的子 agent 会话
            let fname = path.file_stem().unwrap_or_default().to_string_lossy();
            if fname.starts_with("agent-") {
                continue;
            }

            if let Some(meta) = extract_session_meta(&path, &project_dir, &fname) {
                sessions.push(meta);
            }
        }
    }

    sessions
}

fn extract_session_meta(
    path: &PathBuf,
    project_dir: &str,
    session_id: &str,
) -> Option<SessionMeta> {
    // 用文件 mtime 作为最后活跃时间，避免解析文件内容中的时间戳
    let mtime = super::file_mtime_millis(path);

    let file = fs::File::open(path).ok()?;
    let file_size = file.metadata().ok()?.len();

    // 读取头部（前 15 行）提取标题
    let head_lines = read_head_lines(path, 15)?;
    let mut title: Option<String> = None;

    for line in &head_lines {
        let val: serde_json::Value = serde_json::from_str(line).ok()?;

        if val.get("isMeta").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }

        if let Some(t) = extract_title_from_line(&val) {
            title = Some(t);
            break;
        }
    }

    // 读取尾部（后 30 行）提取 custom-title
    let tail_lines = read_tail_lines(path, file_size, 30);
    let mut custom_title: Option<String> = None;

    for line in &tail_lines {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        if val.get("type").and_then(|v| v.as_str()) == Some("custom-title") {
            if let Some(ct) = val.get("customTitle").and_then(|v| v.as_str()) {
                custom_title = Some(ct.to_string());
            }
        }
    }

    // 标题优先级：custom-title > 第一条 user 消息 > session ID 前 8 位
    let final_title = custom_title
        .or(title)
        .unwrap_or_else(|| session_id.chars().take(8).collect());

    Some(SessionMeta {
        agent: "claude".into(),
        resume_command: Some(format!("claude --resume {}", session_id)),
        session_id: session_id.to_string(),
        title: truncate(&final_title, 80),
        project_dir: Some(project_dir.to_string()),
        last_active_at: mtime,
        source_path: path.to_string_lossy().to_string(),
    })
}

/// 删除 mtime 早于 cutoff 的会话文件
pub fn purge_sessions(cutoff_millis: i64) -> usize {
    let base = projects_dir();
    if !base.exists() {
        return 0;
    }

    let mut count = 0;
    let Ok(project_dirs) = fs::read_dir(&base) else {
        return 0;
    };

    for project_entry in project_dirs.flatten() {
        if !project_entry.path().is_dir() {
            continue;
        }
        let Ok(files) = fs::read_dir(project_entry.path()) else {
            continue;
        };
        for file_entry in files.flatten() {
            let path = file_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
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

        // 跳过 meta 行和 custom-title 行
        if val.get("isMeta").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        if val.get("type").and_then(|v| v.as_str()) == Some("custom-title") {
            continue;
        }

        let timestamp = parse_timestamp(&val);

        // 提取 message 内容
        if let Some(msg) = val.get("message") {
            let role = msg
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let content = extract_content(msg);
            if content.trim().is_empty() {
                continue;
            }

            // tool_use / tool_result 映射到 tool role
            let final_role = if content.starts_with("[Tool") { "tool".into() } else { role };

            messages.push(SessionMessage {
                role: final_role,
                content,
                timestamp,
            });
        }
    }

    Ok(messages)
}

/// 从 message.content 提取文本
fn extract_content(msg: &serde_json::Value) -> String {
    match msg.get("content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(blocks)) => {
            let mut parts = Vec::new();
            for block in blocks {
                match block.get("type").and_then(|v| v.as_str()) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                            parts.push(text.to_string());
                        }
                    }
                    Some("tool_use") => {
                        let name = block
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let input = block
                            .get("input")
                            .map(|v| serde_json::to_string(v).unwrap_or_default())
                            .unwrap_or_default();
                        parts.push(format!("[Tool: {}] {}", name, input));
                    }
                    Some("tool_result") => {
                        let content = block
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        parts.push(format!("[Tool Result] {}", content));
                    }
                    _ => {}
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

fn extract_title_from_line(val: &serde_json::Value) -> Option<String> {
    let msg_type = val.get("type").and_then(|v| v.as_str())?;
    if msg_type != "user" {
        return None;
    }
    let msg = val.get("message")?;
    let role = msg.get("role").and_then(|v| v.as_str())?;
    if role != "user" {
        return None;
    }
    let content = extract_content(msg);
    let trimmed = content.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') {
        return None;
    }
    Some(trimmed.to_string())
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

fn decode_project_dir(encoded: &str) -> String {
    // Claude Code 把路径中的分隔符替换为 -，还原时使用当前平台的分隔符
    let sep = std::path::MAIN_SEPARATOR_STR;
    if encoded.starts_with('-') {
        // Unix 绝对路径：首个 - 对应 /
        encoded.replacen('-', sep, 1).replace('-', sep)
    } else {
        encoded.replace('-', sep)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max - 3).collect();
        format!("{}...", truncated)
    }
}

fn read_head_lines(path: &PathBuf, n: usize) -> Option<Vec<String>> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader
        .lines()
        .take(n)
        .filter_map(|l| l.ok())
        .filter(|l| !l.trim().is_empty())
        .collect();
    Some(lines)
}

fn read_tail_lines(path: &PathBuf, file_size: u64, n: usize) -> Vec<String> {
    let Ok(mut file) = fs::File::open(path) else {
        return vec![];
    };

    // 对小文件直接读全部
    let chunk_size: u64 = 32 * 1024; // 32KB
    if file_size <= chunk_size {
        let reader = BufReader::new(&file);
        let all: Vec<String> = reader
            .lines()
            .filter_map(|l| l.ok())
            .filter(|l| !l.trim().is_empty())
            .collect();
        return all.into_iter().rev().take(n).rev().collect();
    }

    // 大文件从尾部 seek 读取
    let seek_pos = file_size.saturating_sub(chunk_size);
    if file.seek(SeekFrom::Start(seek_pos)).is_err() {
        return vec![];
    }
    let reader = BufReader::new(&file);
    let mut lines: Vec<String> = reader
        .lines()
        .filter_map(|l| l.ok())
        .filter(|l| !l.trim().is_empty())
        .collect();

    // 第一行可能不完整，丢弃
    if seek_pos > 0 && !lines.is_empty() {
        lines.remove(0);
    }

    lines.into_iter().rev().take(n).rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_project_dir_converts() {
        let sep = std::path::MAIN_SEPARATOR_STR;
        let expected = format!(
            "{0}Users{0}jinjianxun{0}code{0}personal{0}flip",
            sep
        );
        assert_eq!(
            decode_project_dir("-Users-jinjianxun-code-personal-flip"),
            expected
        );
    }

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string_ellipsis() {
        let long = "a".repeat(100);
        let result = truncate(&long, 20);
        assert!(result.ends_with("..."));
        assert_eq!(result.chars().count(), 20);
    }

    #[test]
    fn extract_content_from_string() {
        let msg = serde_json::json!({"role": "user", "content": "hello world"});
        assert_eq!(extract_content(&msg), "hello world");
    }

    #[test]
    fn extract_content_from_blocks() {
        let msg = serde_json::json!({
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Let me check."},
                {"type": "tool_use", "name": "Read", "input": {"path": "src/main.rs"}}
            ]
        });
        let content = extract_content(&msg);
        assert!(content.contains("Let me check."));
        assert!(content.contains("[Tool: Read]"));
    }
}
