pub mod claude;
pub mod codex;
pub mod opencode;
mod prompts;
pub mod sources;

pub use prompts::{load_system_prompts, SessionPrompt};

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::ops::Deref;
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
    #[serde(default)]
    pub source: Option<sources::SessionSourceRef>,
    pub project_dir: Option<String>,
    /// 最后活跃时间（毫秒时间戳）
    pub last_active_at: Option<i64>,
    /// Provider 原始会话的读取标识；文件型会话是路径，数据库型会话是内部定位串。
    pub source_path: String,
    /// 可以直接在文件管理器中定位的独立会话文件。
    pub source_file_path: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionTimelineCategory {
    User,
    Assistant,
    Reasoning,
    ToolCall,
    ToolResult,
    System,
    Metadata,
    Event,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionTimelineImportance {
    Primary,
    Detail,
    Debug,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRawReference {
    pub section: String,
    pub index: usize,
    pub record_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTimelineEntry {
    pub id: String,
    pub category: SessionTimelineCategory,
    pub importance: SessionTimelineImportance,
    pub title: String,
    pub role: String,
    pub content: String,
    pub timestamp: Option<i64>,
    pub raw_reference: SessionRawReference,
}

/// 各 agent 上报的口径并不一致，这里是统一后的业务口径：
/// input 只算未命中缓存的新增输入，output 含思考产出，thinking 是其中的思考部分。
/// 四项相加即本轮总消耗，跨 agent 可直接横向比较。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTokenUsage {
    pub total_tokens: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub thinking_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    /// 缓存写入的存活时长细分，目前只有 Claude 会上报
    pub cache_write_1h_tokens: Option<u64>,
    pub cache_write_5m_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionTimelineBlockKind {
    Context,
    Turn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionTurnStatus {
    Running,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTimelineBlock {
    pub id: String,
    pub kind: SessionTimelineBlockKind,
    pub ordinal: Option<usize>,
    pub status: Option<SessionTurnStatus>,
    pub usage: Option<SessionTokenUsage>,
    pub entry_start: usize,
    pub entry_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTimeline {
    pub blocks: Vec<SessionTimelineBlock>,
    pub entries: Vec<SessionTimelineEntry>,
}

impl Deref for SessionTimeline {
    type Target = [SessionTimelineEntry];

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TimelineRecordSemantics {
    pub starts_turn: bool,
    pub completes_turn: bool,
    pub usage_sample: Option<TimelineUsageSample>,
}

#[derive(Debug, Clone)]
pub(crate) struct TimelineUsageSample {
    pub deduplication_key: String,
    pub usage: SessionTokenUsage,
}

/// 扫描会话列表，支持分页和 agent 筛选
pub fn scan_sessions(
    agent_filter: Option<&str>,
    source_filter: Option<&str>,
    offset: usize,
    limit: usize,
) -> Result<Vec<SessionMeta>, String> {
    let mut all = Vec::new();

    let scan_claude = agent_filter.is_none() || agent_filter == Some("claude");
    let scan_codex = agent_filter.is_none() || agent_filter == Some("codex");
    let scan_opencode = agent_filter.is_none() || agent_filter == Some("opencode");

    if scan_claude {
        all.extend(claude::scan_sessions());
    }
    if scan_codex {
        all.extend(codex::scan_sessions()?);
    }
    if scan_opencode {
        all.extend(opencode::scan_sessions());
    }

    Ok(select_sessions(all, source_filter, offset, limit))
}

fn select_sessions(
    mut all: Vec<SessionMeta>,
    source_filter: Option<&str>,
    offset: usize,
    limit: usize,
) -> Vec<SessionMeta> {
    // 来源筛选先于排序与分页，确保目录里的历史记录都可以被检索到。
    all.retain(|session| {
        source_filter.map_or(true, |id| {
            session.source.as_ref().map(|source| source.id.as_str()) == Some(id)
        })
    });
    all.sort_by(|a, b| {
        b.last_active_at
            .cmp(&a.last_active_at)
            .then_with(|| a.source_path.cmp(&b.source_path))
    });
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

/// 将不同 Agent 的原始记录映射为统一时间线。
/// 每条源记录都必须产生一个条目，未知类型也只能降级成摘要，不能静默丢弃。
pub fn load_timeline_entries(agent: &str, source_path: &str) -> Result<SessionTimeline, String> {
    if agent == "opencode" {
        return opencode::load_timeline_entries(source_path);
    }

    let records = match agent {
        "claude" | "codex" => load_jsonl_raw_records_exact(source_path)?,
        _ => return Err(format!("unknown agent: {agent}")),
    };

    let semantics = match agent {
        "claude" => records
            .iter()
            .map(claude::interpret_timeline_record)
            .collect(),
        "codex" => codex::interpret_timeline_records(&records),
        _ => unreachable!("agent was validated before mapping"),
    };
    let mut entries: Vec<SessionTimelineEntry> = records
        .iter()
        .map(|record| match agent {
            "claude" => claude::represent_timeline_record(record),
            "codex" => codex::represent_timeline_record(record),
            _ => unreachable!("agent was validated before mapping"),
        })
        .collect();

    if agent == "codex" {
        codex::demote_duplicate_message_events(&records, &mut entries);
    }

    Ok(assemble_session_timeline(entries, semantics))
}

/// 只有一条会话对应一个独立文件时，才向产品层暴露“定位会话文件”能力。
pub fn session_source_file_path(agent: &str, source_path: &str) -> Option<std::path::PathBuf> {
    match agent {
        "claude" | "codex" if !source_path.trim().is_empty() => {
            Some(std::path::PathBuf::from(source_path))
        }
        _ => None,
    }
}

/// 将 Provider 侧的边界、完成信号和单次调用用量组装成统一 Turn。
/// 运行中的 Turn 保留已采集样本，但只在确定完成后对外暴露汇总，避免刷新时显示误导性的半成品。
pub(crate) fn assemble_session_timeline(
    entries: Vec<SessionTimelineEntry>,
    semantics: Vec<TimelineRecordSemantics>,
) -> SessionTimeline {
    debug_assert_eq!(entries.len(), semantics.len());
    let mut blocks = Vec::new();
    let mut current = TimelineBlockAccumulator::context();
    let mut turn_ordinal = 0;

    for (entry_index, signal) in semantics.into_iter().enumerate() {
        // 新用户输入会确定性封口上一 Turn，即使旧格式没有写入 Provider 的显式结束事件。
        if signal.starts_turn {
            if current.entry_start < entry_index {
                if current.kind == SessionTimelineBlockKind::Turn {
                    current.completed = true;
                }
                blocks.push(current.finish(entry_index, &entries));
            }
            turn_ordinal += 1;
            current = TimelineBlockAccumulator::turn(entry_index, turn_ordinal);
        }

        current.absorb(signal);
    }

    if current.entry_start < entries.len() {
        blocks.push(current.finish(entries.len(), &entries));
    }

    SessionTimeline { blocks, entries }
}

struct TimelineBlockAccumulator {
    kind: SessionTimelineBlockKind,
    entry_start: usize,
    ordinal: Option<usize>,
    completed: bool,
    usage_samples: HashMap<String, SessionTokenUsage>,
}

impl TimelineBlockAccumulator {
    fn context() -> Self {
        Self {
            kind: SessionTimelineBlockKind::Context,
            entry_start: 0,
            ordinal: None,
            completed: false,
            usage_samples: HashMap::new(),
        }
    }

    fn turn(entry_start: usize, ordinal: usize) -> Self {
        Self {
            kind: SessionTimelineBlockKind::Turn,
            entry_start,
            ordinal: Some(ordinal),
            completed: false,
            usage_samples: HashMap::new(),
        }
    }

    fn absorb(&mut self, signal: TimelineRecordSemantics) {
        self.completed |= signal.completes_turn;
        let Some(sample) = signal.usage_sample else {
            return;
        };
        // 同一调用的流式记录是用量快照；后续快照可能把普通输入重新归入缓存，必须整份替换。
        if let Some(previous) = self.usage_samples.get(&sample.deduplication_key) {
            if previous != &sample.usage {
                log::debug!(
                    "Replacing usage snapshot: key={}, previous={previous:?}, latest={:?}, reason=later snapshot for the same call",
                    sample.deduplication_key,
                    sample.usage,
                );
            }
        }
        self.usage_samples
            .insert(sample.deduplication_key, sample.usage);
    }

    fn finish(self, entry_end: usize, entries: &[SessionTimelineEntry]) -> SessionTimelineBlock {
        let first_entry_id = entries
            .get(self.entry_start)
            .map(|entry| entry.id.as_str())
            .unwrap_or("empty");
        let id = match self.ordinal {
            Some(ordinal) => format!("turn-{ordinal}-{first_entry_id}"),
            None => format!("context-{first_entry_id}"),
        };
        let status = match self.kind {
            SessionTimelineBlockKind::Context => None,
            SessionTimelineBlockKind::Turn if self.completed => Some(SessionTurnStatus::Completed),
            SessionTimelineBlockKind::Turn => Some(SessionTurnStatus::Running),
        };
        // 每次调用只汇总最终保留的快照；任一调用缺少完整总量时，不把已知部分冒充 Total。
        let mut usage = SessionTokenUsage::default();
        for sample in self.usage_samples.values() {
            usage.accumulate(sample);
        }
        if self
            .usage_samples
            .values()
            .any(|sample| sample.total_tokens.is_none())
        {
            if let Some(partial_total) = usage.total_tokens {
                log::debug!(
                    "Omitting partial token total: block={id}, previous={partial_total}, latest=unknown, reason=call with missing total"
                );
            }
            usage.total_tokens = None;
        }
        let usage = (self.completed && !usage.is_empty()).then_some(usage);

        SessionTimelineBlock {
            id,
            kind: self.kind,
            ordinal: self.ordinal,
            status,
            usage,
            entry_start: self.entry_start,
            entry_end,
        }
    }
}

impl SessionTokenUsage {
    fn accumulate(&mut self, sample: &Self) {
        accumulate_optional_tokens(&mut self.total_tokens, sample.total_tokens);
        accumulate_optional_tokens(&mut self.input_tokens, sample.input_tokens);
        accumulate_optional_tokens(&mut self.output_tokens, sample.output_tokens);
        accumulate_optional_tokens(&mut self.thinking_tokens, sample.thinking_tokens);
        accumulate_optional_tokens(&mut self.cache_read_tokens, sample.cache_read_tokens);
        accumulate_optional_tokens(&mut self.cache_write_tokens, sample.cache_write_tokens);
        accumulate_optional_tokens(
            &mut self.cache_write_1h_tokens,
            sample.cache_write_1h_tokens,
        );
        accumulate_optional_tokens(
            &mut self.cache_write_5m_tokens,
            sample.cache_write_5m_tokens,
        );
    }

    fn is_empty(&self) -> bool {
        self.total_tokens.is_none()
            && self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.thinking_tokens.is_none()
            && self.cache_read_tokens.is_none()
            && self.cache_write_tokens.is_none()
            && self.cache_write_1h_tokens.is_none()
            && self.cache_write_5m_tokens.is_none()
    }
}

fn accumulate_optional_tokens(total: &mut Option<u64>, sample: Option<u64>) {
    if let Some(sample) = sample {
        *total = Some(total.unwrap_or(0).saturating_add(sample));
    }
}

/// 按时间线条目的稳定引用读取真实原始对象，不经过摘要或字符串压缩。
pub fn load_entry_raw_value(
    agent: &str,
    source_path: &str,
    reference: &SessionRawReference,
) -> Result<serde_json::Value, String> {
    match agent {
        "claude" | "codex" => load_jsonl_raw_record_exact(source_path, reference.index),
        "opencode" => opencode::load_raw_record_exact(source_path, reference),
        _ => Err(format!("unknown agent: {agent}")),
    }
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
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(content_block_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn content_block_text(item: &serde_json::Value) -> Option<&str> {
    item.get("text")
        .or_else(|| item.get("input_text"))
        .or_else(|| item.get("output_text"))
        .and_then(serde_json::Value::as_str)
}

fn load_jsonl_raw_records(source_path: &str) -> Result<Vec<SessionRawRecord>, String> {
    Ok(load_jsonl_raw_records_exact(source_path)?
        .into_iter()
        .map(|record| SessionRawRecord {
            value: compact_raw_value(record.value),
            ..record
        })
        .collect())
}

fn load_jsonl_raw_records_exact(source_path: &str) -> Result<Vec<SessionRawRecord>, String> {
    let mut records = Vec::new();
    visit_jsonl_records_exact(source_path, |record| records.push(record))?;
    Ok(records)
}

fn visit_jsonl_records_exact(
    source_path: &str,
    mut visit: impl FnMut(SessionRawRecord),
) -> Result<(), String> {
    let file = std::fs::File::open(source_path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);

    for (line_index, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }

        let (section, value) = match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(value) => ("jsonl".to_string(), value),
            Err(err) => (
                "jsonl_parse_error".to_string(),
                serde_json::json!({
                    "error": err.to_string(),
                    "raw": line,
                }),
            ),
        };

        visit(SessionRawRecord {
            section,
            index: line_index + 1,
            value,
        });
    }

    Ok(())
}

fn load_jsonl_raw_record_exact(
    source_path: &str,
    target_index: usize,
) -> Result<serde_json::Value, String> {
    if target_index == 0 {
        return Err("raw record index must start from 1".into());
    }

    let file = std::fs::File::open(source_path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    let line = reader
        .lines()
        .nth(target_index - 1)
        .ok_or_else(|| format!("raw record not found at line {target_index}"))?
        .map_err(|e| e.to_string())?;

    match serde_json::from_str::<serde_json::Value>(&line) {
        Ok(value) => Ok(value),
        Err(error) => Ok(serde_json::json!({
            "error": error.to_string(),
            "raw": line,
        })),
    }
}

pub(crate) fn represent_timeline_entry(
    record: &SessionRawRecord,
    category: SessionTimelineCategory,
    importance: SessionTimelineImportance,
    title: String,
    role: String,
    content: String,
    timestamp: Option<i64>,
) -> SessionTimelineEntry {
    let record_id = extract_raw_record_id(&record.value);
    let stable_suffix = record_id
        .as_deref()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| record.index.to_string());

    SessionTimelineEntry {
        id: format!("{}-{stable_suffix}", record.section),
        category,
        importance,
        title,
        role,
        content,
        timestamp,
        raw_reference: SessionRawReference {
            section: record.section.clone(),
            index: record.index,
            record_id,
        },
    }
}

pub(crate) fn summarize_timeline_value(value: &serde_json::Value, limit: usize) -> String {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|error| {
        log::warn!("[sessions] failed to serialize timeline preview: {error}");
        value.to_string()
    });
    summarize_timeline_text(&text, limit)
}

pub(crate) fn summarize_timeline_text(text: &str, limit: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= limit {
        return text.to_string();
    }

    let preview: String = text.chars().take(limit).collect();
    format!(
        "{preview}\n… 另有 {} 个字符，点击“原始数据”查看",
        char_count - limit
    )
}

pub(crate) fn describe_embedded_data(value: &serde_json::Value) -> Option<String> {
    let object = value.as_object()?;
    let source = object.get("source").unwrap_or(value);
    let media_type = source
        .get("media_type")
        .or_else(|| source.get("mediaType"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let data_chars = source
        .get("data")
        .and_then(serde_json::Value::as_str)
        .map(|data| data.chars().count());

    Some(match data_chars {
        Some(char_count) => format!("[内嵌数据 {media_type}，{char_count} 个字符]"),
        None => format!("[内嵌数据 {media_type}]"),
    })
}

fn extract_raw_record_id(value: &serde_json::Value) -> Option<String> {
    value
        .get("id")
        .or_else(|| value.get("data").and_then(|data| data.get("id")))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

pub(crate) fn timeline_section_priority(section: &str) -> u8 {
    match section {
        "session" => 0,
        "message" => 1,
        "part" => 2,
        _ => 3,
    }
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
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::tempdir;

    #[test]
    fn source_filter_is_applied_before_pagination() {
        let sessions: Vec<SessionMeta> = (0..5).map(|index| SessionMeta {
            agent: "codex".into(), session_id: index.to_string(), title: "会话".into(),
            source: Some(sources::SessionSourceRef {
                id: if index < 3 { "native" } else { "dev" }.into(), label: "来源".into(),
            }),
            project_dir: None, last_active_at: Some(5 - index),
            source_path: format!("/tmp/{index}.jsonl"), source_file_path: None, resume_command: None,
        }).collect();
        let page = select_sessions(sessions, Some("dev"), 1, 1);
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].session_id, "4");
    }

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
                source: None,
                project_dir: None,
                last_active_at: Some(1),
                source_path: source.to_string_lossy().to_string(),
                source_file_path: Some(source.to_string_lossy().to_string()),
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
                source: None,
                project_dir: None,
                last_active_at: Some(1),
                source_path: source.to_string_lossy().to_string(),
                source_file_path: Some(source.to_string_lossy().to_string()),
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
                source: None,
                project_dir: None,
                last_active_at: Some(2),
                source_path: source.to_string_lossy().to_string(),
                source_file_path: Some(source.to_string_lossy().to_string()),
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

    #[test]
    fn session_timeline_keeps_metadata_unknown_blocks_and_tool_records() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("claude.jsonl");
        std::fs::write(
            &source,
            [
                r#"{"type":"system","subtype":"init","cwd":"/tmp/project","model":"claude-test"}"#,
                r#"{"type":"assistant","timestamp":"2026-08-18T08:00:00Z","message":{"role":"assistant","content":[{"type":"text","text":"先看结果"},{"type":"server_tool_result","payload":{"answer":42}}]}}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/main.rs","offset":10}}]}}"#,
            ]
            .join("\n"),
        )
        .expect("write");

        let entries =
            load_timeline_entries("claude", source.to_str().expect("path")).expect("timeline");

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].category, SessionTimelineCategory::Metadata);
        assert_eq!(entries[0].importance, SessionTimelineImportance::Debug);
        assert_eq!(entries[0].raw_reference.index, 1);
        assert_eq!(entries[1].category, SessionTimelineCategory::Assistant);
        assert_eq!(entries[1].importance, SessionTimelineImportance::Primary);
        assert!(entries[1].content.contains("先看结果"));
        assert!(entries[1].content.contains("server_tool_result"));
        assert_eq!(entries[2].category, SessionTimelineCategory::ToolCall);
        assert_eq!(entries[2].importance, SessionTimelineImportance::Detail);
        assert!(entries[2].title.contains("Read"));
        assert!(entries[2].content.contains("src/main.rs"));
    }

    #[test]
    fn claude_timeline_keeps_failed_tool_results_as_collapsed_details() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("claude-error.jsonl");
        std::fs::write(
            &source,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"content":"permission denied"}]}}"#,
        )
        .expect("write");

        let entries =
            load_timeline_entries("claude", source.to_str().expect("path")).expect("timeline");

        assert_eq!(entries[0].category, SessionTimelineCategory::ToolResult);
        assert_eq!(entries[0].importance, SessionTimelineImportance::Detail);
        assert!(entries[0].content.contains("permission denied"));
    }

    #[test]
    fn session_timeline_keeps_unknown_codex_events() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("codex.jsonl");
        std::fs::write(
            &source,
            [
                r#"{"type":"session_meta","payload":{"id":"s1","cwd":"/tmp/project"}}"#,
                r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":99}}}}"#,
                r#"{"type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"检查实现"}]}}"#,
            ]
            .join("\n"),
        )
        .expect("write");

        let entries =
            load_timeline_entries("codex", source.to_str().expect("path")).expect("timeline");

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].category, SessionTimelineCategory::Metadata);
        assert_eq!(entries[0].importance, SessionTimelineImportance::Debug);
        assert_eq!(entries[1].category, SessionTimelineCategory::Event);
        assert_eq!(entries[1].importance, SessionTimelineImportance::Debug);
        assert!(entries[1].title.contains("token_count"));
        assert_eq!(entries[2].category, SessionTimelineCategory::Reasoning);
        assert_eq!(entries[2].importance, SessionTimelineImportance::Detail);
        assert!(entries[2].content.contains("检查实现"));
    }

    #[test]
    fn codex_timeline_keeps_legacy_messages_primary_but_demotes_duplicate_events() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("codex-duplicates.jsonl");
        std::fs::write(
            &source,
            [
                r#"{"timestamp":"2026-08-18T08:00:00Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"完成"}]}}"#,
                r#"{"timestamp":"2026-08-18T08:00:00Z","type":"event_msg","payload":{"type":"agent_message","message":"完成"}}"#,
                r#"{"timestamp":"2026-08-18T08:00:01Z","type":"event_msg","payload":{"type":"warning","message":"需要用户注意"}}"#,
                r#"{"timestamp":"2026-08-18T08:00:02Z","type":"event_msg","payload":{"type":"tool_error","message":"工具失败"}}"#,
            ]
            .join("\n"),
        )
        .expect("write");

        let entries =
            load_timeline_entries("codex", source.to_str().expect("path")).expect("timeline");

        assert_eq!(entries[0].importance, SessionTimelineImportance::Primary);
        assert_eq!(entries[1].importance, SessionTimelineImportance::Debug);
        assert_eq!(entries[2].importance, SessionTimelineImportance::Primary);
        assert_eq!(entries[3].importance, SessionTimelineImportance::Detail);
    }

    #[test]
    fn only_file_backed_sessions_expose_a_source_file() {
        assert_eq!(
            session_source_file_path("claude", "/tmp/claude.jsonl"),
            Some(std::path::PathBuf::from("/tmp/claude.jsonl"))
        );
        assert_eq!(
            session_source_file_path("codex", "/tmp/codex.jsonl"),
            Some(std::path::PathBuf::from("/tmp/codex.jsonl"))
        );
        assert_eq!(
            session_source_file_path("opencode", "sqlite:/tmp/opencode.db:ses_1"),
            None
        );
    }

    #[test]
    fn claude_turn_deduplicates_split_message_usage_and_hides_running_totals() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("claude-usage.jsonl");
        std::fs::write(
            &source,
            [
                r#"{"type":"system","subtype":"init"}"#,
                r#"{"type":"user","message":{"role":"user","content":"开始"}}"#,
                r#"{"type":"assistant","message":{"id":"msg_tool","role":"assistant","stop_reason":"tool_use","usage":{"input_tokens":100,"output_tokens":10,"output_tokens_details":{"thinking_tokens":6},"cache_read_input_tokens":80,"cache_creation_input_tokens":5,"cache_creation":{"ephemeral_1h_input_tokens":5,"ephemeral_5m_input_tokens":0}},"content":[{"type":"thinking","thinking":"检查"}]}}"#,
                r#"{"type":"assistant","message":{"id":"msg_tool","role":"assistant","stop_reason":"tool_use","usage":{"input_tokens":100,"output_tokens":10,"output_tokens_details":{"thinking_tokens":6},"cache_read_input_tokens":80,"cache_creation_input_tokens":5,"cache_creation":{"ephemeral_1h_input_tokens":5,"ephemeral_5m_input_tokens":0}},"content":[{"type":"tool_use","name":"Read","input":{}}]}}"#,
            ]
            .join("\n"),
        )
        .expect("write");

        let running =
            load_timeline_entries("claude", source.to_str().expect("path")).expect("timeline");
        assert_eq!(running.blocks.len(), 2);
        assert_eq!(running.blocks[1].status, Some(SessionTurnStatus::Running));
        assert_eq!(running.blocks[1].usage, None);

        std::fs::OpenOptions::new()
            .append(true)
            .open(&source)
            .expect("open")
            .write_all(
                r#"
{"type":"user","toolUseResult":{"ok":true},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call_1","content":"ok"}]}}
{"type":"assistant","message":{"id":"msg_final","role":"assistant","stop_reason":"end_turn","usage":{"input_tokens":50,"output_tokens":20,"output_tokens_details":{"thinking_tokens":4},"cache_read_input_tokens":40,"cache_creation_input_tokens":0,"cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":0}},"content":[{"type":"text","text":"完成"}]}}"#.as_bytes(),
            )
            .expect("append");

        let completed =
            load_timeline_entries("claude", source.to_str().expect("path")).expect("timeline");
        assert_eq!(completed.blocks.len(), 2);
        assert_eq!(
            completed.blocks[1].status,
            Some(SessionTurnStatus::Completed)
        );
        assert_eq!(
            completed.blocks[1].usage,
            Some(SessionTokenUsage {
                total_tokens: Some(305),
                input_tokens: Some(150),
                output_tokens: Some(30),
                thinking_tokens: Some(10),
                cache_read_tokens: Some(120),
                cache_write_tokens: Some(5),
                cache_write_1h_tokens: Some(5),
                cache_write_5m_tokens: Some(0),
            })
        );
        assert_eq!(
            completed.blocks[1].entry_end - completed.blocks[1].entry_start,
            5
        );
    }

    #[test]
    fn claude_turn_replaces_usage_snapshots_instead_of_keeping_first_or_maximum() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("claude-updated-usage.jsonl");
        std::fs::write(
            &source,
            [
                r#"{"type":"user","message":{"role":"user","content":"开始"}}"#,
                r#"{"type":"assistant","message":{"id":"msg_tool","role":"assistant","stop_reason":null,"usage":{"input_tokens":18417,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
                r#"{"type":"assistant","message":{"id":"msg_tool","role":"assistant","stop_reason":null,"usage":{"input_tokens":6,"output_tokens":50,"cache_read_input_tokens":0,"cache_creation_input_tokens":23645}}}"#,
                r#"{"type":"assistant","message":{"id":"msg_tool","role":"assistant","stop_reason":"tool_use","usage":{"input_tokens":6,"output_tokens":877,"cache_read_input_tokens":0,"cache_creation_input_tokens":23645}}}"#,
                r#"{"type":"assistant","message":{"id":"msg_tool","role":"assistant","stop_reason":"tool_use","usage":{"input_tokens":6,"output_tokens":877,"cache_read_input_tokens":0,"cache_creation_input_tokens":23645}}}"#,
                r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call_1","content":"ok"}]}}"#,
                r#"{"type":"assistant","message":{"id":"msg_final","role":"assistant","stop_reason":null,"usage":{"input_tokens":50,"output_tokens":0,"cache_read_input_tokens":40,"cache_creation_input_tokens":0}}}"#,
                r#"{"type":"assistant","message":{"id":"msg_final","role":"assistant","stop_reason":"end_turn","usage":{"input_tokens":50,"output_tokens":20,"cache_read_input_tokens":40,"cache_creation_input_tokens":0}}}"#,
                r#"{"type":"cost-state","modelUsage":{"side-call":{"inputTokens":818,"outputTokens":285}}}"#,
            ]
            .join("\n"),
        )
        .expect("write");

        let timeline =
            load_timeline_entries("claude", source.to_str().expect("path")).expect("timeline");

        assert_eq!(timeline.blocks.len(), 1);
        assert_eq!(timeline.blocks[0].status, Some(SessionTurnStatus::Completed));
        assert_eq!(
            timeline.blocks[0].usage,
            Some(SessionTokenUsage {
                total_tokens: Some(24_638),
                input_tokens: Some(56),
                output_tokens: Some(897),
                cache_read_tokens: Some(40),
                cache_write_tokens: Some(23_645),
                ..SessionTokenUsage::default()
            })
        );
    }

    #[test]
    fn claude_turn_does_not_report_a_partial_sum_as_total() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("claude-incomplete-usage.jsonl");
        std::fs::write(
            &source,
            [
                r#"{"type":"user","message":{"role":"user","content":"开始"}}"#,
                r#"{"type":"assistant","message":{"id":"msg_tool","role":"assistant","stop_reason":"tool_use","usage":{"input_tokens":100,"output_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
                r#"{"type":"assistant","message":{"id":"msg_final","role":"assistant","stop_reason":"end_turn","usage":{"input_tokens":50,"output_tokens":20}}}"#,
            ]
            .join("\n"),
        )
        .expect("write");

        let timeline =
            load_timeline_entries("claude", source.to_str().expect("path")).expect("timeline");
        let usage = timeline.blocks[0].usage.as_ref().expect("usage");

        assert_eq!(usage.input_tokens, Some(150));
        assert_eq!(usage.output_tokens, Some(30));
        assert_eq!(usage.total_tokens, None);
    }

    #[test]
    fn codex_turn_sums_distinct_model_calls_and_waits_for_task_complete() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("codex-usage.jsonl");
        let first_usage = r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"total_tokens":10,"input_tokens":8,"output_tokens":2,"reasoning_output_tokens":1,"cached_input_tokens":5,"cache_write_input_tokens":0},"total_token_usage":{"total_tokens":110,"input_tokens":88,"output_tokens":22,"cached_input_tokens":55,"cache_write_input_tokens":0}}}}"#;
        std::fs::write(
            &source,
            [
                r#"{"type":"session_meta","payload":{"id":"s1"}}"#,
                r#"{"type":"event_msg","payload":{"type":"task_started"}}"#,
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"开始"}]}}"#,
                first_usage,
                first_usage,
                r#"{"type":"turn_context","payload":{"turn_id":"internal-continuation"}}"#,
                r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"total_tokens":20,"input_tokens":15,"output_tokens":5,"reasoning_output_tokens":2,"cached_input_tokens":10,"cache_write_input_tokens":0},"total_token_usage":{"total_tokens":130,"input_tokens":103,"output_tokens":27,"cached_input_tokens":65,"cache_write_input_tokens":0}}}}"#,
            ]
            .join("\n"),
        )
        .expect("write");

        let running =
            load_timeline_entries("codex", source.to_str().expect("path")).expect("timeline");
        assert_eq!(running.blocks.len(), 2);
        assert_eq!(running.blocks[1].status, Some(SessionTurnStatus::Running));
        assert_eq!(running.blocks[1].usage, None);

        std::fs::OpenOptions::new()
            .append(true)
            .open(&source)
            .expect("open")
            .write_all(
                br#"
{"type":"event_msg","payload":{"type":"task_complete"}}"#,
            )
            .expect("append");

        let completed =
            load_timeline_entries("codex", source.to_str().expect("path")).expect("timeline");
        assert_eq!(
            completed.blocks[1].status,
            Some(SessionTurnStatus::Completed)
        );
        assert_eq!(
            completed.blocks[1].usage,
            Some(SessionTokenUsage {
                total_tokens: Some(30),
                // Codex 上报 8 和 15 的输入里各自含 5 和 10 的缓存命中，统一口径只留新增部分
                input_tokens: Some(8),
                output_tokens: Some(7),
                thinking_tokens: Some(3),
                cache_read_tokens: Some(15),
                cache_write_tokens: Some(0),
                cache_write_1h_tokens: None,
                cache_write_5m_tokens: None,
            })
        );
    }

    #[test]
    fn session_entry_raw_value_is_loaded_without_compaction() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("raw.jsonl");
        let original = "x".repeat(1_400);
        std::fs::write(
            &source,
            serde_json::json!({"type": "event", "payload": original}).to_string(),
        )
        .expect("write");

        let raw = load_entry_raw_value(
            "codex",
            source.to_str().expect("path"),
            &SessionRawReference {
                section: "jsonl".into(),
                index: 1,
                record_id: None,
            },
        )
        .expect("raw value");

        assert_eq!(raw["payload"].as_str().expect("payload").len(), 1_400);
        assert!(!raw["payload"]
            .as_str()
            .expect("payload")
            .contains("omitted"));
    }
}
