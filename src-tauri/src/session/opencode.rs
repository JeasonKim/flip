use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;

use super::{truncate_raw_value, SessionMessage, SessionMeta, SessionRawRecord};

const RAW_SQLITE_ROW_LIMIT: usize = 80;

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

/// 导出 OpenCode 会话在 SQLite 中的原始表结构。
pub fn load_raw_records_with_truncation(
    source_path: &str,
    max_string_chars: usize,
) -> Result<(Vec<SessionRawRecord>, bool), String> {
    let (db_path, session_id) = parse_sqlite_source(source_path)
        .ok_or_else(|| format!("invalid opencode source path: {source_path}"))?;

    let conn = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("failed to open opencode db: {e}"))?;

    let mut records = Vec::new();

    if let Some(session) = load_raw_session_row(&conn, &session_id, max_string_chars)? {
        records.push(SessionRawRecord {
            section: "session".into(),
            index: 1,
            value: session,
        });
    }

    let (message_records, messages_truncated) =
        load_raw_message_rows(&conn, &session_id, max_string_chars)?;
    let (part_records, parts_truncated) = load_raw_part_rows(&conn, &session_id, max_string_chars)?;
    records.extend(message_records);
    records.extend(part_records);

    Ok((records, messages_truncated || parts_truncated))
}

fn load_raw_session_row(
    conn: &Connection,
    session_id: &str,
    max_string_chars: usize,
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

    Ok(row.map(|value| truncate_raw_value(value, max_string_chars)))
}

fn load_raw_message_rows(
    conn: &Connection,
    session_id: &str,
    max_string_chars: usize,
) -> Result<(Vec<SessionRawRecord>, bool), String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, time_created, data FROM message WHERE session_id = ?1 ORDER BY time_created ASC, id ASC LIMIT ?2",
        )
        .map_err(|e| format!("failed to prepare raw message query: {e}"))?;

    let rows = stmt
        .query_map(
            rusqlite::params![session_id, (RAW_SQLITE_ROW_LIMIT + 1) as i64],
            |row| {
                let id: String = row.get(0)?;
                let session_id: String = row.get(1)?;
                let time_created: i64 = row.get(2)?;
                let data: String = row.get(3)?;
                Ok(serde_json::json!({
                    "id": id,
                    "session_id": session_id,
                    "time_created": time_created,
                    "data": parse_json_text_as_raw_value(&data, max_string_chars),
                }))
            },
        )
        .map_err(|e| format!("failed to query raw messages: {e}"))?;

    let mut records = Vec::new();
    let mut truncated = false;
    for (index, row) in rows.enumerate() {
        if index >= RAW_SQLITE_ROW_LIMIT {
            truncated = true;
            break;
        }
        records.push(SessionRawRecord {
            section: "message".into(),
            index: index + 1,
            value: row.map_err(|e| format!("failed to read raw message row: {e}"))?,
        });
    }

    Ok((records, truncated))
}

fn load_raw_part_rows(
    conn: &Connection,
    session_id: &str,
    max_string_chars: usize,
) -> Result<(Vec<SessionRawRecord>, bool), String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, message_id, time_created, data FROM part WHERE session_id = ?1 ORDER BY time_created ASC, id ASC LIMIT ?2",
        )
        .map_err(|e| format!("failed to prepare raw part query: {e}"))?;

    let rows = stmt
        .query_map(
            rusqlite::params![session_id, (RAW_SQLITE_ROW_LIMIT + 1) as i64],
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
                    "data": parse_json_text_as_raw_value(&data, max_string_chars),
                }))
            },
        )
        .map_err(|e| format!("failed to query raw parts: {e}"))?;

    let mut records = Vec::new();
    let mut truncated = false;
    for (index, row) in rows.enumerate() {
        if index >= RAW_SQLITE_ROW_LIMIT {
            truncated = true;
            break;
        }
        records.push(SessionRawRecord {
            section: "part".into(),
            index: index + 1,
            value: row.map_err(|e| format!("failed to read raw part row: {e}"))?,
        });
    }

    Ok((records, truncated))
}

fn parse_json_text_as_raw_value(text: &str, max_string_chars: usize) -> Value {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => truncate_raw_value(value, max_string_chars),
        Err(err) => serde_json::json!({
            "_parse_error": err.to_string(),
            "raw": truncate_raw_value(Value::String(text.to_string()), max_string_chars),
        }),
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
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

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
        let (records, truncated) =
            load_raw_records_with_truncation(&source, 10).expect("raw records");

        assert!(!truncated);
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].section, "session");
        assert_eq!(records[0].value["id"], "ses_1");
        assert_eq!(records[1].section, "message");
        assert_eq!(records[1].value["data"]["role"], "assistant");
        assert_eq!(records[1].value["data"]["extra"], "abcdefg...");
        assert_eq!(records[2].section, "part");
        assert_eq!(records[2].value["data"]["type"], "text");
    }

    #[test]
    fn load_raw_records_with_truncation_reports_message_row_limit() {
        let temp = tempdir().expect("tempdir");
        let db = temp.path().join("opencode.db");
        let conn = Connection::open(&db).expect("open");
        create_schema(&conn);

        conn.execute(
            "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5)",
            ("ses_1", "Raw", "/tmp/p", 1000_i64, 3000_i64),
        )
        .expect("insert session");

        for index in 0..(RAW_SQLITE_ROW_LIMIT + 1) {
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
        let (records, truncated) =
            load_raw_records_with_truncation(&source, 10).expect("raw records");

        assert!(truncated);
        assert_eq!(
            records
                .iter()
                .filter(|record| record.section == "message")
                .count(),
            RAW_SQLITE_ROW_LIMIT
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
