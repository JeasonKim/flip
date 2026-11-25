use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;

use super::{
    describe_embedded_data, represent_timeline_entry, summarize_timeline_text,
    summarize_timeline_value, SessionMessage, SessionMeta, SessionRawRecord,
    SessionTimelineCategory, SessionTimelineEntry, SessionTimelineImportance, SessionTokenUsage,
    TimelineRecordSemantics, TimelineUsageSample,
};

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

            if let Some(meta) = super::cached_file_session_meta("claude", &path, || {
                extract_session_meta(&path, &project_dir, &fname)
            }) {
                sessions.push(meta);
            }
        }
    }

    sessions
}

fn extract_session_meta(
    path: &PathBuf,
    fallback_project_dir: &str,
    session_id: &str,
) -> Option<SessionMeta> {
    // 用文件 mtime 作为最后活跃时间，避免解析文件内容中的时间戳
    let mtime = super::file_mtime_millis(path);

    let file = fs::File::open(path).ok()?;
    let file_size = file.metadata().ok()?.len();

    // 读取头部提取 cwd（真实项目路径）和标题
    let head_lines = read_head_lines(path, 15)?;
    let mut title: Option<String> = None;
    let mut cwd: Option<String> = None;

    for line in &head_lines {
        let val: serde_json::Value = serde_json::from_str(line).ok()?;

        // 从任意行提取 cwd — meta 行和消息行都可能带有此字段
        if cwd.is_none() {
            cwd = val
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }

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

    // 项目路径优先用 JSONL 中的 cwd（原始路径），fallback 到目录名解码
    let project_dir = cwd.unwrap_or_else(|| fallback_project_dir.to_string());

    Some(SessionMeta {
        agent: "claude".into(),
        source: None,
        resume_command: Some(format!("claude --resume {}", session_id)),
        session_id: session_id.to_string(),
        title: truncate(&final_title, 80),
        project_dir: Some(project_dir),
        last_active_at: mtime,
        source_path: path.to_string_lossy().to_string(),
        source_file_path: Some(path.to_string_lossy().to_string()),
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
            let final_role = if content.starts_with("[Tool") {
                "tool".into()
            } else {
                role
            };

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
                        let content = block.get("content").and_then(|v| v.as_str()).unwrap_or("");
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

pub(crate) fn represent_timeline_record(record: &SessionRawRecord) -> SessionTimelineEntry {
    let value = &record.value;
    let timestamp = parse_timestamp(value);

    if record.section == "jsonl_parse_error" {
        return represent_timeline_entry(
            record,
            SessionTimelineCategory::Unknown,
            SessionTimelineImportance::Debug,
            "JSONL 解析失败".into(),
            "system".into(),
            summarize_timeline_value(value, 900),
            timestamp,
        );
    }

    if let Some(message) = value.get("message") {
        let preview = represent_claude_message(message);
        return represent_timeline_entry(
            record,
            preview.category,
            preview.importance,
            preview.title,
            preview.role,
            preview.content,
            timestamp,
        );
    }

    let record_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let subtype = value.get("subtype").and_then(serde_json::Value::as_str);
    let title = subtype
        .map(|subtype| format!("{record_type} · {subtype}"))
        .unwrap_or_else(|| record_type.to_string());
    let category = match (record_type, subtype) {
        ("system", Some("init")) => SessionTimelineCategory::Metadata,
        ("system", _) => SessionTimelineCategory::System,
        ("custom-title" | "queue-operation" | "file-history-snapshot", _) => {
            SessionTimelineCategory::Metadata
        }
        ("unknown", _) => SessionTimelineCategory::Unknown,
        _ => SessionTimelineCategory::Event,
    };

    represent_timeline_entry(
        record,
        category,
        claude_record_importance(value, record_type, subtype),
        title,
        "system".into(),
        summarize_timeline_value(value, 900),
        timestamp,
    )
}

pub(crate) fn interpret_timeline_record(record: &SessionRawRecord) -> TimelineRecordSemantics {
    let value = &record.value;
    let message = value.get("message");
    let role = message
        .and_then(|message| message.get("role"))
        .and_then(serde_json::Value::as_str);

    let starts_turn = value.get("type").and_then(serde_json::Value::as_str) == Some("user")
        && role == Some("user")
        && !is_claude_tool_result_continuation(value);
    let stop_reason = message
        .and_then(|message| message.get("stop_reason"))
        .and_then(serde_json::Value::as_str);
    let completes_turn = role == Some("assistant")
        && stop_reason.is_some_and(|reason| !matches!(reason, "tool_use" | "pause_turn"));
    let usage_sample = (role == Some("assistant"))
        .then(|| message.and_then(|message| message.get("usage")))
        .flatten()
        .map(|usage| TimelineUsageSample {
            deduplication_key: message
                .and_then(|message| message.get("id"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("claude-record-{}", record.index)),
            usage: SessionTokenUsage {
                total_tokens: claude_total_tokens(usage),
                // Claude 的 input_tokens 已经只含未命中缓存的新增输入，直接对应统一口径
                input_tokens: usage.get("input_tokens").and_then(token_count),
                output_tokens: usage.get("output_tokens").and_then(token_count),
                thinking_tokens: usage
                    .get("output_tokens_details")
                    .and_then(|details| details.get("thinking_tokens"))
                    .and_then(token_count),
                cache_read_tokens: usage.get("cache_read_input_tokens").and_then(token_count),
                cache_write_tokens: usage
                    .get("cache_creation_input_tokens")
                    .and_then(token_count),
                cache_write_1h_tokens: usage
                    .get("cache_creation")
                    .and_then(|creation| creation.get("ephemeral_1h_input_tokens"))
                    .and_then(token_count),
                cache_write_5m_tokens: usage
                    .get("cache_creation")
                    .and_then(|creation| creation.get("ephemeral_5m_input_tokens"))
                    .and_then(token_count),
            },
        });

    TimelineRecordSemantics {
        starts_turn,
        completes_turn,
        usage_sample,
    }
}

fn claude_total_tokens(usage: &serde_json::Value) -> Option<u64> {
    // Thinking 已含在输出里，缓存时长明细已含在写入里；缺少任一主项时保留未知。
    [
        "input_tokens",
        "output_tokens",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
    ]
    .into_iter()
    .try_fold(0_u64, |total, field| {
        Some(total.saturating_add(token_count(usage.get(field)?)?))
    })
}

fn is_claude_tool_result_continuation(value: &serde_json::Value) -> bool {
    if value
        .get("toolUseResult")
        .is_some_and(|result| !result.is_null())
    {
        return true;
    }

    value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|blocks| {
            blocks.iter().any(|block| {
                block.get("type").and_then(serde_json::Value::as_str) == Some("tool_result")
            })
        })
}

fn token_count(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|count| u64::try_from(count).ok()))
}

struct ClaudeTimelinePreview {
    category: SessionTimelineCategory,
    importance: SessionTimelineImportance,
    title: String,
    role: String,
    content: String,
}

fn represent_claude_message(message: &serde_json::Value) -> ClaudeTimelinePreview {
    let original_role = message
        .get("role")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let mut block_types = Vec::new();
    let mut tool_name: Option<String> = None;
    let content = match message.get("content") {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .map(|block| {
                let block_type = block
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");
                block_types.push(block_type.to_string());
                if block_type == "tool_use" {
                    tool_name = block
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned);
                }
                represent_claude_content_block(block, block_type)
            })
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        Some(other) => summarize_timeline_value(other, 1_200),
        None => "[消息没有 content 字段]".into(),
    };

    let has_tool_result = block_types.iter().any(|kind| kind == "tool_result");
    let has_tool_call = block_types.iter().any(|kind| kind == "tool_use");
    let has_reasoning = block_types
        .iter()
        .any(|kind| kind == "thinking" || kind == "reasoning");

    if has_tool_result {
        return ClaudeTimelinePreview {
            category: SessionTimelineCategory::ToolResult,
            importance: SessionTimelineImportance::Detail,
            title: "Tool Result".into(),
            role: "tool".into(),
            content,
        };
    }
    if has_tool_call {
        return ClaudeTimelinePreview {
            category: SessionTimelineCategory::ToolCall,
            importance: SessionTimelineImportance::Detail,
            title: tool_name
                .map(|name| format!("Tool · {name}"))
                .unwrap_or_else(|| "Tool Call".into()),
            role: "tool".into(),
            content,
        };
    }
    if has_reasoning && !block_types.iter().any(|kind| kind == "text") {
        return ClaudeTimelinePreview {
            category: SessionTimelineCategory::Reasoning,
            importance: SessionTimelineImportance::Detail,
            title: "Reasoning".into(),
            role: "assistant".into(),
            content,
        };
    }

    let (category, importance, title, role) = match original_role {
        "user" => (
            SessionTimelineCategory::User,
            SessionTimelineImportance::Primary,
            "User",
            "user",
        ),
        "assistant" => (
            SessionTimelineCategory::Assistant,
            SessionTimelineImportance::Primary,
            "Assistant",
            "assistant",
        ),
        "system" => (
            SessionTimelineCategory::System,
            SessionTimelineImportance::Debug,
            "System",
            "system",
        ),
        _ => (
            SessionTimelineCategory::Unknown,
            SessionTimelineImportance::Debug,
            "Unknown Message",
            "system",
        ),
    };

    ClaudeTimelinePreview {
        category,
        importance,
        title: title.into(),
        role: role.into(),
        content,
    }
}

fn claude_record_importance(
    value: &serde_json::Value,
    record_type: &str,
    subtype: Option<&str>,
) -> SessionTimelineImportance {
    let describes_problem = [Some(record_type), subtype]
        .into_iter()
        .flatten()
        .any(|label| {
            let label = label.to_ascii_lowercase();
            label.contains("error") || label.contains("warning") || label.contains("abort")
        });
    let reports_error = value
        .get("is_error")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
        || value.get("error").is_some_and(|error| !error.is_null());

    if describes_problem || reports_error {
        SessionTimelineImportance::Primary
    } else {
        SessionTimelineImportance::Debug
    }
}

fn represent_claude_content_block(block: &serde_json::Value, block_type: &str) -> String {
    match block_type {
        "text" => block
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("[空文本块]")
            .to_string(),
        "thinking" | "reasoning" => block
            .get("thinking")
            .or_else(|| block.get("text"))
            .and_then(serde_json::Value::as_str)
            .map(|text| format!("[Reasoning]\n{text}"))
            .unwrap_or_else(|| format!("[{block_type}]")),
        "tool_use" => {
            let name = block
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            let input = block
                .get("input")
                .map(|value| summarize_timeline_value(value, 900))
                .unwrap_or_else(|| "{}".into());
            format!("工具: {name}\n输入:\n{input}")
        }
        "tool_result" => {
            let content = block
                .get("content")
                .map(|value| match value {
                    serde_json::Value::String(text) => summarize_timeline_text(text, 1_200),
                    other => summarize_timeline_value(other, 1_200),
                })
                .unwrap_or_else(|| "[工具没有返回内容]".into());
            let status = if block
                .get("is_error")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                "失败"
            } else {
                "完成"
            };
            format!("状态: {status}\n结果:\n{content}")
        }
        "image" | "document" => describe_embedded_data(block)
            .unwrap_or_else(|| format!("[{block_type} 数据，点击“原始数据”查看]")),
        _ => format!("[{block_type}]\n{}", summarize_timeline_value(block, 700)),
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
    fn usage_total_excludes_thinking_and_cache_duration_subtotals() {
        let record = SessionRawRecord {
            section: "jsonl".into(),
            index: 1,
            value: serde_json::json!({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "usage": {
                        "input_tokens": 6,
                        "output_tokens": 877,
                        "output_tokens_details": { "thinking_tokens": 100 },
                        "cache_read_input_tokens": 40,
                        "cache_creation_input_tokens": 23645,
                        "cache_creation": {
                            "ephemeral_1h_input_tokens": 23000,
                            "ephemeral_5m_input_tokens": 645
                        }
                    }
                }
            }),
        };

        let sample = interpret_timeline_record(&record)
            .usage_sample
            .expect("usage");

        assert_eq!(sample.usage.total_tokens, Some(24_568));
        assert_eq!(sample.usage.thinking_tokens, Some(100));
        assert_eq!(sample.usage.cache_write_1h_tokens, Some(23_000));
        assert_eq!(sample.usage.cache_write_5m_tokens, Some(645));
    }

    #[test]
    fn usage_total_distinguishes_missing_counts_from_explicit_zero() {
        for (usage, expected) in [
            (serde_json::json!({}), None),
            (
                serde_json::json!({"input_tokens": 0, "output_tokens": 0}),
                None,
            ),
            (
                serde_json::json!({"input_tokens": 0, "output_tokens": 0, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}),
                Some(0),
            ),
            (
                serde_json::json!({"input_tokens": -1, "output_tokens": 0, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}),
                None,
            ),
        ] {
            let record = SessionRawRecord {
                section: "jsonl".into(),
                index: 1,
                value: serde_json::json!({
                    "type": "assistant",
                    "message": { "role": "assistant", "usage": usage }
                }),
            };

            let sample = interpret_timeline_record(&record)
                .usage_sample
                .expect("usage");
            assert_eq!(sample.usage.total_tokens, expected);
        }
    }

    #[test]
    fn decode_project_dir_converts() {
        let sep = std::path::MAIN_SEPARATOR_STR;
        let expected = format!("{0}Users{0}jinjianxun{0}code{0}personal{0}flip", sep);
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
