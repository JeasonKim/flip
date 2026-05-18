use crate::agent;
use crate::profile::{Account, AccountType, AgentConfig, AgentType, FlipConfig};

pub struct ClaudeLiveSnapshot {
    pub settings: Option<serde_json::Value>,
    pub credentials: Option<serde_json::Value>,
}

pub struct CodexLiveSnapshot {
    pub auth: Option<serde_json::Value>,
}

pub struct LiveCurrentAccountMatch {
    pub account_id: Option<String>,
    pub live_identity: Option<String>,
}

pub fn reconcile_saved_current_accounts(config: &mut FlipConfig) -> bool {
    let mut updated = false;
    updated |= reconcile_saved_claude_current(config);
    updated |= reconcile_saved_codex_current(config);
    updated
}

fn reconcile_saved_claude_current(config: &mut FlipConfig) -> bool {
    let snapshot = match crate::agent::claude::snapshot_live() {
        Ok((settings, credentials)) => ClaudeLiveSnapshot {
            settings,
            credentials,
        },
        Err(err) => {
            log::warn!("[current-account] failed to snapshot Claude live config: {err}");
            return false;
        }
    };
    let live_match = resolve_claude_live_current_account(
        &config.agent_config(AgentType::Claude).accounts,
        &snapshot,
    );
    sync_agent_current(
        config.agent_config_mut(AgentType::Claude),
        AgentType::Claude,
        live_match,
    )
}

fn reconcile_saved_codex_current(config: &mut FlipConfig) -> bool {
    let snapshot = match crate::agent::codex::snapshot_live() {
        Ok((auth, _)) => CodexLiveSnapshot { auth },
        Err(err) => {
            log::warn!("[current-account] failed to snapshot Codex live config: {err}");
            return false;
        }
    };
    let live_match = resolve_codex_live_current_account(
        &config.agent_config(AgentType::Codex).accounts,
        &snapshot,
    );
    sync_agent_current(
        config.agent_config_mut(AgentType::Codex),
        AgentType::Codex,
        live_match,
    )
}

fn sync_agent_current(
    agent_config: &mut AgentConfig,
    agent_type: AgentType,
    live_match: LiveCurrentAccountMatch,
) -> bool {
    if agent_config.current == live_match.account_id {
        return false;
    }

    log::warn!(
        "[current-account] reconciled current account agent={} stored_current={:?} live_current={:?} live_identity={:?}",
        agent_name(agent_type),
        agent_config.current,
        live_match.account_id,
        live_match.live_identity,
    );
    agent_config.current = live_match.account_id;
    true
}

pub fn resolve_claude_live_current_account(
    accounts: &[Account],
    snapshot: &ClaudeLiveSnapshot,
) -> LiveCurrentAccountMatch {
    match crate::agent::claude::detect_account_type(&snapshot.settings, &snapshot.credentials) {
        AccountType::Plan => {
            let live_refresh_token = snapshot
                .credentials
                .as_ref()
                .and_then(read_claude_oauth)
                .and_then(|oauth| oauth.get("refreshToken"))
                .and_then(|value| value.as_str());

            LiveCurrentAccountMatch {
                account_id: live_refresh_token.and_then(|refresh_token| {
                    accounts
                        .iter()
                        .find(|account| {
                            account.account_type == AccountType::Plan
                                && account
                                    .credentials
                                    .as_ref()
                                    .and_then(read_claude_oauth)
                                    .and_then(|oauth| oauth.get("refreshToken"))
                                    .and_then(|value| value.as_str())
                                    == Some(refresh_token)
                        })
                        .map(|account| account.id.clone())
                }),
                live_identity: live_refresh_token.map(|refresh_token| {
                    format!("claude-plan:{}", agent::stable_id_suffix(refresh_token))
                }),
            }
        }
        AccountType::Api => {
            // 顶层 apiKey 或 env.ANTHROPIC_AUTH_TOKEN 均作为身份标识
            let live_api_key = extract_live_claude_api_key(snapshot.settings.as_ref());

            LiveCurrentAccountMatch {
                account_id: live_api_key.and_then(|api_key| {
                    accounts
                        .iter()
                        .find(|account| {
                            account.account_type == AccountType::Api
                                && saved_claude_api_key_matches(account, api_key)
                        })
                        .map(|account| account.id.clone())
                }),
                live_identity: live_api_key
                    .map(|api_key| format!("claude-api:{}", agent::stable_id_suffix(api_key))),
            }
        }
    }
}

pub fn resolve_codex_live_current_account(
    accounts: &[Account],
    snapshot: &CodexLiveSnapshot,
) -> LiveCurrentAccountMatch {
    match crate::agent::codex::detect_account_type(&snapshot.auth) {
        AccountType::Plan => {
            let live_account_id = snapshot
                .auth
                .as_ref()
                .and_then(|auth| auth.get("tokens"))
                .and_then(|tokens| tokens.get("account_id"))
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty());

            LiveCurrentAccountMatch {
                account_id: live_account_id.and_then(|account_id| {
                    accounts
                        .iter()
                        .find(|account| {
                            account.account_type == AccountType::Plan
                                && account
                                    .auth
                                    .as_ref()
                                    .and_then(|auth| auth.get("tokens"))
                                    .and_then(|tokens| tokens.get("account_id"))
                                    .and_then(|value| value.as_str())
                                    == Some(account_id)
                        })
                        .map(|account| account.id.clone())
                }),
                live_identity: live_account_id.map(|account_id| {
                    format!("codex-plan:{}", agent::stable_id_suffix(account_id))
                }),
            }
        }
        AccountType::Api => {
            let live_api_key = snapshot
                .auth
                .as_ref()
                .and_then(|auth| auth.get("OPENAI_API_KEY"))
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty());

            LiveCurrentAccountMatch {
                account_id: live_api_key.and_then(|api_key| {
                    accounts
                        .iter()
                        .find(|account| {
                            account.account_type == AccountType::Api
                                && account
                                    .auth
                                    .as_ref()
                                    .and_then(|auth| auth.get("OPENAI_API_KEY"))
                                    .and_then(|value| value.as_str())
                                    == Some(api_key)
                        })
                        .map(|account| account.id.clone())
                }),
                live_identity: live_api_key
                    .map(|api_key| format!("codex-api:{}", agent::stable_id_suffix(api_key))),
            }
        }
    }
}

fn read_claude_oauth(credentials: &serde_json::Value) -> Option<&serde_json::Value> {
    credentials
        .get("claudeAiOauth")
        .or_else(|| credentials.get("claude.ai_oauth"))
}

/// 从 live settings 中提取 API Key（顶层 apiKey 或 env.ANTHROPIC_AUTH_TOKEN）
fn extract_live_claude_api_key(settings: Option<&serde_json::Value>) -> Option<&str> {
    let s = settings?;
    s.get("apiKey")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            s.get("env")
                .and_then(|e| {
                    e.get("ANTHROPIC_AUTH_TOKEN")
                        .or_else(|| e.get("ANTHROPIC_API_KEY"))
                })
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
}

/// 检查已保存账号的 API Key 是否与 live key 匹配（支持顶层和 env 两种格式）
fn saved_claude_api_key_matches(account: &Account, live_key: &str) -> bool {
    let Some(creds) = account.credentials.as_ref() else {
        return false;
    };
    if creds
        .get("apiKey")
        .and_then(|v| v.as_str())
        .is_some_and(|k| k == live_key)
    {
        return true;
    }
    creds
        .get("env")
        .and_then(|e| {
            e.get("ANTHROPIC_AUTH_TOKEN")
                .or_else(|| e.get("ANTHROPIC_API_KEY"))
        })
        .and_then(|v| v.as_str())
        .is_some_and(|k| k == live_key)
}

fn agent_name(agent_type: AgentType) -> &'static str {
    match agent_type {
        AgentType::Claude => "claude",
        AgentType::Codex => "codex",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_account(
        id: &str,
        account_type: AccountType,
        credentials: Option<serde_json::Value>,
        auth: Option<serde_json::Value>,
    ) -> Account {
        Account {
            id: id.into(),
            account_type,
            label: id.into(),
            credentials,
            auth,
            config: None,
        }
    }

    #[test]
    fn resolve_claude_live_current_account_matches_api_key() {
        let accounts = vec![
            sample_account(
                "openrouter-1",
                AccountType::Api,
                Some(serde_json::json!({
                    "apiKey": "sk-live",
                    "baseUrl": "https://openrouter.ai/api/v1"
                })),
                None,
            ),
            sample_account("plan-1", AccountType::Plan, None, None),
        ];

        let live_match = resolve_claude_live_current_account(
            &accounts,
            &ClaudeLiveSnapshot {
                settings: Some(serde_json::json!({
                    "apiKey": "sk-live",
                    "baseUrl": "https://openrouter.ai/api/v1"
                })),
                credentials: None,
            },
        );

        assert_eq!(live_match.account_id.as_deref(), Some("openrouter-1"));
    }

    #[test]
    fn resolve_claude_live_current_account_matches_env_auth_token() {
        let accounts = vec![sample_account(
            "zhipu-1",
            AccountType::Api,
            Some(serde_json::json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://open.bigmodel.cn/api/anthropic",
                    "ANTHROPIC_AUTH_TOKEN": "zhipu-key",
                    "ANTHROPIC_MODEL": "glm-5.1"
                }
            })),
            None,
        )];

        let live_match = resolve_claude_live_current_account(
            &accounts,
            &ClaudeLiveSnapshot {
                settings: Some(serde_json::json!({
                    "env": {
                        "ANTHROPIC_BASE_URL": "https://open.bigmodel.cn/api/anthropic",
                        "ANTHROPIC_AUTH_TOKEN": "zhipu-key",
                        "ANTHROPIC_MODEL": "glm-5.1"
                    }
                })),
                credentials: None,
            },
        );

        assert_eq!(live_match.account_id.as_deref(), Some("zhipu-1"));
    }

    #[test]
    fn resolve_claude_live_current_account_prefers_api_when_oauth_also_exists() {
        let accounts = vec![
            sample_account(
                "packy-api",
                AccountType::Api,
                Some(serde_json::json!({
                    "env": {
                        "ANTHROPIC_BASE_URL": "https://www.packyapi.com",
                        "ANTHROPIC_AUTH_TOKEN": "packy-key"
                    }
                })),
                None,
            ),
            sample_account(
                "claude-plan",
                AccountType::Plan,
                Some(serde_json::json!({
                    "claudeAiOauth": { "refreshToken": "refresh-token" }
                })),
                None,
            ),
        ];

        let live_match = resolve_claude_live_current_account(
            &accounts,
            &ClaudeLiveSnapshot {
                settings: Some(serde_json::json!({
                    "env": {
                        "ANTHROPIC_BASE_URL": "https://www.packyapi.com",
                        "ANTHROPIC_AUTH_TOKEN": "packy-key"
                    }
                })),
                credentials: Some(serde_json::json!({
                    "claudeAiOauth": { "refreshToken": "refresh-token" }
                })),
            },
        );

        assert_eq!(live_match.account_id.as_deref(), Some("packy-api"));
        assert!(live_match
            .live_identity
            .as_deref()
            .is_some_and(|identity| identity.starts_with("claude-api:")));
    }

    #[test]
    fn resolve_codex_live_current_account_returns_none_when_no_saved_match() {
        let accounts = vec![sample_account(
            "plan-a",
            AccountType::Plan,
            None,
            Some(serde_json::json!({
                "auth_mode": "chatgpt",
                "tokens": { "account_id": "user-a" }
            })),
        )];

        let live_match = resolve_codex_live_current_account(
            &accounts,
            &CodexLiveSnapshot {
                auth: Some(serde_json::json!({
                    "auth_mode": "chatgpt",
                    "tokens": { "account_id": "user-b", "access_token": "tok" }
                })),
            },
        );

        assert_eq!(live_match.account_id, None);
        assert!(live_match.live_identity.is_some());
    }
}
