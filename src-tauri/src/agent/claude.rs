use std::path::PathBuf;

use crate::file_ops;
use crate::profile::{Account, AccountType};

/// Claude Code settings.json 路径
pub fn settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("settings.json")
}

/// Claude Code 凭据文件路径
pub fn credentials_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join(".credentials.json")
}

/// 读取当前 live 快照（settings + credentials），仅用于身份检测
/// macOS 上文件读不到 credentials 时，从 Keychain 补充
pub fn snapshot_live() -> Result<(Option<serde_json::Value>, Option<serde_json::Value>), String> {
    let settings = read_json_optional(&settings_path())?;
    #[allow(unused_mut)]
    let mut credentials = read_json_optional(&credentials_path())?;

    #[cfg(target_os = "macos")]
    if credentials.is_none() {
        credentials = crate::credential::read_claude_keychain_raw();
    }

    Ok((settings, credentials))
}

/// 判断账号类型：API Key 优先；没有 API Key 时，有 OAuth 凭据 → Plan。
pub fn detect_account_type(
    settings: &Option<serde_json::Value>,
    credentials: &Option<serde_json::Value>,
) -> AccountType {
    if has_live_api_key(settings.as_ref()) {
        return AccountType::Api;
    }
    if let Some(creds) = credentials {
        if creds.get("claudeAiOauth").is_some() || creds.get("claude.ai_oauth").is_some() {
            return AccountType::Plan;
        }
    }
    AccountType::Plan
}

fn has_live_api_key(settings: Option<&serde_json::Value>) -> bool {
    settings
        .and_then(|s| {
            s.get("apiKey")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    s.get("env")
                        .and_then(|env| {
                            env.get("ANTHROPIC_AUTH_TOKEN")
                                .or_else(|| env.get("ANTHROPIC_API_KEY"))
                        })
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.is_empty())
                })
        })
        .is_some()
}

/// 推断账号 label（同步 fallback，优先用 resolve_plan_identity 的结果覆盖）
pub fn infer_label(
    settings: &Option<serde_json::Value>,
    credentials: &Option<serde_json::Value>,
    account_type: &AccountType,
) -> String {
    match account_type {
        AccountType::Plan => {
            if let Some(creds) = credentials {
                let oauth = creds
                    .get("claudeAiOauth")
                    .or_else(|| creds.get("claude.ai_oauth"));
                if let Some(sub_type) = oauth
                    .and_then(|o| o.get("subscriptionType"))
                    .and_then(|v| v.as_str())
                {
                    return format!("Claude {}", capitalize(sub_type));
                }
            }
            "Claude Plan".into()
        }
        AccountType::Api => {
            if let Some(s) = settings {
                // 顶层 baseUrl 格式
                if let Some(url) = s.get("baseUrl").and_then(|v| v.as_str()) {
                    return super::infer_provider_from_url(url);
                }
                // env 格式（智谱等第三方 API）
                if let Some(url) = s
                    .get("env")
                    .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
                    .and_then(|v| v.as_str())
                {
                    return super::infer_provider_from_url(url);
                }
            }
            "API".into()
        }
    }
}

/// 从 OAuth profile API 获取真实用户名（email / display_name）
pub async fn resolve_plan_identity(access_token: &str) -> Option<String> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.anthropic.com/api/oauth/profile")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let body: serde_json::Value = resp.json().await.ok()?;
    let account = body.get("account")?;
    account
        .get("email")
        .and_then(|v| v.as_str())
        .or_else(|| account.get("display_name").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}

/// 生成稳定的账号 ID（同一账号多次捕获 ID 不变）
pub fn generate_account_id(account_type: &AccountType, label: &str, api_key: &str) -> String {
    match account_type {
        AccountType::Plan => label.to_lowercase().replace(' ', "-"),
        AccountType::Api => {
            let base = label.to_lowercase().replace(' ', "-");
            format!("{}-{}", base, super::stable_id_suffix(api_key))
        }
    }
}

/// 将账号凭证写入 Claude Code 的 live 文件
/// - Plan：整个 credentials 写入 .credentials.json
/// - API：将 apiKey + baseUrl 合并到 settings.json（保留其余设置）
pub fn apply_profile(account: &Account) -> Result<(), String> {
    let creds = account
        .credentials
        .as_ref()
        .ok_or("account has no credentials")?;

    match account.account_type {
        AccountType::Plan => {
            // 同时写入文件和 Keychain，确保 Claude Code 能读取到
            file_ops::write_json_file(&credentials_path(), creds)
                .map_err(|e| format!("write credentials failed: {}", e))?;

            #[cfg(target_os = "macos")]
            crate::credential::write_claude_keychain(creds)
                .map_err(|e| format!("write keychain failed: {}", e))?;
        }
        AccountType::Api => {
            // 读取现有 settings.json，合并 apiKey + baseUrl，保留其余字段
            let path = settings_path();
            let mut settings = if path.exists() {
                let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                serde_json::from_str::<serde_json::Value>(&text).unwrap_or_default()
            } else {
                serde_json::json!({})
            };

            if let Some(obj) = settings.as_object_mut() {
                if let Some(api_key) = creds.get("apiKey") {
                    // 顶层 apiKey 格式：写入 apiKey + baseUrl，清除 env 中的 ANTHROPIC_* 避免冲突
                    obj.insert("apiKey".into(), api_key.clone());
                    match creds.get("baseUrl") {
                        Some(url) => {
                            obj.insert("baseUrl".into(), url.clone());
                        }
                        None => {
                            obj.remove("baseUrl");
                        }
                    }
                    if let Some(env_obj) = obj.get_mut("env").and_then(|e| e.as_object_mut()) {
                        env_obj.retain(|k, _| !k.starts_with("ANTHROPIC_"));
                    }
                } else if let Some(env) = creds.get("env") {
                    // env 格式（智谱等第三方 API）：写入 env 块，清除顶层 apiKey/baseUrl 避免冲突
                    obj.insert("env".into(), env.clone());
                    obj.remove("apiKey");
                    obj.remove("baseUrl");
                }
            }

            file_ops::write_json_file(&path, &settings)
                .map_err(|e| format!("write settings failed: {}", e))?;
        }
    }
    Ok(())
}

/// 从 live 配置提取需要保存的凭证数据
/// - Plan：整个 .credentials.json
/// - API：从 settings.json 提取 apiKey + baseUrl
pub fn extract_credentials(
    settings: &Option<serde_json::Value>,
    credentials: &Option<serde_json::Value>,
    account_type: &AccountType,
) -> Option<serde_json::Value> {
    match account_type {
        AccountType::Plan => credentials.clone(),
        AccountType::Api => {
            let s = settings.as_ref()?;

            // 顶层 apiKey 格式
            if s.get("apiKey").is_some() {
                let mut obj = serde_json::Map::new();
                if let Some(key) = s.get("apiKey") {
                    obj.insert("apiKey".into(), key.clone());
                }
                if let Some(url) = s.get("baseUrl") {
                    obj.insert("baseUrl".into(), url.clone());
                }
                return if obj.is_empty() {
                    None
                } else {
                    Some(serde_json::Value::Object(obj))
                };
            }

            // env 格式（智谱等第三方 API）：保存完整 env 块，包含 model 配置
            if let Some(env) = s.get("env") {
                if env
                    .get("ANTHROPIC_AUTH_TOKEN")
                    .or_else(|| env.get("ANTHROPIC_API_KEY"))
                    .is_some()
                {
                    return Some(serde_json::json!({ "env": env }));
                }
            }

            None
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

fn read_json_optional(path: &PathBuf) -> Result<Option<serde_json::Value>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_plan_with_oauth_credentials() {
        let creds = Some(serde_json::json!({
            "claudeAiOauth": { "accessToken": "test", "expiresAt": 9999999999999i64 }
        }));
        assert_eq!(detect_account_type(&None, &creds), AccountType::Plan);
    }

    #[test]
    fn detect_api_with_api_key() {
        let settings = Some(serde_json::json!({
            "apiKey": "sk-or-test",
            "baseUrl": "https://openrouter.ai/api/v1"
        }));
        assert_eq!(detect_account_type(&settings, &None), AccountType::Api);
    }

    #[test]
    fn infer_label_api_from_base_url() {
        let settings = Some(serde_json::json!({
            "apiKey": "sk-test",
            "baseUrl": "https://openrouter.ai/api/v1"
        }));
        assert_eq!(
            infer_label(&settings, &None, &AccountType::Api),
            "OpenRouter"
        );
    }

    #[test]
    fn extract_credentials_plan_returns_full_credentials() {
        let creds = Some(serde_json::json!({
            "claudeAiOauth": { "accessToken": "tok", "refreshToken": "ref" }
        }));
        let extracted = extract_credentials(&None, &creds, &AccountType::Plan);
        assert_eq!(extracted, creds);
    }

    #[test]
    fn extract_credentials_api_picks_only_key_and_url() {
        let settings = Some(serde_json::json!({
            "apiKey": "sk-test",
            "baseUrl": "https://openrouter.ai/api/v1",
            "model": "claude-sonnet-4-20250514",
            "hasCompletedOnboarding": true
        }));
        let extracted = extract_credentials(&settings, &None, &AccountType::Api).unwrap();
        assert_eq!(extracted.get("apiKey").unwrap(), "sk-test");
        assert_eq!(
            extracted.get("baseUrl").unwrap(),
            "https://openrouter.ai/api/v1"
        );
        assert!(extracted.get("model").is_none());
        assert!(extracted.get("hasCompletedOnboarding").is_none());
    }

    #[test]
    fn detect_api_with_env_auth_token() {
        let settings = Some(serde_json::json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://open.bigmodel.cn/api/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "zhipu-key"
            }
        }));
        assert_eq!(detect_account_type(&settings, &None), AccountType::Api);
    }

    #[test]
    fn detect_api_settings_take_priority_over_leftover_oauth_credentials() {
        let settings = Some(serde_json::json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://example.com",
                "ANTHROPIC_AUTH_TOKEN": "api-key"
            }
        }));
        let credentials = Some(serde_json::json!({
            "claudeAiOauth": {
                "accessToken": "oauth-token",
                "refreshToken": "refresh-token"
            }
        }));

        assert_eq!(
            detect_account_type(&settings, &credentials),
            AccountType::Api
        );
    }

    #[test]
    fn infer_label_api_from_env_base_url() {
        let settings = Some(serde_json::json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://open.bigmodel.cn/api/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "zhipu-key"
            }
        }));
        assert_eq!(infer_label(&settings, &None, &AccountType::Api), "Zhipu AI");
    }

    #[test]
    fn extract_credentials_api_env_format_stores_full_env_block() {
        let settings = Some(serde_json::json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://open.bigmodel.cn/api/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "zhipu-key",
                "ANTHROPIC_MODEL": "glm-5.1"
            },
            "hasCompletedOnboarding": true
        }));
        let extracted = extract_credentials(&settings, &None, &AccountType::Api).unwrap();
        let env = extracted.get("env").unwrap();
        assert_eq!(env.get("ANTHROPIC_AUTH_TOKEN").unwrap(), "zhipu-key");
        assert_eq!(
            env.get("ANTHROPIC_BASE_URL").unwrap(),
            "https://open.bigmodel.cn/api/anthropic"
        );
        assert_eq!(env.get("ANTHROPIC_MODEL").unwrap(), "glm-5.1");
        // 不应包含 env 以外的字段
        assert!(extracted.get("hasCompletedOnboarding").is_none());
    }
}
