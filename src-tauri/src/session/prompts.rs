use std::collections::HashSet;

use serde::Serialize;
use serde_json::Value;

use super::{
    content_block_text, extract_content_text, extract_raw_record_id, opencode,
    visit_jsonl_records_exact, SessionRawRecord, SessionRawReference,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPromptRole {
    System,
    Developer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPromptOrigin {
    Configured,
    Request,
    Model,
    Runtime,
    Snapshot,
    Unknown,
}

#[derive(Serialize)]
pub struct SessionPrompt {
    pub role: SessionPromptRole,
    pub origin: SessionPromptOrigin,
    pub text: String,
    pub entrypoint: Option<String>,
    pub source: SessionRawReference,
}

struct RecordedPrompt {
    role: SessionPromptRole,
    origin: SessionPromptOrigin,
    text: String,
}

type PromptRecords = fn(&Value) -> Option<Vec<RecordedPrompt>>;

pub fn load_system_prompts(agent: &str, source_path: &str) -> Result<Vec<SessionPrompt>, String> {
    let prompt_records: PromptRecords = match agent {
        "codex" => codex_prompt_records,
        "claude" => claude_prompt_records,
        "opencode" => opencode_prompt_records,
        _ => return Err(format!("unknown agent: {agent}")),
    };

    let mut prompts = Vec::new();
    let mut seen = HashSet::new();
    let record_prompts = |record: SessionRawRecord| {
        let Some(records) = prompt_records(&record.value) else {
            return;
        };
        for RecordedPrompt { role, origin, text } in records {
            if text.trim().is_empty() || !seen.insert((role, origin, text.clone())) {
                continue;
            }
            prompts.push(SessionPrompt {
                role,
                origin,
                text,
                entrypoint: record
                    .value
                    .get("entrypoint")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                source: SessionRawReference {
                    section: record.section.clone(),
                    index: record.index,
                    record_id: extract_raw_record_id(&record.value),
                },
            });
        }
    };

    if agent == "opencode" {
        opencode::load_raw_session_message_rows(source_path)?
            .into_iter()
            .for_each(record_prompts);
    } else {
        visit_jsonl_records_exact(source_path, record_prompts)?;
    }
    Ok(prompts)
}

fn codex_prompt_records(record: &Value) -> Option<Vec<RecordedPrompt>> {
    let payload = record.get("payload")?;
    match record.get("type")?.as_str()? {
        "session_meta" => {
            let instructions = payload.get("base_instructions")?;
            let origin = match instructions
                .pointer("/provenance/type")
                .and_then(Value::as_str)
            {
                Some("custom") => SessionPromptOrigin::Configured,
                Some("model") => SessionPromptOrigin::Model,
                _ => SessionPromptOrigin::Unknown,
            };
            Some(vec![RecordedPrompt {
                role: SessionPromptRole::System,
                origin,
                text: extract_content_text(Some(instructions.get("text").unwrap_or(instructions))),
            }])
        }
        "response_item" if payload.get("type")?.as_str()? == "message" => {
            let role = prompt_role(payload)?;
            let content = payload.get("content")?;
            let metadata = payload
                .pointer("/internal_chat_message_metadata_passthrough/content_item_metadata")
                .and_then(Value::as_array);
            match (content.as_array(), metadata) {
                (Some(parts), Some(metadata)) => {
                    Some(codex_attributed_fragments(role, parts, metadata))
                }
                _ => Some(vec![unattributed_message(role, content)]),
            }
        }
        _ => None,
    }
}

fn codex_attributed_fragments(
    role: SessionPromptRole,
    parts: &[Value],
    metadata: &[Value],
) -> Vec<RecordedPrompt> {
    let mut prompts: Vec<RecordedPrompt> = Vec::new();

    // 来源与正文按数组下标对应，同一来源的片段在同一页签中按原顺序展示。
    for (index, part) in parts.iter().enumerate() {
        let Some(text) = content_block_text(part).filter(|text| !text.is_empty()) else {
            continue;
        };
        let origin = codex_fragment_origin(metadata.get(index));
        if let Some(prompt) = prompts.iter_mut().find(|prompt| prompt.origin == origin) {
            prompt.text.push('\n');
            prompt.text.push_str(text);
        } else {
            prompts.push(RecordedPrompt {
                role,
                origin,
                text: text.to_owned(),
            });
        }
    }

    prompts
}

fn codex_fragment_origin(metadata: Option<&Value>) -> SessionPromptOrigin {
    let Some(metadata) = metadata else {
        return SessionPromptOrigin::Unknown;
    };
    let provenance = metadata.get("provenance").unwrap_or(&Value::Null);
    match provenance.get("type").and_then(Value::as_str) {
        Some("developer_instructions") => {
            match provenance
                .get("from_additional_requirements")
                .and_then(Value::as_bool)
            {
                Some(false) => SessionPromptOrigin::Configured,
                Some(true) => SessionPromptOrigin::Runtime,
                None => SessionPromptOrigin::Unknown,
            }
        }
        Some("harness" | "skills" | "agents_md" | "tool") => SessionPromptOrigin::Runtime,
        None if metadata.get("harness_injected").and_then(Value::as_bool) == Some(true) => {
            SessionPromptOrigin::Runtime
        }
        _ => SessionPromptOrigin::Unknown,
    }
}

fn claude_prompt_records(record: &Value) -> Option<Vec<RecordedPrompt>> {
    let prompt = match record.get("type")?.as_str()? {
        "api-request-shape" => {
            unattributed_message(SessionPromptRole::System, record.pointer("/shape/system")?)
        }
        "attachment" => {
            let attachment = record.get("attachment")?;
            if attachment.get("type")?.as_str()? != "prompt_snapshot" {
                return None;
            }
            let prompt = attachment.get("systemPrompt")?;
            let text = match prompt {
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                _ => extract_content_text(Some(prompt)),
            };
            RecordedPrompt {
                role: SessionPromptRole::System,
                origin: SessionPromptOrigin::Snapshot,
                text,
            }
        }
        _ => {
            let message = record.get("message")?;
            unattributed_message(prompt_role(message)?, message.get("content")?)
        }
    };
    Some(vec![prompt])
}

fn opencode_prompt_records(record: &Value) -> Option<Vec<RecordedPrompt>> {
    let data = record.get("data")?;
    let prompt = if data.get("role")?.as_str()? == "user" {
        RecordedPrompt {
            role: SessionPromptRole::System,
            origin: SessionPromptOrigin::Request,
            text: extract_content_text(Some(data.get("system")?)),
        }
    } else {
        unattributed_message(prompt_role(data)?, data.get("content")?)
    };
    Some(vec![prompt])
}

fn prompt_role(message: &Value) -> Option<SessionPromptRole> {
    match message.get("role")?.as_str()? {
        "system" => Some(SessionPromptRole::System),
        "developer" => Some(SessionPromptRole::Developer),
        _ => None,
    }
}

fn unattributed_message(role: SessionPromptRole, content: &Value) -> RecordedPrompt {
    RecordedPrompt {
        role,
        origin: SessionPromptOrigin::Unknown,
        text: extract_content_text(Some(content)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_codex_prompts_with_distinct_roles_and_preserves_long_text() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("session.jsonl");
        let base = format!("  系统规则\n{}\n", "原文".repeat(12_000));
        let records = [
            serde_json::json!({"type":"session_meta","payload":{"base_instructions":{"text":base,"provenance":{"type":"model","model":"test-model"}}}}),
            serde_json::json!({"type":"session_meta","payload":{"base_instructions":"early CLI format"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":"ordinary request"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"system","content":"system text"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"developer text"},{"type":"input_image","image_url":"example"},{"type":"input_text","text":"another block"}]}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"developer","content":"developer text\nanother block"}}),
        ];
        std::fs::write(&path, records.map(|record| record.to_string()).join("\n")).unwrap();

        let prompts = load_system_prompts("codex", path.to_str().unwrap()).unwrap();
        let actual: Vec<_> = prompts
            .iter()
            .map(|prompt| {
                (
                    prompt.role,
                    prompt.origin,
                    prompt.text.as_str(),
                    prompt.source.index,
                )
            })
            .collect();
        assert_eq!(
            actual,
            [
                (
                    SessionPromptRole::System,
                    SessionPromptOrigin::Model,
                    base.as_str(),
                    1
                ),
                (
                    SessionPromptRole::System,
                    SessionPromptOrigin::Unknown,
                    "early CLI format",
                    2
                ),
                (
                    SessionPromptRole::System,
                    SessionPromptOrigin::Unknown,
                    "system text",
                    4
                ),
                (
                    SessionPromptRole::Developer,
                    SessionPromptOrigin::Unknown,
                    "developer text\nanother block",
                    5
                ),
            ]
        );
    }

    #[test]
    fn classifies_configured_codex_text_separately_from_runtime_and_untagged_text() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("session.jsonl");
        std::fs::write(&path, concat!(
            r#"{"type":"session_meta","payload":{"base_instructions":{"text":"custom base","provenance":{"type":"custom"}}}}"#, "\n",
            r#"{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"text":"custom developer"},{"text":"built-in tools"},{"text":"managed instruction"},{"text":"unattributed part"}],"internal_chat_message_metadata_passthrough":{"content_item_metadata":[{"provenance":{"type":"developer_instructions","from_additional_requirements":false}},{"provenance":{"type":"harness","hook":false}},{"provenance":{"type":"developer_instructions","from_additional_requirements":true}}]}}}"#, "\n",
        )).unwrap();

        let prompts = load_system_prompts("codex", path.to_str().unwrap()).unwrap();
        let actual: Vec<_> = prompts
            .iter()
            .map(|prompt| {
                (
                    prompt.role,
                    prompt.origin,
                    prompt.text.as_str(),
                    prompt.source.index,
                )
            })
            .collect();
        assert_eq!(
            actual,
            [
                (
                    SessionPromptRole::System,
                    SessionPromptOrigin::Configured,
                    "custom base",
                    1
                ),
                (
                    SessionPromptRole::Developer,
                    SessionPromptOrigin::Configured,
                    "custom developer",
                    2
                ),
                (
                    SessionPromptRole::Developer,
                    SessionPromptOrigin::Runtime,
                    "built-in tools\nmanaged instruction",
                    2
                ),
                (
                    SessionPromptRole::Developer,
                    SessionPromptOrigin::Unknown,
                    "unattributed part",
                    2
                ),
            ]
        );
    }

    #[test]
    fn loads_claude_request_prompts_and_ignores_system_events() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"type":"system","subtype":"init","content":"initialization"}"#, "\n",
                r#"{"type":"api-request-shape","shape":{"system":[{"type":"text","text":"first block"},{"type":"text","text":"second block"}],"tools":[]}}"#, "\n",
                r#"{"type":"api-request-blob","message":{"role":"system","content":[{"type":"text","text":"injected system message"}]}}"#, "\n",
                r#"{"type":"api-request-blob","message":{"role":"user","content":"ordinary request"}}"#, "\n",
                r#"{"type":"api-request-shape","shape":{"system":"   "}}"#, "\n",
                r#"{"type":"attachment","entrypoint":"sdk-ts","attachment":{"type":"prompt_snapshot","systemPrompt":["  first snapshot section","last snapshot section\n"],"cliPrefix":"transport metadata"}}"#, "\n",
                r#"{"type":"attachment","attachment":{"type":"prompt_snapshot","systemPrompt":["  first snapshot section","last snapshot section\n"]}}"#, "\n",
                "{incomplete"
            ),
        )
        .unwrap();

        let prompts = load_system_prompts("claude", path.to_str().unwrap()).unwrap();
        let actual: Vec<_> = prompts
            .iter()
            .map(|prompt| {
                (
                    prompt.role,
                    prompt.origin,
                    prompt.text.as_str(),
                    prompt.source.index,
                )
            })
            .collect();
        assert_eq!(
            actual,
            [
                (
                    SessionPromptRole::System,
                    SessionPromptOrigin::Unknown,
                    "first block\nsecond block",
                    2
                ),
                (
                    SessionPromptRole::System,
                    SessionPromptOrigin::Unknown,
                    "injected system message",
                    3
                ),
                (
                    SessionPromptRole::System,
                    SessionPromptOrigin::Snapshot,
                    "  first snapshot section\n\nlast snapshot section\n",
                    6,
                ),
            ]
        );
        assert_eq!(prompts[2].entrypoint.as_deref(), Some("sdk-ts"));
    }

    #[test]
    fn loads_only_the_selected_opencode_sessions_recorded_prompt() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("opencode.db");
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, data TEXT);
                 INSERT INTO message VALUES
                    ('msg_1', 'ses_current', 1, '{\"role\":\"user\",\"system\":\"recorded system prompt\"}'),
                    ('msg_2', 'ses_current', 2, '{\"role\":\"user\"}'),
                    ('msg_3', 'ses_current', 3, '{\"role\":\"assistant\",\"system\":\"assistant metadata\"}'),
                    ('msg_4', 'ses_other', 1, '{\"role\":\"user\",\"system\":\"another session\"}');",
            )
            .unwrap();

        let source = format!("sqlite:{}:ses_current", path.display());
        let prompts = load_system_prompts("opencode", &source).unwrap();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].role, SessionPromptRole::System);
        assert_eq!(prompts[0].origin, SessionPromptOrigin::Request);
        assert_eq!(prompts[0].text, "recorded system prompt");
        assert_eq!(prompts[0].source.section, "message");
        assert_eq!(prompts[0].source.record_id.as_deref(), Some("msg_1"));

        let absent_source = format!("sqlite:{}:ses_absent", path.display());
        assert!(load_system_prompts("opencode", &absent_source)
            .unwrap()
            .is_empty());
    }
}
