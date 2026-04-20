use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub access_token: String,
    pub account_id: Option<String>,
    pub expired: bool,
}

// -- Claude Code 凭据读取 --

pub fn read_claude_credential() -> Result<Credential, String> {
    // 优先 Keychain
    #[cfg(target_os = "macos")]
    {
        if let Ok(cred) = read_claude_from_keychain() {
            return Ok(cred);
        }
    }
    // Fallback: 文件
    read_claude_from_file()
}

#[cfg(target_os = "macos")]
fn read_claude_from_keychain() -> Result<Credential, String> {
    let output = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .map_err(|e| format!("failed to run security command: {e}"))?;

    if !output.status.success() {
        return Err("keychain: no matching credential found".into());
    }

    let text = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    let text = text.trim();
    if text.is_empty() {
        return Err("keychain: empty credential".into());
    }

    parse_claude_credential_json(text)
}

fn read_claude_from_file() -> Result<Credential, String> {
    let path = crate::agent::claude::credentials_path();
    if !path.exists() {
        return Err("credentials file not found".into());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    parse_claude_credential_json(&text)
}

fn parse_claude_credential_json(json_text: &str) -> Result<Credential, String> {
    let val: serde_json::Value = serde_json::from_str(json_text).map_err(|e| e.to_string())?;

    // 支持两种键名
    let oauth = val
        .get("claudeAiOauth")
        .or_else(|| val.get("claude.ai_oauth"))
        .ok_or("no OAuth credentials found")?;

    let access_token = oauth
        .get("accessToken")
        .and_then(|v| v.as_str())
        .ok_or("missing accessToken")?
        .to_string();

    let expired = oauth
        .get("expiresAt")
        .map(|v| is_timestamp_expired(v))
        .unwrap_or(false);

    Ok(Credential {
        access_token,
        account_id: None,
        expired,
    })
}

// -- Codex 凭据读取 --

pub fn read_codex_credential() -> Result<Credential, String> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(cred) = read_codex_from_keychain() {
            return Ok(cred);
        }
    }
    read_codex_from_file()
}

#[cfg(target_os = "macos")]
fn read_codex_from_keychain() -> Result<Credential, String> {
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-s", "Codex Auth", "-w"])
        .output()
        .map_err(|e| format!("failed to run security command: {e}"))?;

    if !output.status.success() {
        return Err("keychain: no matching credential found".into());
    }

    let text = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    let text = text.trim();
    if text.is_empty() {
        return Err("keychain: empty credential".into());
    }

    parse_codex_credential_json(text)
}

fn read_codex_from_file() -> Result<Credential, String> {
    let path = crate::agent::codex::auth_path();
    if !path.exists() {
        return Err("auth file not found".into());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    parse_codex_credential_json(&text)
}

fn parse_codex_credential_json(json_text: &str) -> Result<Credential, String> {
    let val: serde_json::Value = serde_json::from_str(json_text).map_err(|e| e.to_string())?;

    let tokens = val.get("tokens").ok_or("missing tokens")?;
    let access_token = tokens
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("missing access_token")?
        .to_string();
    let account_id = tokens
        .get("account_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Codex token 过期：last_refresh 超过 8 天
    let expired = val
        .get("last_refresh")
        .and_then(|v| v.as_str())
        .map(|s| is_codex_token_stale(s))
        .unwrap_or(false);

    Ok(Credential {
        access_token,
        account_id,
        expired,
    })
}

// -- macOS Keychain 原始 JSON 读写（供 snapshot_live / apply_profile 使用） --

/// 从 macOS Keychain 读取 Claude Code 凭据的原始 JSON
#[cfg(target_os = "macos")]
pub fn read_claude_keychain_raw() -> Option<serde_json::Value> {
    read_keychain_raw_json("Claude Code-credentials")
}

/// 将 JSON 写入 macOS Keychain 的 Claude Code 凭据条目
#[cfg(target_os = "macos")]
pub fn write_claude_keychain(json: &serde_json::Value) -> Result<(), String> {
    write_keychain_json("Claude Code-credentials", json)
}

#[cfg(target_os = "macos")]
fn read_keychain_raw_json(service: &str) -> Option<serde_json::Value> {
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-s", service, "-w"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    serde_json::from_str(text).ok()
}

#[cfg(target_os = "macos")]
fn read_keychain_account(service: &str) -> Option<String> {
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-s", service])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // 从输出中解析 "acct"<blob>="xxx" 行
    let text = String::from_utf8(output.stdout).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("\"acct\"") {
            // 格式: "acct"<blob>="username"
            if let Some(val) = line.rsplit('=').next() {
                let val = val.trim().trim_matches('"');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn write_keychain_json(service: &str, json: &serde_json::Value) -> Result<(), String> {
    let json_str = serde_json::to_string(json).map_err(|e| e.to_string())?;

    // 读取现有条目的 account name，保持一致
    let account = read_keychain_account(service).unwrap_or_else(|| "default".into());

    // -U: 如果已存在则更新
    let output = std::process::Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            service,
            "-a",
            &account,
            "-w",
            &json_str,
        ])
        .output()
        .map_err(|e| format!("failed to run security command: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("keychain write failed: {stderr}"));
    }
    Ok(())
}

// -- 时间工具 --

/// 判断时间戳是否已过期，支持三种格式：毫秒、秒、ISO 8601
fn is_timestamp_expired(value: &serde_json::Value) -> bool {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    match value {
        serde_json::Value::Number(n) => {
            if let Some(ts) = n.as_f64() {
                let ts_secs = if ts >= 1_000_000_000_000.0 {
                    (ts / 1000.0) as u64
                } else {
                    ts as u64
                };
                ts_secs < now_secs
            } else {
                false
            }
        }
        serde_json::Value::String(s) => chrono::DateTime::parse_from_rfc3339(s)
            .map(|dt| (dt.timestamp() as u64) < now_secs)
            .unwrap_or(false),
        _ => false,
    }
}

/// Codex token 过期判断：last_refresh 超过 8 天
fn is_codex_token_stale(last_refresh: &str) -> bool {
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(last_refresh) else {
        return false;
    };
    let now = chrono::Utc::now();
    let age = now.signed_duration_since(dt);
    age.num_seconds() > 8 * 24 * 3600
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_claude_credential_claudeaioauth_key() {
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-test","expiresAt":9999999999999}}"#;
        let cred = parse_claude_credential_json(json).unwrap();
        assert_eq!(cred.access_token, "sk-test");
        assert!(!cred.expired);
    }

    #[test]
    fn parse_claude_credential_alternate_key() {
        let json = r#"{"claude.ai_oauth":{"accessToken":"sk-alt","expiresAt":1000000000}}"#;
        let cred = parse_claude_credential_json(json).unwrap();
        assert_eq!(cred.access_token, "sk-alt");
        assert!(cred.expired); // 过去的时间戳
    }

    #[test]
    fn parse_codex_credential_extracts_account_id() {
        let json = r#"{"auth_mode":"chatgpt","tokens":{"access_token":"eyJ","account_id":"user-123"},"last_refresh":"2099-01-01T00:00:00Z"}"#;
        let cred = parse_codex_credential_json(json).unwrap();
        assert_eq!(cred.access_token, "eyJ");
        assert_eq!(cred.account_id.as_deref(), Some("user-123"));
        assert!(!cred.expired);
    }

    #[test]
    fn codex_stale_token_detection() {
        assert!(is_codex_token_stale("2020-01-01T00:00:00Z"));
        assert!(!is_codex_token_stale("2099-01-01T00:00:00Z"));
    }

    #[test]
    fn timestamp_expired_milliseconds() {
        // 很久以前的毫秒时间戳
        let val = serde_json::json!(1000000000000i64);
        assert!(is_timestamp_expired(&val));
    }

    #[test]
    fn timestamp_expired_iso8601() {
        let val = serde_json::json!("2020-01-01T00:00:00Z");
        assert!(is_timestamp_expired(&val));
    }

    #[test]
    fn timestamp_not_expired_future() {
        let val = serde_json::json!(9999999999999i64);
        assert!(!is_timestamp_expired(&val));
    }
}
