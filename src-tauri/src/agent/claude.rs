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

/// 判断账号类型：有 OAuth 凭据 → Plan，有 apiKey → API
pub fn detect_account_type(
    settings: &Option<serde_json::Value>,
    credentials: &Option<serde_json::Value>,
) -> AccountType {
    if let Some(creds) = credentials {
        if creds.get("claudeAiOauth").is_some() || creds.get("claude.ai_oauth").is_some() {
            return AccountType::Plan;
        }
    }
    if let Some(s) = settings {
        if s.get("apiKey").is_some() {
            return AccountType::Api;
        }
    }
    AccountType::Plan
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
                if let Some(url) = s.get("baseUrl").and_then(|v| v.as_str()) {
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
                    obj.insert("apiKey".into(), api_key.clone());
                }
                if let Some(base_url) = creds.get("baseUrl") {
                    obj.insert("baseUrl".into(), base_url.clone());
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
            let mut obj = serde_json::Map::new();
            if let Some(key) = s.get("apiKey") {
                obj.insert("apiKey".into(), key.clone());
            }
            if let Some(url) = s.get("baseUrl") {
                obj.insert("baseUrl".into(), url.clone());
            }
            if obj.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(obj))
            }
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
        // 不应包含其他设置字段
        assert!(extracted.get("model").is_none());
        assert!(extracted.get("hasCompletedOnboarding").is_none());
    }
}
