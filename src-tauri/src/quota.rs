use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaTier {
    pub name: String,
    /// 利用率百分比 0-100
    pub utilization: f64,
    /// ISO 8601 重置时间
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaResult {
    pub success: bool,
    pub tiers: Vec<QuotaTier>,
    pub error: Option<String>,
}

// -- Claude Code 限额查询 --

pub async fn fetch_claude_quota(access_token: &str) -> QuotaResult {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            return QuotaResult {
                success: false,
                tiers: vec![],
                error: Some(format!("request failed: {}", e)),
            }
        }
    };

    if !resp.status().is_success() {
        return QuotaResult {
            success: false,
            tiers: vec![],
            error: Some(format!("HTTP {}", resp.status())),
        };
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return QuotaResult {
                success: false,
                tiers: vec![],
                error: Some(format!("parse error: {}", e)),
            }
        }
    };

    // 只展示明确有意义的 tier，忽略实验性/内部 tier
    let display_tiers = ["five_hour", "seven_day", "seven_day_opus", "seven_day_sonnet"];
    let mut tiers = Vec::new();

    for tier_name in &display_tiers {
        if let Some(window) = body.get(tier_name) {
            if let Some(tier) = parse_claude_tier(tier_name, window) {
                tiers.push(tier);
            }
        }
    }

    QuotaResult {
        success: true,
        tiers,
        error: None,
    }
}

fn parse_claude_tier(name: &str, value: &serde_json::Value) -> Option<QuotaTier> {
    let utilization = value.get("utilization")?.as_f64()?;
    let resets_at = value
        .get("resets_at")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Some(QuotaTier {
        name: name.to_string(),
        utilization,
        resets_at,
    })
}

// -- Codex 限额查询 --

pub async fn fetch_codex_quota(access_token: &str, account_id: Option<&str>) -> QuotaResult {
    let client = reqwest::Client::new();
    let mut req = client
        .get("https://chatgpt.com/backend-api/wham/usage")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("User-Agent", "codex-cli")
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(10));

    if let Some(aid) = account_id {
        req = req.header("ChatGPT-Account-Id", aid);
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return QuotaResult {
                success: false,
                tiers: vec![],
                error: Some(format!("request failed: {}", e)),
            }
        }
    };

    if !resp.status().is_success() {
        return QuotaResult {
            success: false,
            tiers: vec![],
            error: Some(format!("HTTP {}", resp.status())),
        };
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return QuotaResult {
                success: false,
                tiers: vec![],
                error: Some(format!("parse error: {}", e)),
            }
        }
    };

    let mut tiers = Vec::new();
    if let Some(rate_limit) = body.get("rate_limit") {
        for key in ["primary_window", "secondary_window"] {
            if let Some(window) = rate_limit.get(key) {
                if let Some(tier) = parse_codex_window(window) {
                    tiers.push(tier);
                }
            }
        }
    }

    QuotaResult {
        success: true,
        tiers,
        error: None,
    }
}

fn parse_codex_window(window: &serde_json::Value) -> Option<QuotaTier> {
    let used_percent = window.get("used_percent")?.as_f64()?;
    let window_secs = window.get("limit_window_seconds")?.as_i64()?;
    let reset_at = window
        .get("reset_at")
        .and_then(|v| v.as_i64())
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
        .map(|dt| dt.to_rfc3339());

    let name = match window_secs {
        18000 => "five_hour".to_string(),
        604800 => "seven_day".to_string(),
        s => {
            let hours = s / 3600;
            if hours >= 24 {
                format!("{}_day", hours / 24)
            } else {
                format!("{}_hour", hours)
            }
        }
    };

    Some(QuotaTier {
        name,
        utilization: used_percent,
        resets_at: reset_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_claude_tier_extracts_data() {
        let value = serde_json::json!({
            "utilization": 78.5,
            "resets_at": "2026-04-16T15:30:00Z"
        });
        let tier = parse_claude_tier("five_hour", &value).unwrap();
        assert_eq!(tier.name, "five_hour");
        assert!((tier.utilization - 78.5).abs() < f64::EPSILON);
        assert_eq!(tier.resets_at.as_deref(), Some("2026-04-16T15:30:00Z"));
    }

    #[test]
    fn parse_codex_window_maps_seconds_to_name() {
        let window = serde_json::json!({
            "used_percent": 45.5,
            "limit_window_seconds": 18000,
            "reset_at": 1713326400
        });
        let tier = parse_codex_window(&window).unwrap();
        assert_eq!(tier.name, "five_hour");
        assert!((tier.utilization - 45.5).abs() < f64::EPSILON);
        assert!(tier.resets_at.is_some());
    }

    #[test]
    fn parse_codex_seven_day() {
        let window = serde_json::json!({
            "used_percent": 12.3,
            "limit_window_seconds": 604800,
            "reset_at": 1713412800
        });
        let tier = parse_codex_window(&window).unwrap();
        assert_eq!(tier.name, "seven_day");
    }
}
