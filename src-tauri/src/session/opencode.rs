use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;

use super::{
    assemble_session_timeline, compact_raw_value, represent_timeline_entry,
    summarize_timeline_text, summarize_timeline_value, timeline_section_priority, SessionMessage,
    SessionMeta, SessionRawRecord, SessionRawReference, SessionSourceRevision, SessionTimeline,
    SessionTimelineCategory, SessionTimelineEntry, SessionTimelineImportance, SessionTokenUsage,
    TimelineRecordSemantics, TimelineUsageSample,
};

/// OpenCode 数据库候选路径。
///
/// OpenCode 的数据目录在不同平台/启动方式下可能落在 XDG、系统应用数据目录，
/// 或历史 Linux 风格目录中；扫描多个候选路径比只押一个默认值更稳。
fn opencode_db_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if push_opencode_db_candidate(&mut paths, xdg) {
            return paths;
        }
    }
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        push_opencode_db_candidate(&mut paths, local_app_data);
    }
    if let Ok(app_data) = std::env::var("APPDATA") {
        push_opencode_db_candidate(&mut paths, app_data);
    }
    if let Some(data_local) = dirs::data_local_dir() {
        push_unique_path(&mut paths, data_local.join("opencode").join("opencode.db"));
    }
    if let Some(data) = dirs::data_dir() {
        push_unique_path(&mut paths, data.join("opencode").join("opencode.db"));
    }
    if let Some(home) = dirs::home_dir() {
        push_unique_path(
            &mut paths,
            home.join(".local/share/opencode").join("opencode.db"),
        );
    }

    if paths.is_empty() {
        paths.push(PathBuf::from(".local/share/opencode/opencode.db"));
    }

    paths
}

fn push_opencode_db_candidate(paths: &mut Vec<PathBuf>, base: String) -> bool {
    if !base.trim().is_empty() {
        push_unique_path(
            paths,
            PathBuf::from(base).join("opencode").join("opencode.db"),
        );
        return true;
    }
    false
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

/// 扫描 OpenCode 会话（仅 SQLite 存储）
pub fn scan_sessions() -> Vec<SessionMeta> {
    let mut sessions = Vec::new();
    for db_path in opencode_db_paths() {
        sessions.extend(scan_db_sessions(&db_path));
    }
    sessions.sort_unstable_by(|a, b| b.last_active_at.cmp(&a.last_active_at));
    sessions
}

fn scan_db_sessions(db_path: &PathBuf) -> Vec<SessionMeta> {
    if !db_path.exists() {
        return Vec::new();
    }
    let Ok(conn) = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };

    let Ok(mut stmt) = conn.prepare(
        "SELECT id, title, directory, time_created, time_updated FROM session ORDER BY time_updated DESC",
    ) else {
        return Vec::new();
    };

    let db_display = db_path.display().to_string();

    let Ok(iter) = stmt.query_map([], |row| {
        let session_id: String = row.get(0)?;
        let title: String = row.get(1)?;
        let directory: String = row.get(2)?;
        let _created: i64 = row.get(3)?;
        let updated: i64 = row.get(4)?;
        Ok((session_id, title, directory, updated))
    }) else {
        return Vec::new();
    };

    let mut sessions = Vec::new();
    for row in iter.flatten() {
        let (session_id, title, directory, updated) = row;

        // 标题优先级：title 非空 > directory basename > session_id 前 8 位
        let final_title = if !title.trim().is_empty() {
            title
        } else if let Some(base) = path_basename(&directory) {
            base
        } else {
            session_id.chars().take(8).collect()
        };

        sessions.push(SessionMeta {
            agent: "opencode".into(),
            source: None,
            resume_command: Some(format!("opencode -s {session_id}")),
            session_id: session_id.clone(),
            title: truncate(&final_title, 80),
            project_dir: if directory.is_empty() {
                None
            } else {
                Some(directory)
            },
            last_active_at: Some(updated),
            source_path: format!("sqlite:{db_display}:{session_id}"),
            source_file_path: None,
        });
    }

    sessions
}

/// 解析 source_path：`sqlite:<db_path>:<session_id>`
///
/// 用 `rfind(":ses_")` 定位分隔点：db_path 可能含冒号（如 Windows `C:\`），
/// 但 OpenCode 约定 session id 以 `ses_` 前缀开头。
fn parse_sqlite_source(source: &str) -> Option<(PathBuf, String)> {
    let rest = source.strip_prefix("sqlite:")?;
    let sep = rest.rfind(":ses_")?;
    let db_path = PathBuf::from(&rest[..sep]);
    let session_id = rest[sep + 1..].to_string();
    Some((db_path, session_id))
}

/// 从 SQLite 读取单个会话的消息（join message + part）
pub fn parse_messages(source_path: &str) -> Result<Vec<SessionMessage>, String> {
    let (db_path, session_id) = parse_sqlite_source(source_path)
        .ok_or_else(|| format!("invalid opencode source path: {source_path}"))?;

    let conn = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("failed to open opencode db: {e}"))?;

    let mut msg_stmt = conn
        .prepare(
            "SELECT id, time_created, data FROM message WHERE session_id = ?1 ORDER BY time_created ASC",
        )
        .map_err(|e| format!("failed to prepare message query: {e}"))?;

    let msg_rows = msg_stmt
        .query_map([session_id.as_str()], |row| {
            let id: String = row.get(0)?;
            let ts: i64 = row.get(1)?;
            let data: String = row.get(2)?;
            Ok((id, ts, data))
        })
        .map_err(|e| format!("failed to query messages: {e}"))?;

    // 预读所有 part，按 message_id 分桶
    let mut part_stmt = conn
        .prepare(
            "SELECT message_id, data FROM part WHERE session_id = ?1 ORDER BY time_created ASC, id ASC",
        )
        .map_err(|e| format!("failed to prepare part query: {e}"))?;

    let part_rows = part_stmt
        .query_map([session_id.as_str()], |row| {
            let message_id: String = row.get(0)?;
            let data: String = row.get(1)?;
            Ok((message_id, data))
        })
        .map_err(|e| format!("failed to query parts: {e}"))?;

    let mut parts_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for (message_id, data) in part_rows.flatten() {
        parts_map.entry(message_id).or_default().push(data);
    }

    let mut messages = Vec::new();
    for (msg_id, ts, data) in msg_rows.flatten() {
        let Ok(msg_value) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        let role = msg_value
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        // OpenCode 会把一次 assistant 回复拆成 text / reasoning / tool 等多个 part。
        // 逐个 part 展示，避免多个工具调用挤在同一个气泡里只剩 [Tool: bash]。
        if let Some(parts) = parts_map.get(&msg_id) {
            for part_data in parts {
                let Ok(part_value) = serde_json::from_str::<Value>(part_data) else {
                    continue;
                };
                if let Some((part_role, content)) = extract_part_message(&part_value, &role) {
                    messages.push(SessionMessage {
                        role: part_role,
                        content,
                        timestamp: Some(ts),
                    });
                }
            }
        }
    }

    Ok(messages)
}

pub fn source_revision(source_path: &str) -> Result<SessionSourceRevision, String> {
    let (db_path, _session_id) = parse_sqlite_source(source_path)
        .ok_or_else(|| format!("invalid opencode source path: {source_path}"))?;

    super::file_source_revision(&db_path)
        .ok_or_else(|| format!("failed to read opencode db metadata: {}", db_path.display()))
}

/// 导出 OpenCode 会话在 SQLite 中的原始表结构。
pub fn load_raw_records(source_path: &str) -> Result<Vec<SessionRawRecord>, String> {
    Ok(load_raw_records_exact(source_path)?
        .into_iter()
        .map(|record| SessionRawRecord {
            value: compact_raw_value(record.value),
            ..record
        })
        .collect())
}

pub(crate) fn load_raw_records_exact(source_path: &str) -> Result<Vec<SessionRawRecord>, String> {
    let (conn, session_id) = open_session_reader(source_path)?;

    let mut records = Vec::new();

    if let Some(session) = load_raw_session_row_exact(&conn, &session_id)? {
        records.push(SessionRawRecord {
            section: "session".into(),
            index: 1,
            value: session,
        });
    }

    records.extend(load_raw_message_rows_exact(&conn, &session_id)?);
    records.extend(load_raw_part_rows_exact(&conn, &session_id)?);

    Ok(records)
}

pub(super) fn load_raw_session_message_rows(
    source_path: &str,
) -> Result<Vec<SessionRawRecord>, String> {
    let (conn, session_id) = open_session_reader(source_path)?;
    load_raw_message_rows_exact(&conn, &session_id)
}

fn open_session_reader(source_path: &str) -> Result<(Connection, String), String> {
    let (db_path, session_id) = parse_sqlite_source(source_path)
        .ok_or_else(|| format!("invalid opencode source path: {source_path}"))?;
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("failed to open opencode db: {error}"))?;
    Ok((conn, session_id))
}

fn load_raw_session_row_exact(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<Value>, String> {
    let row = conn
        .query_row(
            "SELECT id, title, directory, time_created, time_updated FROM session WHERE id = ?1",
            [session_id],
            |row| {
                let id: String = row.get(0)?;
                let title: String = row.get(1)?;
                let directory: String = row.get(2)?;
                let time_created: i64 = row.get(3)?;
                let time_updated: i64 = row.get(4)?;
                Ok(serde_json::json!({
                    "id": id,
                    "title": title,
                    "directory": directory,
                    "time_created": time_created,
                    "time_updated": time_updated,
                }))
            },
        )
        .optional()
        .map_err(|e| format!("failed to query session row: {e}"))?;

    Ok(row)
}

fn load_raw_message_rows_exact(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<SessionRawRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, time_created, data FROM message WHERE session_id = ?1 ORDER BY time_created ASC, id ASC",
        )
        .map_err(|e| format!("failed to prepare raw message query: {e}"))?;

    let rows = stmt
        .query_map([session_id], |row| {
            let id: String = row.get(0)?;
            let session_id: String = row.get(1)?;
            let time_created: i64 = row.get(2)?;
            let data: String = row.get(3)?;
            Ok(serde_json::json!({
                "id": id,
                "session_id": session_id,
                "time_created": time_created,
                "data": parse_json_text_as_raw_value_exact(&data),
            }))
        })
        .map_err(|e| format!("failed to query raw messages: {e}"))?;

    let mut records = Vec::new();
    for (index, row) in rows.enumerate() {
        records.push(SessionRawRecord {
            section: "message".into(),
            index: index + 1,
            value: row.map_err(|e| format!("failed to read raw message row: {e}"))?,
        });
    }

    Ok(records)
}

fn load_raw_part_rows_exact(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<SessionRawRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, message_id, time_created, data FROM part WHERE session_id = ?1 ORDER BY time_created ASC, id ASC",
        )
        .map_err(|e| format!("failed to prepare raw part query: {e}"))?;

    let rows = stmt
        .query_map([session_id], |row| {
            let id: String = row.get(0)?;
            let session_id: String = row.get(1)?;
            let message_id: String = row.get(2)?;
            let time_created: i64 = row.get(3)?;
            let data: String = row.get(4)?;
            Ok(serde_json::json!({
                "id": id,
                "session_id": session_id,
                "message_id": message_id,
                "time_created": time_created,
                "data": parse_json_text_as_raw_value_exact(&data),
            }))
        })
        .map_err(|e| format!("failed to query raw parts: {e}"))?;

    let mut records = Vec::new();
    for (index, row) in rows.enumerate() {
        records.push(SessionRawRecord {
            section: "part".into(),
            index: index + 1,
            value: row.map_err(|e| format!("failed to read raw part row: {e}"))?,
        });
    }

    Ok(records)
}

fn parse_json_text_as_raw_value_exact(text: &str) -> Value {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => value,
        Err(err) => serde_json::json!({
            "_parse_error": err.to_string(),
            "raw": text,
        }),
    }
}

pub(crate) fn load_timeline_entries(source_path: &str) -> Result<SessionTimeline, String> {
    let records = load_raw_records_exact(source_path)?;
    let message_roles: std::collections::HashMap<String, String> = records
        .iter()
        .filter(|record| record.section == "message")
        .filter_map(|record| {
            let message_id = record.value.get("id")?.as_str()?.to_string();
            let role = record.value.get("data")?.get("role")?.as_str()?.to_string();
            Some((message_id, role))
        })
        .collect();

    let mut adapted_records: Vec<(SessionTimelineEntry, TimelineRecordSemantics)> = records
        .iter()
        .map(|record| {
            let message_role = record
                .value
                .get("message_id")
                .and_then(Value::as_str)
                .and_then(|message_id| message_roles.get(message_id))
                .map(String::as_str);
            (
                represent_timeline_record(record, message_role),
                interpret_timeline_record(record),
            )
        })
        .collect();

    adapted_records.sort_by(|left, right| {
        left.0
            .timestamp
            .cmp(&right.0.timestamp)
            .then_with(|| {
                timeline_section_priority(&left.0.raw_reference.section)
                    .cmp(&timeline_section_priority(&right.0.raw_reference.section))
            })
            .then_with(|| left.0.raw_reference.index.cmp(&right.0.raw_reference.index))
    });
    let (entries, semantics) = adapted_records.into_iter().unzip();
    Ok(assemble_session_timeline(entries, semantics))
}

pub(crate) fn load_raw_record_exact(
    source_path: &str,
    reference: &SessionRawReference,
) -> Result<Value, String> {
    let (db_path, session_id) = parse_sqlite_source(source_path)
        .ok_or_else(|| format!("invalid opencode source path: {source_path}"))?;
    let conn = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("failed to open opencode db: {error}"))?;
    let record_id = reference.record_id.as_deref().ok_or_else(|| {
        format!(
            "raw OpenCode reference has no stable id section={} index={}",
            reference.section, reference.index
        )
    })?;

    let value = match reference.section.as_str() {
        "session" if record_id == session_id => load_raw_session_row_exact(&conn, &session_id)?,
        "message" => load_raw_message_row_exact(&conn, &session_id, record_id)?,
        "part" => load_raw_part_row_exact(&conn, &session_id, record_id)?,
        section => return Err(format!("unknown OpenCode raw section: {section}")),
    };

    value.ok_or_else(|| {
        format!(
            "raw OpenCode record not found section={} index={} id={record_id}",
            reference.section, reference.index
        )
    })
}

fn load_raw_message_row_exact(
    conn: &Connection,
    session_id: &str,
    record_id: &str,
) -> Result<Option<Value>, String> {
    conn.query_row(
        "SELECT id, session_id, time_created, data FROM message WHERE session_id = ?1 AND id = ?2",
        [session_id, record_id],
        |row| {
            let id: String = row.get(0)?;
            let session_id: String = row.get(1)?;
            let time_created: i64 = row.get(2)?;
            let data: String = row.get(3)?;
            Ok(serde_json::json!({
                "id": id,
                "session_id": session_id,
                "time_created": time_created,
                "data": parse_json_text_as_raw_value_exact(&data),
            }))
        },
    )
    .optional()
    .map_err(|error| format!("failed to query raw message {record_id}: {error}"))
}

fn load_raw_part_row_exact(
    conn: &Connection,
    session_id: &str,
    record_id: &str,
) -> Result<Option<Value>, String> {
    conn.query_row(
        "SELECT id, session_id, message_id, time_created, data FROM part WHERE session_id = ?1 AND id = ?2",
        [session_id, record_id],
        |row| {
            let id: String = row.get(0)?;
            let session_id: String = row.get(1)?;
            let message_id: String = row.get(2)?;
            let time_created: i64 = row.get(3)?;
            let data: String = row.get(4)?;
            Ok(serde_json::json!({
                "id": id,
                "session_id": session_id,
                "message_id": message_id,
                "time_created": time_created,
                "data": parse_json_text_as_raw_value_exact(&data),
            }))
        },
    )
    .optional()
    .map_err(|error| format!("failed to query raw part {record_id}: {error}"))
}

fn represent_timeline_record(
    record: &SessionRawRecord,
    message_role: Option<&str>,
) -> SessionTimelineEntry {
    let timestamp = record
        .value
        .get("time_created")
        .or_else(|| record.value.get("time_updated"))
        .and_then(Value::as_i64);
    let preview = match record.section.as_str() {
        "session" => OpenCodeTimelinePreview {
            category: SessionTimelineCategory::Metadata,
            importance: SessionTimelineImportance::Debug,
            title: "Session Metadata".into(),
            role: "system".into(),
            content: summarize_timeline_value(&record.value, 900),
        },
        "message" => represent_opencode_message_record(record),
        "part" => represent_opencode_part_record(record, message_role),
        _ => OpenCodeTimelinePreview {
            category: SessionTimelineCategory::Unknown,
            importance: SessionTimelineImportance::Debug,
            title: format!("Unknown · {}", record.section),
            role: "system".into(),
            content: summarize_timeline_value(&record.value, 900),
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

fn interpret_timeline_record(record: &SessionRawRecord) -> TimelineRecordSemantics {
    if record.section != "message" {
        return TimelineRecordSemantics::default();
    }

    let data = record.value.get("data").unwrap_or(&record.value);
    let role = data.get("role").and_then(Value::as_str);
    let starts_turn = role == Some("user");
    let completed_at = data
        .get("time")
        .and_then(|time| time.get("completed"))
        .and_then(Value::as_i64);
    let finish = data.get("finish").and_then(Value::as_str);
    let has_error = data.get("error").is_some_and(|error| !error.is_null());
    let completes_turn = role == Some("assistant")
        && completed_at.is_some()
        && (has_error || finish.is_some_and(|finish| finish != "tool-calls"));
    let usage_sample = (role == Some("assistant"))
        .then(|| data.get("tokens"))
        .flatten()
        .map(|tokens| TimelineUsageSample {
            deduplication_key: record
                .value
                .get("id")
                .and_then(Value::as_str)
                .map(|id| format!("opencode-message-{id}"))
                .unwrap_or_else(|| format!("opencode-record-{}", record.index)),
            usage: SessionTokenUsage {
                total_tokens: tokens.get("total").and_then(token_count),
                input_tokens: tokens.get("input").and_then(token_count),
                output_tokens: opencode_output_tokens_with_reasoning(tokens),
                thinking_tokens: tokens.get("reasoning").and_then(token_count),
                cache_read_tokens: tokens
                    .get("cache")
                    .and_then(|cache| cache.get("read"))
                    .and_then(token_count),
                cache_write_tokens: tokens
                    .get("cache")
                    .and_then(|cache| cache.get("write"))
                    .and_then(token_count),
                // OpenCode 不区分缓存写入的存活时长
                cache_write_1h_tokens: None,
                cache_write_5m_tokens: None,
            },
        });

    TimelineRecordSemantics {
        starts_turn,
        completes_turn,
        usage_sample,
    }
}

/// OpenCode 把思考产出单独计在 reasoning 里，合并后才与统一口径一致
fn opencode_output_tokens_with_reasoning(tokens: &Value) -> Option<u64> {
    let text_output = tokens.get("output").and_then(token_count)?;
    let reasoning_output = tokens.get("reasoning").and_then(token_count).unwrap_or(0);
    Some(text_output.saturating_add(reasoning_output))
}

fn token_count(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|count| u64::try_from(count).ok()))
}

struct OpenCodeTimelinePreview {
    category: SessionTimelineCategory,
    importance: SessionTimelineImportance,
    title: String,
    role: String,
    content: String,
}

fn represent_opencode_message_record(record: &SessionRawRecord) -> OpenCodeTimelinePreview {
    let data = record.value.get("data").unwrap_or(&record.value);
    let role = data
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    OpenCodeTimelinePreview {
        category: SessionTimelineCategory::Metadata,
        importance: SessionTimelineImportance::Debug,
        title: format!("Message Metadata · {role}"),
        role: "system".into(),
        content: summarize_timeline_value(data, 700),
    }
}

fn represent_opencode_part_record(
    record: &SessionRawRecord,
    message_role: Option<&str>,
) -> OpenCodeTimelinePreview {
    let data = record.value.get("data").unwrap_or(&record.value);
    let part_type = data
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    match part_type {
        "text" => {
            let role = message_role.unwrap_or("assistant");
            let (category, title, display_role) = if role == "user" {
                (SessionTimelineCategory::User, "User Text", "user")
            } else {
                (
                    SessionTimelineCategory::Assistant,
                    "Assistant Text",
                    "assistant",
                )
            };
            OpenCodeTimelinePreview {
                category,
                importance: SessionTimelineImportance::Primary,
                title: title.into(),
                role: display_role.into(),
                content: data
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("[空文本块]")
                    .to_string(),
            }
        }
        "reasoning" => OpenCodeTimelinePreview {
            category: SessionTimelineCategory::Reasoning,
            importance: SessionTimelineImportance::Detail,
            title: "Reasoning".into(),
            role: "assistant".into(),
            content: data
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("[空 reasoning 块]")
                .to_string(),
        },
        "tool" => represent_opencode_tool(data),
        "file" => OpenCodeTimelinePreview {
            category: SessionTimelineCategory::Event,
            importance: SessionTimelineImportance::Primary,
            title: "File".into(),
            role: message_role.unwrap_or("assistant").into(),
            content: summarize_timeline_value(data, 900),
        },
        "patch" | "retry" | "agent" | "subtask" => OpenCodeTimelinePreview {
            category: SessionTimelineCategory::Event,
            importance: SessionTimelineImportance::Detail,
            title: format!("Part · {part_type}"),
            role: "system".into(),
            content: summarize_timeline_value(data, 900),
        },
        _ => OpenCodeTimelinePreview {
            category: SessionTimelineCategory::Event,
            importance: SessionTimelineImportance::Debug,
            title: format!("Part · {part_type}"),
            role: "system".into(),
            content: summarize_timeline_value(data, 900),
        },
    }
}

fn represent_opencode_tool(data: &Value) -> OpenCodeTimelinePreview {
    let tool = data
        .get("tool")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let state = data.get("state");
    let status = state
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let mut sections = vec![format!("工具: {tool}"), format!("状态: {status}")];

    if let Some(input) = state.and_then(|value| value.get("input")) {
        sections.push(format!("输入:\n{}", summarize_timeline_value(input, 900)));
    }
    if let Some(output) = state
        .and_then(|value| value.get("output"))
        .and_then(Value::as_str)
        .filter(|output| !output.trim().is_empty())
    {
        sections.push(format!("结果:\n{}", summarize_timeline_text(output, 1_200)));
    }
    if let Some(exit) = state
        .and_then(|value| value.get("metadata"))
        .and_then(|value| value.get("exit"))
        .and_then(Value::as_i64)
    {
        sections.push(format!("退出码: {exit}"));
    }

    OpenCodeTimelinePreview {
        category: SessionTimelineCategory::ToolCall,
        importance: SessionTimelineImportance::Detail,
        title: format!("Tool · {tool}"),
        role: "tool".into(),
        content: sections.join("\n"),
    }
}

/// 删除 time_updated 早于 cutoff 的会话（三张表手动清理，不依赖外键级联）
pub fn purge_sessions(cutoff_millis: i64) -> usize {
    opencode_db_paths()
        .into_iter()
        .map(|db_path| purge_db_sessions(&db_path, cutoff_millis))
        .sum()
}

fn purge_db_sessions(db_path: &PathBuf, cutoff_millis: i64) -> usize {
    if !db_path.exists() {
        return 0;
    }

    let Ok(conn) = Connection::open(db_path) else {
        return 0;
    };

    // 查要删的 session_id
    let Ok(mut stmt) = conn.prepare("SELECT id FROM session WHERE time_updated < ?1") else {
        return 0;
    };
    let Ok(rows) = stmt.query_map([cutoff_millis], |row| row.get::<_, String>(0)) else {
        return 0;
    };
    let ids: Vec<String> = rows.flatten().collect();
    drop(stmt);

    if ids.is_empty() {
        return 0;
    }

    let Ok(tx) = conn.unchecked_transaction() else {
        return 0;
    };

    let mut deleted = 0;
    for id in &ids {
        let _ = tx.execute("DELETE FROM part WHERE session_id = ?1", [id]);
        let _ = tx.execute("DELETE FROM message WHERE session_id = ?1", [id]);
        if tx
            .execute("DELETE FROM session WHERE id = ?1", [id])
            .map(|n| n > 0)
            .unwrap_or(false)
        {
            deleted += 1;
        }
    }

    if tx.commit().is_err() {
        return 0;
    }
    deleted
}

fn extract_part_message(part: &Value, message_role: &str) -> Option<(String, String)> {
    match part.get("type").and_then(Value::as_str) {
        Some("text") => part
            .get("text")
            .and_then(Value::as_str)
            .filter(|t| !t.trim().is_empty())
            .map(|t| (message_role.to_string(), t.to_string())),
        Some("reasoning") => part
            .get("text")
            .and_then(Value::as_str)
            .filter(|t| !t.trim().is_empty())
            .map(|t| ("assistant".into(), format!("[Reasoning]\n{t}"))),
        Some("tool") => Some(("tool".into(), format_tool_part(part))),
        _ => None,
    }
}

fn format_tool_part(part: &Value) -> String {
    let tool = part
        .get("tool")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let title = part.get("title").and_then(Value::as_str);
    let call_id = part.get("callID").and_then(Value::as_str);
    let state = part.get("state");
    let status = state
        .and_then(|v| v.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    let mut lines = Vec::new();
    lines.push(match title {
        Some(t) if !t.trim().is_empty() => format!("[Tool: {tool}] {t}"),
        _ => format!("[Tool: {tool}]"),
    });
    lines.push(format!("Status: {status}"));
    if let Some(id) = call_id {
        lines.push(format!("Call ID: {id}"));
    }

    if let Some(input) = state.and_then(|v| v.get("input")) {
        lines.push("Input:".into());
        lines.push(format_json_value(input));
    }

    if let Some(output) = state.and_then(|v| v.get("output")).and_then(Value::as_str) {
        if !output.trim().is_empty() {
            lines.push("Output:".into());
            lines.push(output.to_string());
        }
    }

    if let Some(metadata) = state.and_then(|v| v.get("metadata")) {
        if let Some(exit) = metadata.get("exit").and_then(Value::as_i64) {
            lines.push(format!("Exit: {exit}"));
        }
        if metadata
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            lines.push("Output truncated: true".into());
        }
    }

    lines.join("\n")
}

fn format_json_value(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn path_basename(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.trim_end_matches(['/', '\\']);
    normalized
        .split(['/', '\\'])
        .next_back()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
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
    use crate::session::SessionTurnStatus;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    #[test]
    fn failed_opencode_tools_stay_collapsed_and_message_metadata_stays_debug() {
        let tool = represent_opencode_tool(&serde_json::json!({
            "type": "tool",
            "tool": "bash",
            "state": {"status": "failed", "output": "permission denied"},
        }));
        let message = SessionRawRecord {
            section: "message".into(),
            index: 1,
            value: serde_json::json!({
                "data": {"role": "assistant", "error": {"message": "model unavailable"}},
            }),
        };

        assert_eq!(tool.importance, SessionTimelineImportance::Detail);
        assert_eq!(
            represent_opencode_message_record(&message).importance,
            SessionTimelineImportance::Debug
        );
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn create_schema(conn: &Connection) {
        conn.execute_batch(
            "
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                directory TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            ",
        )
        .expect("create schema");
    }

    #[test]
    #[allow(deprecated)]
    fn opencode_db_paths_honors_xdg_override() {
        let _g = env_lock().lock().expect("lock");
        let temp = tempdir().expect("tempdir");
        let original = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", temp.path());

        let paths = opencode_db_paths();

        if let Some(v) = original {
            std::env::set_var("XDG_DATA_HOME", v);
        } else {
            std::env::remove_var("XDG_DATA_HOME");
        }

        assert_eq!(paths, vec![temp.path().join("opencode/opencode.db")]);
    }

    #[test]
    fn parse_sqlite_source_accepts_valid() {
        let (p, id) = parse_sqlite_source("sqlite:/tmp/opencode.db:ses_123").expect("valid");
        assert_eq!(p, PathBuf::from("/tmp/opencode.db"));
        assert_eq!(id, "ses_123");
    }

    #[test]
    fn parse_sqlite_source_rejects_invalid() {
        assert!(parse_sqlite_source("/tmp/x.db:ses_1").is_none());
        assert!(parse_sqlite_source("sqlite:/tmp/x.db:msg_1").is_none());
        assert!(parse_sqlite_source("sqlite:/tmp/x.db").is_none());
    }

    #[test]
    #[allow(deprecated)]
    fn scan_sessions_reads_db() {
        let _g = env_lock().lock().expect("lock");
        let temp = tempdir().expect("tempdir");
        let original = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", temp.path());

        let base = temp.path().join("opencode");
        std::fs::create_dir_all(&base).expect("base");
        let db = base.join("opencode.db");
        let conn = Connection::open(&db).expect("open");
        create_schema(&conn);
        conn.execute(
            "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5)",
            ("ses_1", "", "/tmp/proj-a", 1000_i64, 2000_i64),
        )
        .expect("insert 1");
        conn.execute(
            "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5)",
            ("ses_2", "Named", "/tmp/proj-b", 1500_i64, 2500_i64),
        )
        .expect("insert 2");
        drop(conn);

        let sessions = scan_sessions();

        if let Some(v) = original {
            std::env::set_var("XDG_DATA_HOME", v);
        } else {
            std::env::remove_var("XDG_DATA_HOME");
        }

        assert_eq!(sessions.len(), 2);
        // time_updated DESC
        assert_eq!(sessions[0].session_id, "ses_2");
        assert_eq!(sessions[0].title, "Named");
        assert_eq!(sessions[0].source_file_path, None);
        assert_eq!(sessions[1].session_id, "ses_1");
        // 空 title fallback 到 directory basename
        assert_eq!(sessions[1].title, "proj-a");
        assert_eq!(sessions[1].project_dir.as_deref(), Some("/tmp/proj-a"));
        assert!(sessions[0].source_path.starts_with("sqlite:"));
        assert!(sessions[0].source_path.ends_with(":ses_2"));
        assert_eq!(sessions[0].agent, "opencode");
        assert_eq!(
            sessions[0].resume_command.as_deref(),
            Some("opencode -s ses_2")
        );
    }

    #[test]
    fn parse_messages_splits_parts_and_expands_tool_details() {
        let temp = tempdir().expect("tempdir");
        let db = temp.path().join("opencode.db");
        let conn = Connection::open(&db).expect("open");
        create_schema(&conn);

        conn.execute(
            "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5)",
            ("ses_1", "S", "/tmp/p", 1000_i64, 3000_i64),
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            ("msg_1", "ses_1", 1000_i64, r#"{"role":"user"}"#),
        )
        .expect("insert msg1");
        conn.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            ("msg_2", "ses_1", 2000_i64, r#"{"role":"assistant"}"#),
        )
        .expect("insert msg2");
        conn.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                "prt_1",
                "ses_1",
                "msg_1",
                1000_i64,
                r#"{"type":"text","text":"Hello"}"#,
            ),
        )
        .expect("part1");
        conn.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                "prt_2",
                "ses_1",
                "msg_2",
                2000_i64,
                r#"{"type":"tool","tool":"bash","callID":"call_1","state":{"status":"completed","input":{"command":"ls -la","description":"列文件"},"output":"ok","metadata":{"exit":0,"truncated":false}},"title":"列文件"}"#,
            ),
        )
        .expect("part2");
        conn.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                "prt_3",
                "ses_1",
                "msg_2",
                2001_i64,
                r#"{"type":"text","text":"Done"}"#,
            ),
        )
        .expect("part3");
        drop(conn);

        let source = format!("sqlite:{}:ses_1", db.display());
        let msgs = parse_messages(&source).expect("parse");

        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "Hello");
        assert_eq!(msgs[0].timestamp, Some(1000));
        // assistant 下的多个 part 分开展示，工具调用保留输入、输出和状态。
        assert_eq!(msgs[1].role, "tool");
        assert!(msgs[1].content.contains("[Tool: bash]"));
        assert!(msgs[1].content.contains("Status: completed"));
        assert!(msgs[1].content.contains("\"command\": \"ls -la\""));
        assert!(msgs[1].content.contains("Output:\nok"));
        assert_eq!(msgs[2].role, "assistant");
        assert_eq!(msgs[2].content, "Done");
    }

    #[test]
    fn timeline_keeps_every_opencode_row_and_reveals_exact_tool_record() {
        let temp = tempdir().expect("tempdir");
        let db = temp.path().join("opencode.db");
        let conn = Connection::open(&db).expect("open");
        create_schema(&conn);
        conn.execute(
            "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5)",
            ("ses_1", "S", "/tmp/p", 1000_i64, 4000_i64),
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            (
                "msg_1",
                "ses_1",
                2000_i64,
                r#"{"role":"assistant","modelID":"test"}"#,
            ),
        )
        .expect("insert message");
        conn.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                "prt_text",
                "ses_1",
                "msg_1",
                2100_i64,
                r#"{"type":"text","text":"完成"}"#,
            ),
        )
        .expect("insert text part");
        let long_output = "x".repeat(1_400);
        conn.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                "prt_tool",
                "ses_1",
                "msg_1",
                2200_i64,
                serde_json::json!({
                    "type": "tool",
                    "tool": "bash",
                    "state": {
                        "status": "completed",
                        "input": {"command": "pnpm test"},
                        "output": long_output,
                        "metadata": {"exit": 0},
                    }
                })
                .to_string(),
            ),
        )
        .expect("insert tool part");
        conn.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                "prt_unknown",
                "ses_1",
                "msg_1",
                2300_i64,
                r#"{"type":"future_part","payload":{"kept":true}}"#,
            ),
        )
        .expect("insert unknown part");
        drop(conn);

        let source = format!("sqlite:{}:ses_1", db.display());
        let entries = load_timeline_entries(&source).expect("timeline");

        assert_eq!(entries.len(), 5);
        assert!(entries.iter().any(|entry| {
            entry.category == SessionTimelineCategory::Assistant
                && entry.importance == SessionTimelineImportance::Primary
                && entry.content == "完成"
        }));
        assert!(entries.iter().any(|entry| {
            entry.category == SessionTimelineCategory::Event
                && entry.importance == SessionTimelineImportance::Debug
                && entry.title == "Part · future_part"
                && entry.content.contains("kept")
        }));
        let tool_entry = entries
            .iter()
            .find(|entry| entry.category == SessionTimelineCategory::ToolCall)
            .expect("tool entry");
        assert_eq!(tool_entry.importance, SessionTimelineImportance::Detail);
        assert!(tool_entry.content.contains("pnpm test"));
        assert!(tool_entry.content.contains("退出码: 0"));
        assert!(tool_entry.content.len() < 1_400);

        let raw = load_raw_record_exact(&source, &tool_entry.raw_reference).expect("tool raw");
        assert_eq!(
            raw["data"]["state"]["output"]
                .as_str()
                .expect("output")
                .len(),
            1_400
        );
    }

    #[test]
    fn timeline_aggregates_opencode_assistant_usage_after_terminal_finish() {
        let temp = tempdir().expect("tempdir");
        let db = temp.path().join("opencode-usage.db");
        let conn = Connection::open(&db).expect("open");
        create_schema(&conn);
        conn.execute(
            "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5)",
            ("ses_usage", "Usage", "/tmp/p", 1000_i64, 5000_i64),
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            ("msg_user", "ses_usage", 2000_i64, r#"{"role":"user"}"#),
        )
        .expect("insert user");
        conn.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            (
                "msg_tool",
                "ses_usage",
                3000_i64,
                r#"{"role":"assistant","finish":"tool-calls","time":{"created":3000,"completed":3100},"tokens":{"total":100,"input":20,"output":10,"reasoning":4,"cache":{"read":70,"write":0}}}"#,
            ),
        )
        .expect("insert tool call");
        conn.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            (
                "msg_final",
                "ses_usage",
                4000_i64,
                r#"{"role":"assistant","time":{"created":4000},"tokens":{"total":50,"input":15,"output":5,"reasoning":1,"cache":{"read":25,"write":5}}}"#,
            ),
        )
        .expect("insert running final");
        drop(conn);

        let source = format!("sqlite:{}:ses_usage", db.display());
        let running = load_timeline_entries(&source).expect("running timeline");
        assert_eq!(running.blocks[1].status, Some(SessionTurnStatus::Running));
        assert_eq!(running.blocks[1].usage, None);

        let conn = Connection::open(&db).expect("reopen");
        conn.execute(
            "UPDATE message SET data = ?1 WHERE id = 'msg_final'",
            [r#"{"role":"assistant","finish":"stop","time":{"created":4000,"completed":4100},"tokens":{"total":50,"input":15,"output":5,"reasoning":1,"cache":{"read":25,"write":5}}}"#],
        )
        .expect("complete final");
        drop(conn);

        let completed = load_timeline_entries(&source).expect("completed timeline");
        assert_eq!(
            completed.blocks[1].status,
            Some(SessionTurnStatus::Completed)
        );
        assert_eq!(
            completed.blocks[1].usage,
            Some(SessionTokenUsage {
                total_tokens: Some(150),
                input_tokens: Some(35),
                // OpenCode 把 4 和 1 的思考产出单列在 reasoning 里，统一口径并入 output
                output_tokens: Some(20),
                thinking_tokens: Some(5),
                cache_read_tokens: Some(95),
                cache_write_tokens: Some(5),
                cache_write_1h_tokens: None,
                cache_write_5m_tokens: None,
            })
        );
    }

    #[test]
    fn load_raw_records_exports_session_message_and_part_rows() {
        let temp = tempdir().expect("tempdir");
        let db = temp.path().join("opencode.db");
        let conn = Connection::open(&db).expect("open");
        create_schema(&conn);

        conn.execute(
            "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5)",
            ("ses_1", "Raw", "/tmp/p", 1000_i64, 3000_i64),
        )
        .expect("insert session");
        conn.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            (
                "msg_1",
                "ses_1",
                1100_i64,
                r#"{"role":"assistant","extra":"abcdefghijklmnopqrstuvwxyz"}"#,
            ),
        )
        .expect("insert message");
        conn.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                "prt_1",
                "ses_1",
                "msg_1",
                1200_i64,
                r#"{"type":"text","text":"hello"}"#,
            ),
        )
        .expect("insert part");
        drop(conn);

        let source = format!("sqlite:{}:ses_1", db.display());
        let records = load_raw_records(&source).expect("raw records");

        assert_eq!(records.len(), 3);
        assert_eq!(records[0].section, "session");
        assert_eq!(records[0].value["id"], "ses_1");
        assert_eq!(records[1].section, "message");
        assert_eq!(records[1].value["data"]["role"], "assistant");
        assert_eq!(
            records[1].value["data"]["extra"],
            "abcdefghijklmnopqrstuvwxyz"
        );
        assert_eq!(records[2].section, "part");
        assert_eq!(records[2].value["data"]["type"], "text");
    }

    #[test]
    fn load_raw_records_exports_all_message_rows() {
        let temp = tempdir().expect("tempdir");
        let db = temp.path().join("opencode.db");
        let conn = Connection::open(&db).expect("open");
        create_schema(&conn);

        conn.execute(
            "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5)",
            ("ses_1", "Raw", "/tmp/p", 1000_i64, 3000_i64),
        )
        .expect("insert session");

        for index in 0..85 {
            conn.execute(
                "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
                (
                    format!("msg_{index}"),
                    "ses_1",
                    1100_i64 + index as i64,
                    r#"{"role":"assistant"}"#,
                ),
            )
            .expect("insert message");
        }
        drop(conn);

        let source = format!("sqlite:{}:ses_1", db.display());
        let records = load_raw_records(&source).expect("raw records");

        assert_eq!(
            records
                .iter()
                .filter(|record| record.section == "message")
                .count(),
            85
        );
    }

    #[test]
    #[allow(deprecated)]
    fn purge_removes_expired_sessions() {
        let _g = env_lock().lock().expect("lock");
        let temp = tempdir().expect("tempdir");
        let original = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", temp.path());

        let base = temp.path().join("opencode");
        std::fs::create_dir_all(&base).expect("base");
        let db = base.join("opencode.db");
        let conn = Connection::open(&db).expect("open");
        create_schema(&conn);
        conn.execute(
            "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5)",
            ("ses_old", "Old", "/tmp/a", 1000_i64, 1000_i64),
        )
        .expect("old");
        conn.execute(
            "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5)",
            ("ses_new", "New", "/tmp/b", 5000_i64, 5000_i64),
        )
        .expect("new");
        conn.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            ("msg_old", "ses_old", 1000_i64, r#"{"role":"user"}"#),
        )
        .expect("msg");
        conn.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                "prt_old",
                "ses_old",
                "msg_old",
                1000_i64,
                r#"{"type":"text","text":"x"}"#,
            ),
        )
        .expect("part");
        drop(conn);

        let removed = purge_sessions(3000);

        if let Some(v) = original {
            std::env::set_var("XDG_DATA_HOME", v);
        } else {
            std::env::remove_var("XDG_DATA_HOME");
        }

        assert_eq!(removed, 1);

        let conn = Connection::open(&db).expect("reopen");
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
            .expect("count");
        let msgs: i64 = conn
            .query_row("SELECT COUNT(*) FROM message", [], |r| r.get(0))
            .expect("count msgs");
        let parts: i64 = conn
            .query_row("SELECT COUNT(*) FROM part", [], |r| r.get(0))
            .expect("count parts");
        assert_eq!(remaining, 1);
        assert_eq!(msgs, 0);
        assert_eq!(parts, 0);
    }

    #[test]
    fn truncate_respects_max() {
        assert_eq!(truncate("short", 10), "short");
        let long = "a".repeat(100);
        let out = truncate(&long, 20);
        assert!(out.ends_with("..."));
        assert_eq!(out.chars().count(), 20);
    }

    #[test]
    fn path_basename_strips_trailing_slash() {
        assert_eq!(path_basename("/tmp/project/"), Some("project".into()));
        assert_eq!(path_basename("/tmp/project"), Some("project".into()));
        assert_eq!(path_basename("project"), Some("project".into()));
        assert_eq!(path_basename(""), None);
    }
}
