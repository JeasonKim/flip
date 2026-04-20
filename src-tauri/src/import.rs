use crate::profile::{Account, AccountType, AgentType, FlipConfig};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub imported: u32,
    pub skipped: u32,
}

fn ccswitch_db_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".cc-switch")
        .join("cc-switch.db")
}

/// 从 ccswitch 的 SQLite 数据库导入所有 provider 到 Flip
pub fn import_from_ccswitch() -> Result<ImportResult, String> {
    let db_path = ccswitch_db_path();
    if !db_path.exists() {
        return Err("未找到 CC Switch 数据库 (~/.cc-switch/cc-switch.db)".into());
    }

    let conn =
        rusqlite::Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| format!("打开数据库失败: {}", e))?;

    let mut stmt = conn
        .prepare("SELECT id, app_type, name, settings_config, is_current FROM providers")
        .map_err(|e| format!("查询失败: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, bool>(4)?,
            ))
        })
        .map_err(|e| format!("读取数据失败: {}", e))?;

    let mut config = crate::profile::load_profiles();
    let mut imported = 0u32;
    let mut skipped = 0u32;

    for row in rows {
        let (id, app_type, name, settings_json, is_current) =
            row.map_err(|e| format!("解析行失败: {}", e))?;

        let agent_type = match app_type.as_str() {
            "claude" => AgentType::Claude,
            "codex" => AgentType::Codex,
            _ => {
                skipped += 1;
                continue;
            }
        };

        let settings: serde_json::Value = match serde_json::from_str(&settings_json) {
            Ok(settings) => settings,
            Err(err) => {
                log::warn!(
                    "[import] failed to parse provider settings id={} app_type={}: {}. Imported with empty settings.",
                    id,
                    app_type,
                    err
                );
                serde_json::Value::Object(serde_json::Map::new())
            }
        };

        let account = match agent_type {
            AgentType::Claude => convert_claude_provider(&id, &name, &settings),
            AgentType::Codex => convert_codex_provider(&id, &name, &settings),
        };

        match config.enroll_account(agent_type, account) {
            Ok(_) => {
                if is_current {
                    log::warn!(
                        "[import] ignored CC Switch current provider id={} agent={:?}; Flip current account follows live agent config.",
                        id,
                        agent_type
                    );
                }
                imported += 1;
            }
            Err(_) => {
                // ID 重复，跳过
                skipped += 1;
            }
        }
    }

    crate::current_account::reconcile_saved_current_accounts(&mut config);
    crate::profile::save_profiles(&config)?;
    Ok(ImportResult { imported, skipped })
}

/// ccswitch Claude provider → Flip Account
fn convert_claude_provider(id: &str, name: &str, settings: &serde_json::Value) -> Account {
    let env = settings.get("env");

    // 从 env 中提取 API Key
    let api_key = env
        .and_then(|e| e.get("ANTHROPIC_AUTH_TOKEN").or(e.get("ANTHROPIC_API_KEY")))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    if let Some(key) = api_key {
        // API 账号：提取 key 和 base_url
        let base_url = env
            .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());

        let mut creds = serde_json::Map::new();
        creds.insert("apiKey".into(), key.into());
        if let Some(url) = base_url {
            creds.insert("baseUrl".into(), url.into());
        }

        Account {
            id: id.to_string(),
            account_type: AccountType::Api,
            label: name.to_string(),
            credentials: Some(serde_json::Value::Object(creds)),
            auth: None,
            config: None,
        }
    } else {
        // Plan 账号：凭据在 Keychain/文件中，导入时不含具体 token
        Account {
            id: id.to_string(),
            account_type: AccountType::Plan,
            label: name.to_string(),
            credentials: None,
            auth: None,
            config: None,
        }
    }
}

/// ccswitch Codex provider → Flip Account
fn convert_codex_provider(id: &str, name: &str, settings: &serde_json::Value) -> Account {
    let raw_auth = settings.get("auth").cloned();
    let raw_config = settings
        .get("config")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let account_type = crate::agent::codex::detect_account_type(&raw_auth);
    let (auth, config) = match account_type {
        AccountType::Plan => {
            let auth = crate::agent::codex::collapse_empty_auth(
                raw_auth
                    .as_ref()
                    .map(crate::agent::codex::extract_plan_auth),
            );
            (auth, None)
        }
        AccountType::Api => {
            let auth = crate::agent::codex::collapse_empty_auth(
                raw_auth.as_ref().map(crate::agent::codex::extract_api_auth),
            );
            let config = crate::agent::codex::collapse_empty_config(
                raw_config
                    .as_deref()
                    .map(crate::agent::codex::extract_api_config),
            );
            (auth, config)
        }
    };

    Account {
        id: id.to_string(),
        account_type,
        label: name.to_string(),
        credentials: None,
        auth,
        config,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_codex_plan_provider_keeps_only_plan_fields() {
        let account = convert_codex_provider(
            "plan-1",
            "Plan",
            &serde_json::json!({
                "auth": {
                    "auth_mode": "chatgpt",
                    "tokens": {
                        "account_id": "user-a",
                        "access_token": "tok"
                    },
                    "last_refresh": "2026-04-20T00:00:00Z",
                    "OPENAI_API_KEY": null
                },
                "config": "model = \"o3\"\n[projects.demo]\ntrusted = true\n"
            }),
        );

        assert_eq!(account.account_type, AccountType::Plan);
        assert_eq!(
            account.auth,
            Some(serde_json::json!({
                "auth_mode": "chatgpt",
                "tokens": {
                    "account_id": "user-a",
                    "access_token": "tok"
                },
                "last_refresh": "2026-04-20T00:00:00Z"
            }))
        );
        assert_eq!(account.config, None);
    }

    #[test]
    fn convert_codex_api_provider_keeps_only_required_fields() {
        let account = convert_codex_provider(
            "api-1",
            "API",
            &serde_json::json!({
                "auth": {
                    "OPENAI_API_KEY": "sk-test",
                    "foo": "bar"
                },
                "config": "model = \"o3\"\nmodel_provider = \"azure\"\n[model_providers.azure]\nbase_url = \"https://example.com/v1\"\nwire_api = \"responses\"\n[projects.demo]\ntrusted = true\n"
            }),
        );

        assert_eq!(account.account_type, AccountType::Api);
        assert_eq!(
            account.auth,
            Some(serde_json::json!({
                "OPENAI_API_KEY": "sk-test"
            }))
        );

        let doc = account
            .config
            .as_deref()
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert_eq!(doc.get("model_provider").unwrap().as_str(), Some("azure"));
        assert!(doc.get("model_providers").is_some());
        assert!(doc.get("model").is_none());
        assert!(doc.get("projects").is_none());
    }
}

/// 手动注册 API 账号
pub fn enroll_api_account(
    config: &mut FlipConfig,
    agent_type: AgentType,
    label: String,
    api_key: String,
    base_url: Option<String>,
) -> Result<Account, String> {
    let account = match agent_type {
        AgentType::Claude => {
            let provider_label = base_url
                .as_deref()
                .map(crate::agent::infer_provider_from_url)
                .unwrap_or_else(|| label.clone());
            let id = crate::agent::claude::generate_account_id(
                &AccountType::Api,
                &provider_label,
                &api_key,
            );

            let mut creds = serde_json::Map::new();
            creds.insert("apiKey".into(), api_key.into());
            if let Some(url) = &base_url {
                creds.insert("baseUrl".into(), url.clone().into());
            }

            Account {
                id,
                account_type: AccountType::Api,
                label,
                credentials: Some(serde_json::Value::Object(creds)),
                auth: None,
                config: None,
            }
        }
        AgentType::Codex => {
            let id = crate::agent::codex::generate_account_id(&AccountType::Api, &label, &api_key);

            let auth = serde_json::json!({ "OPENAI_API_KEY": api_key });

            // 有 base_url 时生成 config.toml 的 provider 段
            let config_str = base_url.as_deref().map(|url| {
                let key = label.to_lowercase().replace(' ', "_").replace('-', "_");
                format!(
                    "model_provider = \"{key}\"\n\n\
                     [model_providers.{key}]\n\
                     base_url = \"{url}\"\n\
                     wire_api = \"responses\"\n\
                     requires_openai_auth = true\n"
                )
            });

            Account {
                id,
                account_type: AccountType::Api,
                label,
                credentials: None,
                auth: Some(auth),
                config: config_str,
            }
        }
    };

    config.enroll_account(agent_type, account.clone())?;
    crate::profile::save_profiles(config)?;
    Ok(account)
}
