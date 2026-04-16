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
    } else {
        // 截取域名
        url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
            .unwrap_or_else(|| url.to_string())
    }
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
    }
}
