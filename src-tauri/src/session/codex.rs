use std::fs;
use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use super::{
    extract_content_text,
    describe_embedded_data, represent_timeline_entry, summarize_timeline_text,
    summarize_timeline_value, SessionMessage, SessionMeta, SessionRawRecord,
    SessionTimelineCategory, SessionTimelineEntry, SessionTimelineImportance, SessionTokenUsage,
    TimelineRecordSemantics, TimelineUsageSample,
};

fn sessions_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    vec![
        home.join(".codex").join("sessions"),
        home.join(".codex").join("archived_sessions"),
    ]
}

/// 扫描所有 Codex 会话文件的元数据
pub fn scan_sessions() -> Result<Vec<SessionMeta>, String> {
    Ok(scan_source_sessions(&super::sources::configured_sources()?))
}

fn scan_source_sessions(sources: &[super::sources::SessionSource]) -> Vec<SessionMeta> {
    let mut sessions = Vec::new();
    let mut visited = HashSet::new();
    for source in sources.iter().filter(|source| source.config.enabled) {
        let start = sessions.len();
        for name in ["sessions", "archived_sessions"] {
            let dir = PathBuf::from(&source.config.path).join(name);
            if dir.exists() {
                collect_jsonl_files(&dir, &mut sessions, &mut visited, 0);
            }
        }
        // 来源信息在文件缓存之外附加，标签改名即时生效。
        for session in &mut sessions[start..] {
            session.source = Some(super::sources::SessionSourceRef {
                id: source.config.id.clone(),
                label: source.config.label.clone(),
            });
            if !source.is_default {
                session.resume_command = Some(codex_resume_command(
                    &source.config.path,
                    &session.session_id,
                ));
            }
        }
    }

    sessions
}

fn codex_resume_command(home: &str, session_id: &str) -> String {
    #[cfg(target_os = "windows")]
    return windows_codex_resume_command(home, session_id);
    #[cfg(not(target_os = "windows"))]
    format!(
        "env CODEX_HOME={} codex resume {}",
        super::shell_quote_posix(home),
        super::shell_quote_posix(session_id)
    )
}

#[cfg(any(target_os = "windows", test))]
fn windows_codex_resume_command(home: &str, session_id: &str) -> String {
    use base64::Engine;
    let script = format!(
        "$env:CODEX_HOME = {}; & codex resume {}",
        super::powershell_quote(home),
        super::powershell_quote(session_id)
    );
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell -NoProfile -EncodedCommand {}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// 递归扫描目录下的 .jsonl 文件（sessions 使用 YYYY/MM/DD 三级目录）
fn collect_jsonl_files(
    dir: &PathBuf,
    sessions: &mut Vec<SessionMeta>,
    visited: &mut HashSet<PathBuf>,
    depth: usize,
) {
    const MAX_DEPTH: usize = 4;
    if depth > MAX_DEPTH {
        return;
    }

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            log::warn!(
                "[session-sources] scan failed path={}: {}",
                dir.display(),
                error
            );
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, sessions, visited, depth + 1);
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

        let canonical = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if !visited.insert(canonical.clone()) {
            continue;
        }

        if let Some(meta) = super::cached_file_session_meta("codex", &canonical, || {
            extract_session_meta(&canonical, &fname)
        }) {
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
    let mut saw_session_meta = false;

    for line in &lines {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        let line_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

        if line_type == "session_meta" && !saw_session_meta {
            saw_session_meta = true;
            if let Some(payload) = val.get("payload") {
                session_id = payload
                    .get("id")
                    .or_else(|| payload.get("session_id"))
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
        source: None,
        project_dir,
        last_active_at: mtime,
        source_path: path.to_string_lossy().to_string(),
        source_file_path: Some(path.to_string_lossy().to_string()),
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

    let line_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let payload = value.get("payload").unwrap_or(value);
    let payload_type = payload
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(line_type);
    let preview = match line_type {
        "response_item" => represent_codex_response_item(payload, payload_type),
        "event_msg" => represent_codex_event(payload, payload_type),
        "session_meta" | "turn_context" => CodexTimelinePreview {
            category: SessionTimelineCategory::Metadata,
            importance: SessionTimelineImportance::Debug,
            title: line_type.into(),
            role: "system".into(),
            content: summarize_timeline_value(payload, 900),
        },
        "unknown" => CodexTimelinePreview {
            category: SessionTimelineCategory::Unknown,
            importance: SessionTimelineImportance::Debug,
            title: "Unknown Record".into(),
            role: "system".into(),
            content: summarize_timeline_value(value, 900),
        },
        _ => CodexTimelinePreview {
            category: SessionTimelineCategory::Event,
            importance: SessionTimelineImportance::Debug,
            title: line_type.into(),
            role: "system".into(),
            content: summarize_timeline_value(payload, 900),
        },
    };

    represent_timeline_entry(
        record,
        preview.category,
        preview.importance,
        preview.title,
        preview.role,
        preview.content,
        timestamp,
    )
}

pub(crate) fn interpret_timeline_records(
    records: &[SessionRawRecord],
) -> Vec<TimelineRecordSemantics> {
    let has_turn_context = records.iter().any(|record| {
        record.value.get("type").and_then(serde_json::Value::as_str) == Some("turn_context")
    });
    let has_task_started = records
        .iter()
        .any(|record| codex_payload_type(&record.value) == "task_started");
    let has_user_message = records
        .iter()
        .any(|record| codex_payload_type(&record.value) == "user_message");

    records
        .iter()
        .map(|record| {
            let line_type = record
                .value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            let payload = record.value.get("payload").unwrap_or(&record.value);
            let payload_type = codex_payload_type(&record.value);
            let starts_turn = if has_task_started {
                payload_type == "task_started"
            } else if has_user_message {
                payload_type == "user_message"
            } else if has_turn_context {
                line_type == "turn_context"
            } else {
                line_type == "response_item"
                    && payload_type == "message"
                    && payload.get("role").and_then(serde_json::Value::as_str) == Some("user")
            };
            let completes_turn = payload_type == "task_complete";
            let usage_sample = (payload_type == "token_count")
                .then(|| payload.get("info"))
                .flatten()
                .and_then(|info| {
                    let usage = info.get("last_token_usage")?;
                    let deduplication_key = info
                        .get("total_token_usage")
                        .and_then(|total| serde_json::to_string(total).ok())
                        .map(|total| format!("codex-total-{total}"))
                        .unwrap_or_else(|| format!("codex-record-{}", record.index));
                    Some(TimelineUsageSample {
                        deduplication_key,
                        usage: SessionTokenUsage {
                            total_tokens: usage.get("total_tokens").and_then(token_count),
                            input_tokens: codex_uncached_input_tokens(usage),
                            output_tokens: usage.get("output_tokens").and_then(token_count),
                            thinking_tokens: usage
                                .get("reasoning_output_tokens")
                                .and_then(token_count),
                            cache_read_tokens: usage
                                .get("cached_input_tokens")
                                .and_then(token_count),
                            cache_write_tokens: usage
                                .get("cache_write_input_tokens")
                                .and_then(token_count),
                            // Codex 不区分缓存写入的存活时长
                            cache_write_1h_tokens: None,
                            cache_write_5m_tokens: None,
                        },
                    })
                });

            TimelineRecordSemantics {
                starts_turn,
                completes_turn,
                usage_sample,
            }
        })
        .collect()
}

fn codex_payload_type(value: &serde_json::Value) -> &str {
    value
        .get("payload")
        .and_then(|payload| payload.get("type"))
        .or_else(|| value.get("type"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
}

/// Codex 上报的 input_tokens 把命中缓存的部分也算在内，扣除后才与统一口径一致
fn codex_uncached_input_tokens(usage: &serde_json::Value) -> Option<u64> {
    let reported_input = usage.get("input_tokens").and_then(token_count)?;
    let cached_input = usage
        .get("cached_input_tokens")
        .and_then(token_count)
        .unwrap_or(0);
    Some(reported_input.saturating_sub(cached_input))
}

fn token_count(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|count| u64::try_from(count).ok()))
}

struct CodexTimelinePreview {
    category: SessionTimelineCategory,
    importance: SessionTimelineImportance,
    title: String,
    role: String,
    content: String,
}

fn represent_codex_response_item(
    payload: &serde_json::Value,
    payload_type: &str,
) -> CodexTimelinePreview {
    match payload_type {
        "message" => {
            let role = payload
                .get("role")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            let content = represent_codex_content(payload.get("content"));
            let (category, importance, title, display_role) = match role {
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
                "system" | "developer" => (
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
            CodexTimelinePreview {
                category,
                importance,
                title: title.into(),
                role: display_role.into(),
                content,
            }
        }
        "function_call" | "custom_tool_call" => {
            let name = payload
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            let arguments = payload
                .get("arguments")
                .or_else(|| payload.get("input"))
                .map(represent_codex_tool_value)
                .unwrap_or_else(|| "{}".into());
            CodexTimelinePreview {
                category: SessionTimelineCategory::ToolCall,
                importance: SessionTimelineImportance::Detail,
                title: format!("Tool · {name}"),
                role: "tool".into(),
                content: format!("工具: {name}\n输入:\n{arguments}"),
            }
        }
        "function_call_output" | "custom_tool_call_output" => {
            let output = payload
                .get("output")
                .map(represent_codex_tool_value)
                .unwrap_or_else(|| "[工具没有返回内容]".into());
            CodexTimelinePreview {
                category: SessionTimelineCategory::ToolResult,
                importance: SessionTimelineImportance::Detail,
                title: "Tool Result".into(),
                role: "tool".into(),
                content: format!("结果:\n{output}"),
            }
        }
        "reasoning" => CodexTimelinePreview {
            category: SessionTimelineCategory::Reasoning,
            importance: SessionTimelineImportance::Detail,
            title: "Reasoning".into(),
            role: "assistant".into(),
            content: represent_codex_reasoning(payload),
        },
        _ => CodexTimelinePreview {
            category: SessionTimelineCategory::Event,
            importance: SessionTimelineImportance::Debug,
            title: format!("response_item · {payload_type}"),
            role: "system".into(),
            content: summarize_timeline_value(payload, 900),
        },
    }
}

fn represent_codex_event(payload: &serde_json::Value, payload_type: &str) -> CodexTimelinePreview {
    match payload_type {
        "agent_message" => CodexTimelinePreview {
            category: SessionTimelineCategory::Assistant,
            importance: SessionTimelineImportance::Primary,
            title: "Assistant Event".into(),
            role: "assistant".into(),
            content: payload
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("[空消息]")
                .to_string(),
        },
        "user_message" => CodexTimelinePreview {
            category: SessionTimelineCategory::User,
            importance: SessionTimelineImportance::Primary,
            title: "User Event".into(),
            role: "user".into(),
            content: payload
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("[空消息]")
                .to_string(),
        },
        "agent_reasoning" | "reasoning" => CodexTimelinePreview {
            category: SessionTimelineCategory::Reasoning,
            importance: SessionTimelineImportance::Detail,
            title: "Reasoning Event".into(),
            role: "assistant".into(),
            content: payload
                .get("text")
                .or_else(|| payload.get("message"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| summarize_timeline_value(payload, 1_200)),
        },
        event if event.contains("tool") || event.contains("function") => CodexTimelinePreview {
            category: SessionTimelineCategory::ToolResult,
            importance: SessionTimelineImportance::Detail,
            title: event.into(),
            role: "tool".into(),
            content: summarize_timeline_value(payload, 1_200),
        },
        event if is_actionable_codex_event(event) => CodexTimelinePreview {
            category: SessionTimelineCategory::System,
            importance: SessionTimelineImportance::Primary,
            title: format!("event · {payload_type}"),
            role: "system".into(),
            content: summarize_timeline_value(payload, 1_200),
        },
        _ => CodexTimelinePreview {
            category: SessionTimelineCategory::Event,
            importance: SessionTimelineImportance::Debug,
            title: format!("event · {payload_type}"),
            role: "system".into(),
            content: summarize_timeline_value(payload, 900),
        },
    }
}

fn is_actionable_codex_event(event: &str) -> bool {
    let event = event.to_ascii_lowercase();
    [
        "warning",
        "error",
        "abort",
        "request_user_input",
        "approval_request",
    ]
    .iter()
    .any(|marker| event.contains(marker))
}

/// Codex 的兼容事件流可能重复发送 response_item 中已经存在的用户/助手文本。
/// 保留原始记录用于排障，但只让规范 response_item 作为默认可见正文。
pub(crate) fn demote_duplicate_message_events(
    records: &[SessionRawRecord],
    entries: &mut [SessionTimelineEntry],
) {
    let response_messages: Vec<SessionTimelineEntry> = records
        .iter()
        .zip(entries.iter())
        .filter(|(record, _)| {
            record.value.get("type").and_then(serde_json::Value::as_str) == Some("response_item")
                && codex_payload_type(&record.value) == "message"
        })
        .map(|(_, entry)| entry.clone())
        .collect();

    for (record, entry) in records.iter().zip(entries.iter_mut()) {
        let is_compatibility_message = record.value.get("type").and_then(serde_json::Value::as_str)
            == Some("event_msg")
            && matches!(
                codex_payload_type(&record.value),
                "agent_message" | "user_message"
            );
        if !is_compatibility_message {
            continue;
        }

        if response_messages.iter().any(|message| {
            message.category == entry.category
                && message.content == entry.content
                && timestamps_are_equivalent(message.timestamp, entry.timestamp)
        }) {
            entry.importance = SessionTimelineImportance::Debug;
        }
    }
}

fn timestamps_are_equivalent(left: Option<i64>, right: Option<i64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => (left - right).abs() <= 1_000,
        (None, None) => true,
        _ => false,
    }
}

fn represent_codex_content(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .map(|item| {
                if let Some(text) = item
                    .get("text")
                    .or_else(|| item.get("input_text"))
                    .or_else(|| item.get("output_text"))
                    .and_then(serde_json::Value::as_str)
                {
                    return text.to_string();
                }

                let item_type = item
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");
                if item_type.contains("image") {
                    return describe_embedded_data(item)
                        .unwrap_or_else(|| format!("[{item_type} 数据，点击“原始数据”查看]"));
                }
                format!("[{item_type}]\n{}", summarize_timeline_value(item, 700))
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        Some(other) => summarize_timeline_value(other, 1_200),
        None => "[消息没有 content 字段]".into(),
    }
}

fn represent_codex_reasoning(payload: &serde_json::Value) -> String {
    for field in ["summary", "content"] {
        if let Some(value) = payload.get(field) {
            let content = represent_codex_content(Some(value));
            if !content.trim().is_empty() {
                return content;
            }
        }
    }
    summarize_timeline_value(payload, 1_200)
}

fn represent_codex_tool_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text) {
                summarize_timeline_value(&parsed, 1_200)
            } else {
                summarize_timeline_text(text, 1_200)
            }
        }
        other => summarize_timeline_value(other, 1_200),
    }
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

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn resume_uses_the_source_home_and_quotes_shell_values() {
        assert_eq!(
            codex_resume_command("/tmp/it's a home/$(touch nope)", "session-id"),
            "env CODEX_HOME='/tmp/it'\\''s a home/$(touch nope)' codex resume 'session-id'"
        );
    }

    #[test]
    fn windows_resume_encodes_environment_values_for_the_terminal() {
        use base64::Engine;
        let command = windows_codex_resume_command("C:\\my %PATH%\\it's home", "session-id");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(command.split_whitespace().last().unwrap())
            .unwrap();
        let utf16: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(
            String::from_utf16(&utf16).unwrap(),
            "$env:CODEX_HOME = 'C:\\my %PATH%\\it''s home'; & codex resume 'session-id'"
        );
    }

    #[test]
    fn scans_active_and_archived_sources_and_refreshes_cached_labels() {
        let temp = tempdir().unwrap();
        let active = temp.path().join("sessions/2026/10/26");
        let archived = temp.path().join("archived_sessions");
        for dir in [&active, &archived] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(
                dir.join("rollout-session.jsonl"),
                r#"{"type":"session_meta","payload":{"id":"shared-id","cwd":"/project"}}"#,
            )
            .unwrap();
        }
        let mut source = super::super::sources::SessionSource {
            config: super::super::sources::SessionSourceConfig {
                id: "dev".into(),
                label: "开发".into(),
                path: temp.path().to_string_lossy().into_owned(),
                enabled: true,
            },
            is_default: false,
            issue: None,
        };
        let first = scan_source_sessions(&[source.clone()]);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].source.as_ref().unwrap().label, "开发");
        source.config.label = "SkillFlow Dev".into();
        let renamed = scan_source_sessions(&[source.clone()]);
        assert_eq!(renamed[0].source.as_ref().unwrap().label, "SkillFlow Dev");
        source.config.enabled = false;
        assert!(scan_source_sessions(&[source]).is_empty());
        assert!(active.join("rollout-session.jsonl").exists());
    }

    #[cfg(unix)]
    #[test]
    fn overlapping_sources_scan_each_physical_file_once() {
        let temp = tempdir().unwrap();
        let sessions = temp.path().join("sessions");
        std::fs::create_dir(&sessions).unwrap();
        let original = sessions.join("rollout-original.jsonl");
        std::fs::write(
            &original,
            r#"{"type":"session_meta","payload":{"id":"one"}}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(&original, sessions.join("rollout-alias.jsonl")).unwrap();
        let source = super::super::sources::SessionSource {
            config: super::super::sources::SessionSourceConfig {
                id: "dev".into(),
                label: "开发".into(),
                path: temp.path().to_string_lossy().into_owned(),
                enabled: true,
            },
            is_default: false,
            issue: None,
        };
        let sessions = scan_source_sessions(&[source.clone(), source]);
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].source_path,
            std::fs::canonicalize(original).unwrap().to_string_lossy()
        );
    }

    #[test]
    fn extract_uuid_from_rollout_filename() {
        let uuid = extract_uuid_from_filename(
            "rollout-2026-03-31T12-58-23-019d4241-d505-7d02-b5be-2af94d1313eb.jsonl",
        );
        assert_eq!(uuid, "019d4241-d505-7d02-b5be-2af94d1313eb");
    }

    #[test]
    fn extract_session_meta_keeps_current_fork_session_id() {
        let temp = tempdir().expect("tempdir");
        let source = temp
            .path()
            .join("rollout-2026-08-07T17-20-50-current.jsonl");
        std::fs::write(
            &source,
            [
                r#"{"type":"session_meta","payload":{"id":"current-session","session_id":"current-session","cwd":"/current"}}"#,
                r#"{"type":"session_meta","payload":{"id":"ancestor-session","session_id":"ancestor-session","cwd":"/ancestor"}}"#,
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Build the thing"}]}}"#,
            ]
            .join("\n"),
        )
        .expect("write");

        let meta = extract_session_meta(&source, "rollout-2026-08-07T17-20-50-current.jsonl")
            .expect("meta");

        assert_eq!(meta.session_id, "current-session");
        assert_eq!(
            meta.resume_command.as_deref(),
            Some("codex resume current-session")
        );
        assert_eq!(meta.project_dir.as_deref(), Some("/current"));
        assert_eq!(meta.title, "Build the thing");
        assert_eq!(meta.source_file_path.as_deref(), source.to_str());
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
