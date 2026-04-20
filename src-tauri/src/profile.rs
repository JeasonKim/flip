use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::file_ops;

// -- 数据结构 --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlipConfig {
    pub version: u32,
    #[serde(default)]
    pub claude: AgentConfig,
    #[serde(default)]
    pub codex: AgentConfig,
}

impl Default for FlipConfig {
    fn default() -> Self {
        Self {
            version: 1,
            claude: AgentConfig::default(),
            codex: AgentConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentConfig {
    pub current: Option<String>,
    #[serde(default)]
    pub accounts: Vec<Account>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    #[serde(rename = "type")]
    pub account_type: AccountType,
    pub label: String,
    /// Claude: OAuth 凭据（.credentials.json）或 API 连接信息（apiKey + baseUrl）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials: Option<serde_json::Value>,
    /// Codex: auth.json 内容
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<serde_json::Value>,
    /// Codex: config.toml 内容
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountType {
    Plan,
    Api,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Claude,
    Codex,
}

impl std::str::FromStr for AgentType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "claude" => Ok(AgentType::Claude),
            "codex" => Ok(AgentType::Codex),
            _ => Err(format!("unknown agent type: {}", s)),
        }
    }
}

// -- 文件路径 --

pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flip")
        .join("profiles.yaml")
}

// -- 读写 --

pub fn load_profiles() -> FlipConfig {
    let path = config_path();
    if !path.exists() {
        return FlipConfig::default();
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_yaml::from_str(&text).unwrap_or_default(),
        Err(_) => FlipConfig::default(),
    }
}

pub fn save_profiles(config: &FlipConfig) -> Result<(), String> {
    let path = config_path();
    let yaml = serde_yaml::to_string(config).map_err(|e| e.to_string())?;
    file_ops::write_text_file(&path, &yaml).map_err(|e| e.to_string())
}

// -- 操作 --

impl FlipConfig {
    pub fn agent_config(&self, agent: AgentType) -> &AgentConfig {
        match agent {
            AgentType::Claude => &self.claude,
            AgentType::Codex => &self.codex,
        }
    }

    pub fn agent_config_mut(&mut self, agent: AgentType) -> &mut AgentConfig {
        match agent {
            AgentType::Claude => &mut self.claude,
            AgentType::Codex => &mut self.codex,
        }
    }

    /// 添加或更新账号（ID 重复时更新凭据，而非拒绝）
    pub fn enroll_account(&mut self, agent: AgentType, account: Account) -> Result<(), String> {
        let cfg = self.agent_config_mut(agent);
        if let Some(existing) = cfg.accounts.iter_mut().find(|a| a.id == account.id) {
            // ID 已存在：更新凭据和标签（token 刷新等场景）
            existing.label = account.label;
            existing.credentials = account.credentials;
            existing.auth = account.auth;
            existing.config = account.config;
            return Ok(());
        }
        // 第一个账号自动设为当前
        if cfg.accounts.is_empty() {
            cfg.current = Some(account.id.clone());
        }
        cfg.accounts.push(account);
        Ok(())
    }

    /// 删除账号
    pub fn dismiss_account(&mut self, agent: AgentType, account_id: &str) -> Result<(), String> {
        let cfg = self.agent_config_mut(agent);
        let before = cfg.accounts.len();
        cfg.accounts.retain(|a| a.id != account_id);
        if cfg.accounts.len() == before {
            return Err(format!("account '{}' not found", account_id));
        }
        // 如果删的是当前激活账号，先清空，后续由 live 配置决定是否需要重新对齐
        if cfg.current.as_deref() == Some(account_id) {
            cfg.current = None;
        }
        Ok(())
    }

    /// 设置当前激活账号
    pub fn designate_active(&mut self, agent: AgentType, account_id: &str) -> Result<(), String> {
        let cfg = self.agent_config_mut(agent);
        if !cfg.accounts.iter().any(|a| a.id == account_id) {
            return Err(format!("account '{}' not found", account_id));
        }
        cfg.current = Some(account_id.to_string());
        Ok(())
    }

    /// 重命名账号
    pub fn rename_account(
        &mut self,
        agent: AgentType,
        account_id: &str,
        new_label: &str,
    ) -> Result<(), String> {
        let cfg = self.agent_config_mut(agent);
        let account = cfg
            .accounts
            .iter_mut()
            .find(|a| a.id == account_id)
            .ok_or_else(|| format!("account '{}' not found", account_id))?;
        account.label = new_label.to_string();
        Ok(())
    }

    /// 获取当前激活的账号
    pub fn active_account(&self, agent: AgentType) -> Option<&Account> {
        let cfg = self.agent_config(agent);
        let current_id = cfg.current.as_deref()?;
        cfg.accounts.iter().find(|a| a.id == current_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_account(id: &str, account_type: AccountType) -> Account {
        Account {
            id: id.to_string(),
            account_type,
            label: id.to_string(),
            credentials: None,
            auth: None,
            config: None,
        }
    }

    #[test]
    fn enroll_first_account_sets_current() {
        let mut cfg = FlipConfig::default();
        let acc = sample_account("user@test.com", AccountType::Plan);
        cfg.enroll_account(AgentType::Claude, acc).unwrap();
        assert_eq!(cfg.claude.current.as_deref(), Some("user@test.com"));
        assert_eq!(cfg.claude.accounts.len(), 1);
    }

    #[test]
    fn enroll_duplicate_updates_credentials() {
        let mut cfg = FlipConfig::default();
        let acc = sample_account("dup", AccountType::Api);
        cfg.enroll_account(AgentType::Claude, acc).unwrap();

        // 同 ID 再次注册：应更新 label 而非报错
        let mut updated = sample_account("dup", AccountType::Api);
        updated.label = "Updated Label".into();
        cfg.enroll_account(AgentType::Claude, updated).unwrap();
        assert_eq!(cfg.claude.accounts.len(), 1);
        assert_eq!(cfg.claude.accounts[0].label, "Updated Label");
    }

    #[test]
    fn dismiss_active_clears_current() {
        let mut cfg = FlipConfig::default();
        cfg.enroll_account(AgentType::Codex, sample_account("a", AccountType::Plan))
            .unwrap();
        cfg.enroll_account(AgentType::Codex, sample_account("b", AccountType::Api))
            .unwrap();
        cfg.designate_active(AgentType::Codex, "a").unwrap();
        cfg.dismiss_account(AgentType::Codex, "a").unwrap();
        assert_eq!(cfg.codex.current, None);
    }

    #[test]
    fn designate_unknown_rejected() {
        let mut cfg = FlipConfig::default();
        assert!(cfg
            .designate_active(AgentType::Claude, "nonexistent")
            .is_err());
    }

    #[test]
    fn rename_account_works() {
        let mut cfg = FlipConfig::default();
        cfg.enroll_account(AgentType::Claude, sample_account("test", AccountType::Api))
            .unwrap();
        cfg.rename_account(AgentType::Claude, "test", "New Name")
            .unwrap();
        assert_eq!(cfg.claude.accounts[0].label, "New Name");
    }

    #[test]
    fn yaml_roundtrip() {
        let mut cfg = FlipConfig::default();
        cfg.enroll_account(
            AgentType::Claude,
            Account {
                id: "user@test.com".into(),
                account_type: AccountType::Plan,
                label: "user@test.com".into(),
                credentials: Some(serde_json::json!({"claudeAiOauth": {"accessToken": "tok"}})),
                auth: None,
                config: None,
            },
        )
        .unwrap();

        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let restored: FlipConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(restored.claude.accounts.len(), 1);
        assert_eq!(restored.claude.accounts[0].id, "user@test.com");
        assert_eq!(restored.claude.accounts[0].account_type, AccountType::Plan);
    }
}
