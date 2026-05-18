pub mod claude;
pub mod codex;

/// 从 base_url 推断 API 提供方名称
pub fn infer_provider_from_url(url: &str) -> String {
    let url_lower = url.to_lowercase();
    if url_lower.contains("api.anthropic.com") {
        "Anthropic".into()
    } else if url_lower.contains("openrouter.ai") {
        "OpenRouter".into()
    } else if url_lower.contains("bedrock") || url_lower.contains("amazonaws.com") {
        "AWS Bedrock".into()
    } else if url_lower.contains("openai.azure.com") || url_lower.contains("azure") {
        "Azure OpenAI".into()
    } else if url_lower.contains("api.openai.com") {
        "OpenAI".into()
    } else if url_lower.contains("bigmodel.cn") {
        "Zhipu AI".into()
    } else {
        // 截取域名
        url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
            .unwrap_or_else(|| url.to_string())
    }
}

/// 确定性哈希，用于生成稳定的账号 ID 后缀
pub fn stable_id_suffix(s: &str) -> String {
    let mut h: u32 = 5381;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u32);
    }
    format!("{:04x}", h & 0xFFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_known_providers() {
        assert_eq!(
            infer_provider_from_url("https://api.anthropic.com/v1"),
            "Anthropic"
        );
        assert_eq!(
            infer_provider_from_url("https://openrouter.ai/api/v1"),
            "OpenRouter"
        );
        assert_eq!(
            infer_provider_from_url("https://bedrock-runtime.us-east-1.amazonaws.com"),
            "AWS Bedrock"
        );
        assert_eq!(
            infer_provider_from_url("https://my-resource.openai.azure.com/openai"),
            "Azure OpenAI"
        );
        assert_eq!(
            infer_provider_from_url("https://api.openai.com/v1"),
            "OpenAI"
        );
    }

    #[test]
    fn infer_unknown_extracts_domain() {
        assert_eq!(
            infer_provider_from_url("https://my-proxy.example.com/v1"),
            "my-proxy.example.com"
        );
        assert_eq!(
            infer_provider_from_url("https://open.bigmodel.cn/api/anthropic"),
            "Zhipu AI"
        );
    }
}
