use std::path::PathBuf;

use crate::file_ops;
use crate::profile::{Account, AccountType};

pub fn auth_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".codex")
        .join("auth.json")
}

pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".codex")
        .join("config.toml")
}

/// 读取当前 live 快照（auth + config），仅用于身份检测
pub fn snapshot_live() -> Result<(Option<serde_json::Value>, Option<String>), String> {
    let auth = read_json_optional(&auth_path())?;
    let config = read_text_optional(&config_path());
    Ok((auth, config))
}

/// 判断账号类型
pub fn detect_account_type(auth: &Option<serde_json::Value>) -> AccountType {
    if let Some(auth_val) = auth {
        if let Some(mode) = auth_val.get("auth_mode").and_then(|v| v.as_str()) {
            if mode == "chatgpt" {
                if let Some(tokens) = auth_val.get("tokens") {
                    if tokens.get("access_token").is_some() {
                        return AccountType::Plan;
                    }
                }
            }
        }
    }
    AccountType::Api
}

/// 推断 label
pub fn infer_label(
    auth: &Option<serde_json::Value>,
    config_text: &Option<String>,
    account_type: &AccountType,
) -> String {
    match account_type {
        AccountType::Plan => {
            // 优先从 id_token JWT 中提取 email 或 name
            if let Some(auth_val) = auth {
                if let Some(label) = extract_identity_from_id_token(auth_val) {
                    return label;
                }
                // fallback: account_id
                if let Some(account_id) = auth_val
                    .get("tokens")
                    .and_then(|t| t.get("account_id"))
                    .and_then(|v| v.as_str())
                {
                    return account_id.to_string();
                }
            }
            "Codex Plan".into()
        }
        AccountType::Api => {
            if let Some(text) = config_text {
                if let Ok(doc) = text.parse::<toml_edit::DocumentMut>() {
                    if let Some(providers) = doc.get("model_providers").and_then(|v| v.as_table()) {
                        for (_key, val) in providers.iter() {
                            if let Some(url) = val.get("base_url").and_then(|v| v.as_str()) {
                                return super::infer_provider_from_url(url);
                            }
                        }
                    }
                }
            }
            "API".into()
        }
    }
}

/// 生成稳定的账号 ID（同一账号多次捕获 ID 不变）
pub fn generate_account_id(account_type: &AccountType, label: &str, api_key: &str) -> String {
    let base = label.to_lowercase().replace(' ', "-");
    match account_type {
        AccountType::Plan => base,
        AccountType::Api => {
            format!("{}-{}", base, super::stable_id_suffix(api_key))
        }
    }
}

/// 从 config.toml 中提取 provider 相关配置：
/// - model_provider（指定活跃的 provider）
/// - [model_providers]（各 provider 的连接信息）
/// 去掉 model、model_reasoning_effort、[projects] 等无关字段
pub fn extract_api_config(config_text: &str) -> String {
    let Ok(doc) = config_text.parse::<toml_edit::DocumentMut>() else {
        return String::new();
    };
    let mut new_doc = toml_edit::DocumentMut::new();
    if let Some(provider) = doc.get("model_provider") {
        new_doc.insert("model_provider", provider.clone());
    }
    if let Some(providers) = doc.get("model_providers") {
        new_doc.insert("model_providers", providers.clone());
    }
    new_doc.to_string()
}

/// Plan 账号：保留 auth_mode + tokens + last_refresh（去掉 OPENAI_API_KEY:null 等噪音字段）
pub fn extract_plan_auth(auth: &serde_json::Value) -> serde_json::Value {
    let mut result = serde_json::Map::new();
    for key in ["auth_mode", "tokens", "last_refresh"] {
        if let Some(val) = auth.get(key) {
            result.insert(key.into(), val.clone());
        }
    }
    serde_json::Value::Object(result)
}

/// API 账号：只保留 OPENAI_API_KEY
pub fn extract_api_auth(auth: &serde_json::Value) -> serde_json::Value {
    let mut result = serde_json::Map::new();
    if let Some(key) = auth
        .get("OPENAI_API_KEY")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
    {
        result.insert("OPENAI_API_KEY".into(), key.into());
    }
    serde_json::Value::Object(result)
}

pub fn collapse_empty_auth(auth: Option<serde_json::Value>) -> Option<serde_json::Value> {
    match auth {
        Some(serde_json::Value::Object(fields)) if fields.is_empty() => None,
        other => other,
    }
}

pub fn collapse_empty_config(config: Option<String>) -> Option<String> {
    config.and_then(|text| {
        if text.trim().is_empty() {
            None
        } else {
            Some(text)
        }
    })
}

/// 将账号凭证写入 Codex 的 live 文件
/// - auth → ~/.codex/auth.json（全量覆盖）
/// - config → ~/.codex/config.toml（合并 provider 配置，保留其他设置）
pub fn apply_profile(account: &Account) -> Result<(), String> {
    if let Some(auth_val) = &account.auth {
        file_ops::write_json_file(&auth_path(), auth_val)
            .map_err(|e| format!("write codex auth failed: {}", e))?;
    }
    // 无论 API 还是 Plan，都走 merge：
    // - API 账号：写入保存的 model_provider + [model_providers]
    // - Plan 账号：config 为 None → 传入空字符串 → 清除旧的 provider 配置
    let saved_config = account.config.as_deref().unwrap_or("");
    merge_provider_config_to_live(saved_config)?;
    Ok(())
}

/// 将保存的 provider 配置合并到现有 config.toml
/// 只更新 model_provider 和 [model_providers]，保留 model、model_reasoning_effort、[projects] 等设置
fn merge_provider_config_to_live(saved_config: &str) -> Result<(), String> {
    let path = config_path();
    let live_text = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| format!("read live config: {}", e))?
    } else {
        String::new()
    };
    let merged = merge_toml_provider_fields(&live_text, saved_config)?;
    file_ops::write_text_file(&path, &merged)
        .map_err(|e| format!("write codex config failed: {}", e))
}

/// 纯函数：将 saved 中的 provider 字段合并到 live 文档
fn merge_toml_provider_fields(live_text: &str, saved_text: &str) -> Result<String, String> {
    let saved_doc = saved_text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("parse saved config: {}", e))?;

    let mut live_doc = if live_text.trim().is_empty() {
        toml_edit::DocumentMut::new()
    } else {
        live_text
            .parse::<toml_edit::DocumentMut>()
            .unwrap_or_else(|_| toml_edit::DocumentMut::new())
    };

    // 清除旧的 provider 配置，再写入新的
    live_doc.remove("model_provider");
    live_doc.remove("model_providers");

    if let Some(provider) = saved_doc.get("model_provider") {
        live_doc.insert("model_provider", provider.clone());
    }
    if let Some(providers) = saved_doc.get("model_providers") {
        live_doc.insert("model_providers", providers.clone());
    }

    Ok(live_doc.to_string())
}

/// 从 id_token（JWT）的 payload 中提取 email 或 name
fn extract_identity_from_id_token(auth: &serde_json::Value) -> Option<String> {
    let id_token = auth.get("tokens")?.get("id_token")?.as_str()?;
    let payload_b64 = id_token.split('.').nth(1)?;
    let decoded = base64_url_decode(payload_b64)?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    claims
        .get("email")
        .and_then(|v| v.as_str())
        .or_else(|| claims.get("name").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}

fn base64_url_decode(input: &str) -> Option<Vec<u8>> {
    let mut s: String = input
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            c => c,
        })
        .collect();
    while s.len() % 4 != 0 {
        s.push('=');
    }
    decode_base64_standard(&s)
}

fn decode_base64_standard(input: &str) -> Option<Vec<u8>> {
    let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut buf = Vec::new();
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for &b in input.as_bytes() {
        if b == b'=' {
            break;
        }
        let val = table.iter().position(|&t| t == b)? as u32;
        acc = (acc << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            buf.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(buf)
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

fn read_text_optional(path: &PathBuf) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_plan_chatgpt_mode() {
        let auth = Some(serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": { "access_token": "eyJ...", "account_id": "user-abc" }
        }));
        assert_eq!(detect_account_type(&auth), AccountType::Plan);
    }

    #[test]
    fn detect_api_no_chatgpt() {
        let auth = Some(serde_json::json!({
            "auth_mode": "api-key",
            "api_key": "sk-test"
        }));
        assert_eq!(detect_account_type(&auth), AccountType::Api);
    }

    #[test]
    fn infer_label_plan_fallback_to_account_id() {
        let auth = Some(serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": { "access_token": "eyJ...", "account_id": "user-abc123" }
        }));
        assert_eq!(infer_label(&auth, &None, &AccountType::Plan), "user-abc123");
    }

    #[test]
    fn infer_label_plan_from_id_token_email() {
        let auth = Some(serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "access_token": "eyJ...",
                "account_id": "user-abc123",
                "id_token": "eyJhbGciOiJSUzI1NiJ9.eyJlbWFpbCI6ImFsaWNlQGV4YW1wbGUuY29tIiwibmFtZSI6IkFsaWNlIn0.sig"
            }
        }));
        assert_eq!(
            infer_label(&auth, &None, &AccountType::Plan),
            "alice@example.com"
        );
    }

    #[test]
    fn base64_url_decode_works() {
        let decoded = base64_url_decode("aGVsbG8").unwrap();
        assert_eq!(decoded, b"hello");
    }

    #[test]
    fn infer_label_api_from_toml() {
        let config = Some(
            r#"
[model_providers.azure]
base_url = "https://my-resource.openai.azure.com/openai"
model = "gpt-4"
"#
            .to_string(),
        );
        assert_eq!(
            infer_label(&None, &config, &AccountType::Api),
            "Azure OpenAI"
        );
    }

    #[test]
    fn extract_api_config_includes_model_provider() {
        let config = r#"
model = "o3"
model_provider = "shengsuanyun"
model_reasoning_effort = "high"

[model_providers.shengsuanyun]
name = "ShengSuanYun"
base_url = "https://api.shengsuanyun.com/v1"

[projects.my-project]
sandbox_permissions = ["disk_full_read_access"]
"#;
        let result = extract_api_config(config);
        let doc = result.parse::<toml_edit::DocumentMut>().unwrap();

        // 保留 model_provider 和 model_providers
        assert_eq!(
            doc.get("model_provider").unwrap().as_str().unwrap(),
            "shengsuanyun"
        );
        assert!(doc.get("model_providers").is_some());

        // 去掉无关字段
        assert!(doc.get("model").is_none());
        assert!(doc.get("model_reasoning_effort").is_none());
        assert!(doc.get("projects").is_none());
    }

    #[test]
    fn extract_api_auth_drops_null_openai_api_key() {
        let auth = serde_json::json!({
            "OPENAI_API_KEY": null,
            "foo": "bar"
        });

        assert_eq!(extract_api_auth(&auth), serde_json::json!({}));
    }

    #[test]
    fn merge_preserves_non_provider_settings() {
        let live = r#"
model = "o3"
model_provider = "old_provider"
model_reasoning_effort = "high"

[model_providers.old_provider]
base_url = "https://old.api.com/v1"

[projects.my-project]
sandbox_permissions = ["disk_full_read_access"]
"#;
        let saved = r#"
model_provider = "new_provider"

[model_providers.new_provider]
base_url = "https://new.api.com/v1"
"#;
        let merged = merge_toml_provider_fields(live, saved).unwrap();
        let doc = merged.parse::<toml_edit::DocumentMut>().unwrap();

        // provider 配置已切换
        assert_eq!(
            doc.get("model_provider").unwrap().as_str().unwrap(),
            "new_provider"
        );
        assert!(doc
            .get("model_providers")
            .unwrap()
            .get("new_provider")
            .is_some());
        assert!(doc
            .get("model_providers")
            .unwrap()
            .get("old_provider")
            .is_none());

        // 其他设置原样保留
        assert_eq!(doc.get("model").unwrap().as_str().unwrap(), "o3");
        assert_eq!(
            doc.get("model_reasoning_effort").unwrap().as_str().unwrap(),
            "high"
        );
        assert!(doc.get("projects").is_some());
    }

    #[test]
    fn merge_into_empty_live_config() {
        let saved = r#"
model_provider = "provider_a"

[model_providers.provider_a]
base_url = "https://api.com/v1"
"#;
        let merged = merge_toml_provider_fields("", saved).unwrap();
        let doc = merged.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(
            doc.get("model_provider").unwrap().as_str().unwrap(),
            "provider_a"
        );
        assert!(doc
            .get("model_providers")
            .unwrap()
            .get("provider_a")
            .is_some());
    }

    #[test]
    fn merge_empty_saved_clears_provider_fields() {
        let live = r#"
model = "o3"
model_provider = "old_provider"
model_reasoning_effort = "high"

[model_providers.old_provider]
base_url = "https://old.api.com/v1"
"#;
        // Plan 账号没有 config → 传入空字符串
        let merged = merge_toml_provider_fields(live, "").unwrap();
        let doc = merged.parse::<toml_edit::DocumentMut>().unwrap();

        // provider 相关字段已清除
        assert!(doc.get("model_provider").is_none());
        assert!(doc.get("model_providers").is_none());

        // 其他设置保留
        assert_eq!(doc.get("model").unwrap().as_str().unwrap(), "o3");
        assert_eq!(
            doc.get("model_reasoning_effort").unwrap().as_str().unwrap(),
            "high"
        );
    }
}
