pub mod claude;
pub mod codex;

use serde::{Deserialize, Serialize};

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String, // user, assistant, tool, system
    pub content: String,
    pub timestamp: Option<i64>,
}

/// 扫描会话列表，支持分页和筛选
pub fn scan_sessions(
    agent_filter: Option<&str>,
    project_filter: Option<&str>,
    offset: usize,
    limit: usize,
) -> Vec<SessionMeta> {
    let mut all = Vec::new();

    let scan_claude = agent_filter.is_none() || agent_filter == Some("claude");
    let scan_codex = agent_filter.is_none() || agent_filter == Some("codex");

    if scan_claude {
        all.extend(claude::scan_sessions());
    }
    if scan_codex {
        all.extend(codex::scan_sessions());
    }

    // 按项目筛选
    if let Some(project) = project_filter {
        all.retain(|s| {
            s.project_dir
                .as_deref()
                .map(|p| p.contains(project))
                .unwrap_or(false)
        });
    }

    // 按最后活跃时间排序（降序）
    all.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));

    // 分页
    all.into_iter().skip(offset).take(limit).collect()
}

/// 加载单个会话的完整消息
pub fn load_messages(agent: &str, source_path: &str) -> Result<Vec<SessionMessage>, String> {
    match agent {
        "claude" => claude::parse_messages(source_path),
        "codex" => codex::parse_messages(source_path),
        _ => Err(format!("unknown agent: {}", agent)),
    }
}

/// 去重提取所有项目目录
pub fn list_projects() -> Vec<String> {
    // Claude 项目目录可直接从目录名解码，无需解析文件
    let mut projects = claude::list_project_dirs();

    // Codex 项目目录在文件头部 session_meta 中，需要扫描
    let codex_sessions = codex::scan_sessions();
    projects.extend(codex_sessions.into_iter().filter_map(|s| s.project_dir));

    projects.sort();
    projects.dedup();
    projects
}

/// 删除指定天数之前的会话文件
pub fn purge_sessions(older_than_days: u32) -> usize {
    let now_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let cutoff = now_millis - (older_than_days as i64) * 24 * 60 * 60 * 1000;
    claude::purge_sessions(cutoff) + codex::purge_sessions(cutoff)
}
