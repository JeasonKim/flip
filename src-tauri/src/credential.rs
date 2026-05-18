use serde::{Deserialize, Serialize};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

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
    let output = run_security_command(&[
        "find-generic-password",
        "-s",
        "Claude Code-credentials",
        "-w",
    ])?;

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
    let file_credential = read_codex_from_file();
    #[cfg(target_os = "macos")]
    {
        return choose_codex_live_credential(file_credential, read_codex_from_keychain());
    }
    #[cfg(not(target_os = "macos"))]
    {
        file_credential
    }
}

fn choose_codex_live_credential(
    file_credential: Result<Credential, String>,
    keychain_credential: Result<Credential, String>,
) -> Result<Credential, String> {
    match file_credential {
        Ok(credential) => Ok(credential),
        Err(file_err) => match keychain_credential {
            Ok(credential) => {
                log::warn!(
                    "[credential] using Codex keychain credential because live auth file could not be read: {file_err}"
                );
                Ok(credential)
            }
            Err(keychain_err) => Err(format!(
                "codex credential unavailable: live auth file error: {file_err}; keychain error: {keychain_err}"
            )),
        },
    }
}

#[cfg(target_os = "macos")]
fn read_codex_from_keychain() -> Result<Credential, String> {
    let output = run_security_command(&["find-generic-password", "-s", "Codex Auth", "-w"])?;

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

    let expired = codex_access_token_expired(&access_token).unwrap_or_else(|| {
        val.get("last_refresh")
            .and_then(|v| v.as_str())
            .map(|s| is_codex_token_stale(s))
            .unwrap_or(false)
    });

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
    let output = match run_security_command(&["find-generic-password", "-s", service, "-w"]) {
        Ok(output) => output,
        Err(err) => {
            log::warn!("[credential] keychain raw read failed service={service}: {err}");
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!(
            "[credential] keychain raw read returned non-success service={service}: {stderr}"
        );
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
    let output = match run_security_command(&["find-generic-password", "-s", service]) {
        Ok(output) => output,
        Err(err) => {
            log::warn!("[credential] keychain account read failed service={service}: {err}");
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!(
            "[credential] keychain account read returned non-success service={service}: {stderr}"
        );
        return None;
    }

    // 从输出中解析 "acct"<blob>="xxx" 行
    let text = if output.stdout.is_empty() {
        String::from_utf8(output.stderr).ok()?
    } else {
        String::from_utf8(output.stdout).ok()?
    };
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
    let output = run_security_command(&[
        "add-generic-password",
        "-U",
        "-s",
        service,
        "-a",
        &account,
        "-w",
        &json_str,
    ])?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("keychain write failed: {stderr}"));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
const SECURITY_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(target_os = "macos")]
fn run_security_command(args: &[&str]) -> Result<Output, String> {
    run_command_with_timeout("security", args, SECURITY_COMMAND_TIMEOUT)
}

fn run_command_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<Output, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn command program={program}: {e}"))?;

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child.wait_with_output().map_err(|e| {
                    format!("failed to collect command output program={program}: {e}")
                });
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    if let Err(err) = child.kill() {
                        log::warn!(
                            "[credential] failed to kill timed out command program={program} args={args:?}: {err}"
                        );
                    }
                    let _ = child.wait();
                    return Err(format!(
                        "command timed out program={program} timeout_ms={} args={args:?}",
                        timeout.as_millis()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) => {
                if let Err(kill_err) = child.kill() {
                    log::warn!(
                        "[credential] failed to kill errored command program={program} args={args:?}: {kill_err}"
                    );
                }
                let _ = child.wait();
                return Err(format!(
                    "failed while waiting for command program={program} args={args:?}: {err}"
                ));
            }
        }
    }
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

fn codex_access_token_expired(access_token: &str) -> Option<bool> {
    let payload = access_token.split('.').nth(1)?;
    let decoded = base64_url_decode(payload)?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    let exp = claims.get("exp")?.as_u64()?;

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Some(exp < now_secs)
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
    fn parse_codex_credential_prefers_jwt_exp_over_stale_last_refresh() {
        let access_token = jwt_with_exp(4_102_444_800);
        let json = format!(
            r#"{{"auth_mode":"chatgpt","tokens":{{"access_token":"{}","account_id":"user-123"}},"last_refresh":"2020-01-01T00:00:00Z"}}"#,
            access_token
        );

        let cred = parse_codex_credential_json(&json).unwrap();

        assert!(!cred.expired);
    }

    #[test]
    fn parse_codex_credential_marks_expired_jwt_as_expired() {
        let access_token = jwt_with_exp(946_684_800);
        let json = format!(
            r#"{{"auth_mode":"chatgpt","tokens":{{"access_token":"{}","account_id":"user-123"}},"last_refresh":"2099-01-01T00:00:00Z"}}"#,
            access_token
        );

        let cred = parse_codex_credential_json(&json).unwrap();

        assert!(cred.expired);
    }

    #[test]
    fn codex_credential_prefers_live_file_over_keychain() {
        let live_file = Ok(Credential {
            access_token: "file-token".into(),
            account_id: Some("file-account".into()),
            expired: false,
        });
        let keychain = Ok(Credential {
            access_token: "keychain-token".into(),
            account_id: Some("keychain-account".into()),
            expired: true,
        });

        let cred = choose_codex_live_credential(live_file, keychain).unwrap();

        assert_eq!(cred.access_token, "file-token");
        assert_eq!(cred.account_id.as_deref(), Some("file-account"));
        assert!(!cred.expired);
    }

    #[test]
    fn codex_credential_falls_back_to_keychain_when_live_file_missing() {
        let cred = choose_codex_live_credential(
            Err("auth file not found".into()),
            Ok(Credential {
                access_token: "keychain-token".into(),
                account_id: Some("keychain-account".into()),
                expired: false,
            }),
        )
        .unwrap();

        assert_eq!(cred.access_token, "keychain-token");
        assert_eq!(cred.account_id.as_deref(), Some("keychain-account"));
    }

    #[test]
    fn codex_stale_token_detection() {
        assert!(is_codex_token_stale("2020-01-01T00:00:00Z"));
        assert!(!is_codex_token_stale("2099-01-01T00:00:00Z"));
    }

    #[test]
    fn command_timeout_helper_returns_output_before_deadline() {
        let output =
            run_command_with_timeout("sh", &["-c", "printf hello"], Duration::from_secs(1))
                .unwrap();
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "hello");
    }

    #[test]
    fn command_timeout_helper_aborts_slow_command() {
        let err = run_command_with_timeout("sh", &["-c", "sleep 1"], Duration::from_millis(100))
            .unwrap_err();
        assert!(err.contains("timed out"));
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

    fn jwt_with_exp(exp: u64) -> String {
        let payload = format!(r#"{{"exp":{exp}}}"#);
        format!(
            "eyJhbGciOiJSUzI1NiJ9.{}.sig",
            base64_url_encode(payload.as_bytes())
        )
    }

    fn base64_url_encode(bytes: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut output = String::new();
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0];
            let b1 = *chunk.get(1).unwrap_or(&0);
            let b2 = *chunk.get(2).unwrap_or(&0);
            let triple = ((b0 as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;

            output.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
            output.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);
            if chunk.len() >= 2 {
                output.push(TABLE[((triple >> 6) & 0x3f) as usize] as char);
            }
            if chunk.len() == 3 {
                output.push(TABLE[(triple & 0x3f) as usize] as char);
            }
        }
        output
    }
}
