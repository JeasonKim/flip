pub mod claude;
pub mod codex;
pub mod opencode;

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

static FILE_SESSION_META_CACHE: OnceLock<Mutex<HashMap<FileSessionMetaCacheKey, SessionMeta>>> =
    OnceLock::new();
const TOOL_OUTPUT_RENDER_CHAR_LIMIT: usize = 12_000;
const TOOL_OUTPUT_PREVIEW_CHAR_LIMIT: usize = 4_000;
const LONG_USER_MESSAGE_RENDER_CHAR_LIMIT: usize = 40_000;
const LONG_ASSISTANT_MESSAGE_RENDER_CHAR_LIMIT: usize = 120_000;
const LONG_MESSAGE_PREVIEW_CHAR_LIMIT: usize = 2_000;
const LONG_MESSAGE_CODE_FENCE_LIMIT: usize = 80;
const RAW_STRING_PREVIEW_CHAR_LIMIT: usize = 500;
const RAW_STRING_HEAD_CHAR_LIMIT: usize = 260;
const RAW_STRING_TAIL_CHAR_LIMIT: usize = 120;
const DATA_IMAGE_PREFIX: &str = "data:image/";

/// 从文件 mtime 获取毫秒时间戳，用于排序和过滤
pub(crate) fn file_mtime_millis(path: &std::path::Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

pub(crate) fn file_source_revision(path: &Path) -> Option<SessionSourceRevision> {
    std::fs::metadata(path)
        .ok()
        .map(|metadata| SessionSourceRevision {
            size_bytes: Some(metadata.len()),
            modified_at: metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as i64),
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FileSessionMetaCacheKey {
    agent: String,
    source_path: String,
    size_bytes: Option<u64>,
    modified_at: Option<i64>,
}

pub(crate) fn cached_file_session_meta<F>(
    agent: &str,
    path: &Path,
    extract_meta: F,
) -> Option<SessionMeta>
where
    F: FnOnce() -> Option<SessionMeta>,
{
    let Some(revision) = file_source_revision(path) else {
        log::warn!(
            "[sessions] file metadata unavailable while caching agent={} source={}",
            agent,
            path.display()
        );
        return extract_meta();
    };

    let source_path = path.to_string_lossy().to_string();
    let cache_key = FileSessionMetaCacheKey {
        agent: agent.to_string(),
        source_path: source_path.clone(),
        size_bytes: revision.size_bytes,
        modified_at: revision.modified_at,
    };

    let cache = FILE_SESSION_META_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    match cache.lock() {
        Ok(cache_guard) => {
            if let Some(meta) = cache_guard.get(&cache_key) {
                return Some(meta.clone());
            }
        }
        Err(error) => {
            log::warn!(
                "[sessions] file meta cache read failed agent={} source={}: {}",
                agent,
                source_path,
                error
            );
        }
    }

    let meta = extract_meta()?;

    match cache.lock() {
        Ok(mut cache_guard) => {
            cache_guard.retain(|key, _| key.agent != agent || key.source_path != source_path);
            cache_guard.insert(cache_key, meta.clone());
        }
        Err(error) => {
            log::warn!(
                "[sessions] file meta cache write failed agent={} source={}: {}",
                agent,
                source_path,
                error
            );
        }
    }

    Some(meta)
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
pub struct SessionSourceRevision {
    pub size_bytes: Option<u64>,
    pub modified_at: Option<i64>,
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
    let messages = match agent {
        "claude" => claude::parse_messages(source_path),
        "codex" => codex::parse_messages(source_path),
        "opencode" => opencode::parse_messages(source_path),
        _ => Err(format!("unknown agent: {}", agent)),
    }?;

    Ok(compact_session_transcript(messages))
}

pub fn read_source_revision(
    agent: &str,
    source_path: &str,
) -> Result<SessionSourceRevision, String> {
    match agent {
        "claude" | "codex" => file_source_revision(Path::new(source_path))
            .ok_or_else(|| format!("failed to read source metadata: {source_path}")),
        "opencode" => opencode::source_revision(source_path),
        _ => Err(format!("unknown agent: {}", agent)),
    }
}

/// 加载单个会话的原始结构视图。
pub fn load_raw_content(agent: &str, source_path: &str) -> Result<SessionRawContent, String> {
    let records = match agent {
        "claude" | "codex" => load_jsonl_raw_records(source_path)?,
        "opencode" => opencode::load_raw_records(source_path)?,
        _ => return Err(format!("unknown agent: {}", agent)),
    };

    Ok(SessionRawContent {
        agent: agent.to_string(),
        source_path: source_path.to_string(),
        records,
        truncated: false,
    })
}

fn load_jsonl_raw_records(source_path: &str) -> Result<Vec<SessionRawRecord>, String> {
    let file = std::fs::File::open(source_path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for (line_index, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }

        let (section, value) = match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(value) => ("jsonl".to_string(), compact_raw_value(value)),
            Err(err) => (
                "jsonl_parse_error".to_string(),
                serde_json::json!({
                    "error": err.to_string(),
                    "raw": compact_raw_string(&line),
                }),
            ),
        };

        records.push(SessionRawRecord {
            section,
            index: line_index + 1,
            value,
        });
    }

    Ok(records)
}

pub(crate) fn compact_raw_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) => compact_raw_string(&text),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(compact_raw_value).collect())
        }
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key, compact_raw_value(value)))
                .collect(),
        ),
        other => other,
    }
}

fn compact_raw_string(text: &str) -> serde_json::Value {
    let char_count = text.chars().count();
    if let Some(image_summary) = summarize_data_image_string(text, char_count) {
        return serde_json::Value::String(image_summary);
    }

    if char_count <= RAW_STRING_PREVIEW_CHAR_LIMIT {
        return serde_json::Value::String(text.to_string());
    }

    let head: String = text.chars().take(RAW_STRING_HEAD_CHAR_LIMIT).collect();
    let tail_start = char_count.saturating_sub(RAW_STRING_TAIL_CHAR_LIMIT);
    let tail: String = text.chars().skip(tail_start).collect();

    serde_json::Value::String(format!(
        "[raw:{} omitted chars={}]\n{}\n\n... omitted {} chars ...\n\n{}",
        infer_raw_string_kind(text),
        char_count,
        head,
        char_count.saturating_sub(RAW_STRING_HEAD_CHAR_LIMIT + RAW_STRING_TAIL_CHAR_LIMIT),
        tail
    ))
}

fn summarize_data_image_string(text: &str, char_count: usize) -> Option<String> {
    let start = text.find(DATA_IMAGE_PREFIX)?;
    let after_prefix = &text[start + DATA_IMAGE_PREFIX.len()..];
    let mime_tail = after_prefix
        .split([';', ',', '"', '\''])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or("unknown");

    Some(format!(
        "[raw:image omitted mime=image/{mime_tail} encoding=base64 chars={char_count}]"
    ))
}

fn infer_raw_string_kind(text: &str) -> &'static str {
    let trimmed = text.trim_start();
    if trimmed.starts_with("<!doctype html") || trimmed.starts_with("<html") {
        return "html";
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return "json-string";
    }
    if text.contains("```") {
        return "markdown";
    }
    "string"
}

fn compact_session_transcript(messages: Vec<SessionMessage>) -> Vec<SessionMessage> {
    messages
        .into_iter()
        .map(|message| SessionMessage {
            content: compact_transcript_content(&message.role, &message.content),
            ..message
        })
        .collect()
}

fn compact_transcript_content(role: &str, content: &str) -> String {
    if role == "tool" {
        return compact_tool_transcript_content(role, content);
    }

    compact_long_reader_content(role, content)
}

fn compact_tool_transcript_content(role: &str, content: &str) -> String {
    if role != "tool" {
        return content.to_string();
    }

    let original_char_count = content.chars().count();
    let (content_without_images, omitted_image_count) = omit_embedded_image_payloads(content);
    let compacted_char_count = content_without_images.chars().count();

    if original_char_count <= TOOL_OUTPUT_RENDER_CHAR_LIMIT && omitted_image_count == 0 {
        return content.to_string();
    }

    let rendered_content = if compacted_char_count <= TOOL_OUTPUT_RENDER_CHAR_LIMIT {
        content_without_images
    } else {
        summarize_long_tool_output(&content_without_images, TOOL_OUTPUT_PREVIEW_CHAR_LIMIT)
    };

    let omitted_text = original_char_count.saturating_sub(rendered_content.chars().count());
    format!(
        "[Tool 输出已折叠]\n原始长度: {original_char_count} 字符\n省略内容: {omitted_text} 字符\n省略图片: {omitted_image_count} 个\n\n{rendered_content}"
    )
}

fn compact_long_reader_content(role: &str, content: &str) -> String {
    let original_char_count = content.chars().count();
    let code_fence_count = content.matches("```").count();
    let render_limit = match role {
        "assistant" => LONG_ASSISTANT_MESSAGE_RENDER_CHAR_LIMIT,
        _ => LONG_USER_MESSAGE_RENDER_CHAR_LIMIT,
    };

    if original_char_count <= render_limit
        && code_fence_count <= LONG_MESSAGE_CODE_FENCE_LIMIT
        && !content.contains(DATA_IMAGE_PREFIX)
    {
        return content.to_string();
    }

    let (content_without_images, omitted_image_count) = omit_embedded_image_payloads(content);
    let content_kind = infer_large_content_kind(&content_without_images);
    let preview =
        summarize_long_tool_output(&content_without_images, LONG_MESSAGE_PREVIEW_CHAR_LIMIT);
    let preview_char_count = preview.chars().count();
    let omitted_text = original_char_count.saturating_sub(preview_char_count);

    format!(
        "[长消息已折叠]\n角色: {role}\n类型: {content_kind}\n原始长度: {original_char_count} 字符\n代码围栏: {code_fence_count} 个\n省略内容: {omitted_text} 字符\n省略图片: {omitted_image_count} 个\n\n预览:\n{preview}"
    )
}

fn infer_large_content_kind(content: &str) -> &'static str {
    let trimmed = content.trim_start();
    if trimmed.starts_with("<!doctype html") || trimmed.starts_with("<html") {
        return "HTML";
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return "JSON/结构化文本";
    }
    if content.contains("```mermaid")
        || content.contains("graph TD")
        || content.contains("flowchart")
    {
        return "Markdown/Mermaid";
    }
    if content.contains("```") {
        return "Markdown/代码块";
    }
    "文本"
}

fn omit_embedded_image_payloads(content: &str) -> (String, usize) {
    let mut result = String::with_capacity(content.len().min(TOOL_OUTPUT_RENDER_CHAR_LIMIT));
    let mut cursor = 0;
    let mut omitted_count = 0;

    while let Some(relative_start) = content[cursor..].find(DATA_IMAGE_PREFIX) {
        let start = cursor + relative_start;
        result.push_str(&content[cursor..start]);

        let end = content[start..]
            .find('"')
            .map(|relative_end| start + relative_end)
            .unwrap_or(content.len());
        let omitted_chars = content[start..end].chars().count();
        result.push_str(&format!("[base64 image omitted: {omitted_chars} chars]"));
        omitted_count += 1;
        cursor = end;
    }

    result.push_str(&content[cursor..]);
    (result, omitted_count)
}

fn summarize_long_tool_output(content: &str, preview_chars: usize) -> String {
    let total_chars = content.chars().count();
    if total_chars <= preview_chars * 2 {
        return content.to_string();
    }

    let head: String = content.chars().take(preview_chars).collect();
    let tail_start = total_chars.saturating_sub(preview_chars);
    let tail: String = content.chars().skip(tail_start).collect();
    let omitted_chars = total_chars.saturating_sub(preview_chars * 2);

    format!("{head}\n\n... 已省略 {omitted_chars} 字符 ...\n\n{tail}")
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
    use std::sync::atomic::{AtomicUsize, Ordering};
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
    fn cached_file_session_meta_reuses_entry_until_source_revision_changes() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("session.jsonl");
        std::fs::write(&source, "first").expect("write first");
        let extract_count = AtomicUsize::new(0);

        let first = cached_file_session_meta("codex", &source, || {
            extract_count.fetch_add(1, Ordering::SeqCst);
            Some(SessionMeta {
                agent: "codex".into(),
                session_id: "s1".into(),
                title: "first".into(),
                project_dir: None,
                last_active_at: Some(1),
                source_path: source.to_string_lossy().to_string(),
                resume_command: None,
            })
        })
        .expect("first meta");

        let cached = cached_file_session_meta("codex", &source, || {
            extract_count.fetch_add(1, Ordering::SeqCst);
            Some(SessionMeta {
                agent: "codex".into(),
                session_id: "s1".into(),
                title: "stale".into(),
                project_dir: None,
                last_active_at: Some(1),
                source_path: source.to_string_lossy().to_string(),
                resume_command: None,
            })
        })
        .expect("cached meta");

        std::fs::write(&source, "second revision").expect("write second");
        let refreshed = cached_file_session_meta("codex", &source, || {
            extract_count.fetch_add(1, Ordering::SeqCst);
            Some(SessionMeta {
                agent: "codex".into(),
                session_id: "s1".into(),
                title: "second".into(),
                project_dir: None,
                last_active_at: Some(2),
                source_path: source.to_string_lossy().to_string(),
                resume_command: None,
            })
        })
        .expect("refreshed meta");

        assert_eq!(first.title, "first");
        assert_eq!(cached.title, "first");
        assert_eq!(refreshed.title, "second");
        assert_eq!(extract_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn compact_tool_transcript_content_keeps_short_tool_output() {
        assert_eq!(
            compact_tool_transcript_content("tool", "short output"),
            "short output"
        );
    }

    #[test]
    fn compact_tool_transcript_content_omits_embedded_base64_images() {
        let content = r#"[{"type":"input_image","image_url":"data:image/png;base64,abcdefghijklmnopqrstuvwxyz"}]"#;

        let compacted = compact_tool_transcript_content("tool", content);

        assert!(compacted.contains("[Tool 输出已折叠]"));
        assert!(compacted.contains("省略图片: 1 个"));
        assert!(compacted.contains("[base64 image omitted: 48 chars]"));
        assert!(!compacted.contains("abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn compact_tool_transcript_content_summarizes_long_tool_output() {
        let content = format!("{}{}{}", "a".repeat(7_000), "middle", "z".repeat(7_000));

        let compacted = compact_tool_transcript_content("tool", &content);

        assert!(compacted.contains("[Tool 输出已折叠]"));
        assert!(compacted.contains("... 已省略 "));
        assert!(compacted.contains(&"a".repeat(100)));
        assert!(compacted.contains(&"z".repeat(100)));
        assert!(!compacted.contains("middle"));
        assert!(compacted.chars().count() < content.chars().count());
    }

    #[test]
    fn compact_transcript_content_keeps_normal_markdown_and_mermaid_readable() {
        let content = "方案如下：\n\n```mermaid\ngraph TD\nA-->B\n```\n\n继续说明。";

        assert_eq!(compact_transcript_content("assistant", content), content);
    }

    #[test]
    fn compact_transcript_content_summarizes_huge_user_prompt() {
        let content = format!(
            "请基于下面资料生成页面\n{}{}",
            "```html\n<div>case</div>\n```\n".repeat(90),
            "正文".repeat(30_000)
        );

        let compacted = compact_transcript_content("user", &content);

        assert!(compacted.contains("[长消息已折叠]"));
        assert!(compacted.contains("角色: user"));
        assert!(compacted.contains("类型: Markdown/代码块"));
        assert!(compacted.contains("代码围栏: 180 个"));
        assert!(compacted.chars().count() < content.chars().count());
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
    fn load_raw_content_keeps_records_and_summarizes_long_raw_strings() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("long.jsonl");
        let long_text = "a".repeat(1400);
        let lines: Vec<String> = (0..85)
            .map(|index| {
                serde_json::json!({
                    "type": "message",
                    "index": index,
                    "content": long_text,
                })
                .to_string()
            })
            .collect();
        std::fs::write(&source, lines.join("\n")).expect("write");

        let raw = load_raw_content("codex", source.to_str().expect("path")).expect("raw");

        assert_eq!(raw.records.len(), 85);
        assert_eq!(raw.records[84].index, 85);
        let content = raw.records[0].value["content"].as_str().expect("string");
        assert!(content.starts_with("[raw:string omitted chars=1400]"));
        assert!(content.contains("... omitted "));
        assert!(content.contains("aaa"));
        assert!(!raw.truncated);
    }

    #[test]
    fn compact_raw_value_summarizes_embedded_image_payload() {
        let value = serde_json::json!({
            "image_url": "data:image/png;base64,abcdefghijklmnopqrstuvwxyz",
        });

        let compacted = compact_raw_value(value);

        assert_eq!(
            compacted["image_url"],
            "[raw:image omitted mime=image/png encoding=base64 chars=48]"
        );
    }

    #[test]
    fn compact_raw_value_preserves_object_and_array_shape() {
        let value = serde_json::json!({
            "items": [
                {
                    "kind": "html",
                    "value": format!("<html>{}</html>", "x".repeat(900)),
                }
            ],
            "enabled": true,
        });

        let compacted = compact_raw_value(value);

        assert!(compacted["items"].is_array());
        assert!(compacted["items"][0].is_object());
        assert_eq!(compacted["items"][0]["kind"], "html");
        assert_eq!(compacted["enabled"], true);
        assert!(compacted["items"][0]["value"]
            .as_str()
            .expect("string")
            .starts_with("[raw:html omitted chars="));
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
