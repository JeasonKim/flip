use tauri::Manager;

use crate::credential;
use crate::profile::{self, Account, AgentType};
use crate::quota;
use crate::session;

// -- Profile 命令 --

#[tauri::command]
pub async fn list_profiles() -> Result<serde_json::Value, String> {
    let config = profile::load_profiles();
    serde_json::to_value(&config).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn flip_account(agent: String, account_id: String) -> Result<(), String> {
    let agent_type: AgentType = agent.parse()?;
    let mut config = profile::load_profiles();
    config.designate_active(agent_type, &account_id)?;

    // 将账号凭证写入 Agent 的 live 文件
    let account = config
        .active_account(agent_type)
        .ok_or("account not found after designate")?
        .clone();
    match agent_type {
        AgentType::Claude => crate::agent::claude::apply_profile(&account)?,
        AgentType::Codex => crate::agent::codex::apply_profile(&account)?,
    }

    profile::save_profiles(&config)?;
    Ok(())
}

#[tauri::command]
pub async fn capture_current(agent: String) -> Result<serde_json::Value, String> {
    let agent_type: AgentType = agent.parse()?;
    let mut config = profile::load_profiles();

    let account = match agent_type {
        AgentType::Claude => capture_claude_account().await?,
        AgentType::Codex => capture_codex_account()?,
    };

    let account_value = serde_json::to_value(&account).map_err(|e| e.to_string())?;
    config.enroll_account(agent_type, account)?;
    profile::save_profiles(&config)?;

    Ok(account_value)
}

/// 从 Claude Code live 配置捕获账号（身份 + 凭证）
async fn capture_claude_account() -> Result<Account, String> {
    let (settings, credentials) = crate::agent::claude::snapshot_live()?;
    let account_type = crate::agent::claude::detect_account_type(&settings, &credentials);
    let mut label = crate::agent::claude::infer_label(&settings, &credentials, &account_type);

    // Plan 账号：调 OAuth profile API 获取真实用户名
    if account_type == profile::AccountType::Plan {
        if let Ok(cred) = crate::credential::read_claude_credential() {
            if let Some(identity) =
                crate::agent::claude::resolve_plan_identity(&cred.access_token).await
            {
                label = identity;
            }
        }
    }

    let id = crate::agent::claude::generate_account_id(&account_type, &label);

    // 提取凭证：Plan → OAuth，API → apiKey + baseUrl
    let saved_credentials =
        crate::agent::claude::extract_credentials(&settings, &credentials, &account_type);

    Ok(Account {
        id,
        account_type,
        label,
        credentials: saved_credentials,
        auth: None,
        config: None,
    })
}

/// 从 Codex live 配置捕获账号（身份 + 凭证）
fn capture_codex_account() -> Result<Account, String> {
    let (auth, config_text) = crate::agent::codex::snapshot_live()?;
    let account_type = crate::agent::codex::detect_account_type(&auth);
    let label = crate::agent::codex::infer_label(&auth, &config_text, &account_type);
    let id = crate::agent::codex::generate_account_id(&account_type, &label);

    // API 账号只保留 [model_providers] 段，去掉冗余的顶层字段和 [projects] 段
    let trimmed_config = match account_type {
        profile::AccountType::Api => config_text
            .as_deref()
            .map(crate::agent::codex::extract_api_config),
        profile::AccountType::Plan => config_text,
    };

    Ok(Account {
        id,
        account_type,
        label,
        credentials: None,
        auth,
        config: trimmed_config,
    })
}

#[tauri::command]
pub async fn dismiss_account(agent: String, account_id: String) -> Result<(), String> {
    let agent_type: AgentType = agent.parse()?;
    let mut config = profile::load_profiles();
    config.dismiss_account(agent_type, &account_id)?;
    profile::save_profiles(&config)?;
    Ok(())
}

/// 检测当前 live 配置的身份是否尚未被捕获
/// 直接比对凭证指纹，不走网络调用，避免 API 超时导致误报
#[tauri::command]
pub async fn detect_unsaved(agent: String) -> Result<bool, String> {
    let agent_type: AgentType = agent.parse()?;
    let config = profile::load_profiles();
    let accounts = &config.agent_config(agent_type).accounts;

    match agent_type {
        AgentType::Claude => detect_claude_unsaved(accounts),
        AgentType::Codex => detect_codex_unsaved(accounts),
    }
}

/// Claude：Plan 比对 refreshToken，API 比对 apiKey
fn detect_claude_unsaved(accounts: &[Account]) -> Result<bool, String> {
    let (settings, credentials) = crate::agent::claude::snapshot_live()?;
    let account_type = crate::agent::claude::detect_account_type(&settings, &credentials);

    match account_type {
        profile::AccountType::Plan => {
            let live_rt = credentials
                .as_ref()
                .and_then(|c| c.get("claudeAiOauth").or_else(|| c.get("claude.ai_oauth")))
                .and_then(|o| o.get("refreshToken"))
                .and_then(|v| v.as_str());
            let Some(live_rt) = live_rt else {
                return Ok(true);
            };
            let found = accounts.iter().any(|a| {
                a.account_type == profile::AccountType::Plan
                    && a.credentials
                        .as_ref()
                        .and_then(|c| {
                            c.get("claudeAiOauth").or_else(|| c.get("claude.ai_oauth"))
                        })
                        .and_then(|o| o.get("refreshToken"))
                        .and_then(|v| v.as_str())
                        == Some(live_rt)
            });
            Ok(!found)
        }
        profile::AccountType::Api => {
            let live_key = settings
                .as_ref()
                .and_then(|s| s.get("apiKey"))
                .and_then(|v| v.as_str());
            let Some(live_key) = live_key else {
                return Ok(true);
            };
            let found = accounts.iter().any(|a| {
                a.account_type == profile::AccountType::Api
                    && a.credentials
                        .as_ref()
                        .and_then(|c| c.get("apiKey"))
                        .and_then(|v| v.as_str())
                        == Some(live_key)
            });
            Ok(!found)
        }
    }
}

/// Codex：Plan 比对 account_id，API 比对 OPENAI_API_KEY
fn detect_codex_unsaved(accounts: &[Account]) -> Result<bool, String> {
    let (auth, _config_text) = crate::agent::codex::snapshot_live()?;
    let account_type = crate::agent::codex::detect_account_type(&auth);

    match account_type {
        profile::AccountType::Plan => {
            let live_account_id = auth
                .as_ref()
                .and_then(|a| a.get("tokens"))
                .and_then(|t| t.get("account_id"))
                .and_then(|v| v.as_str());
            let Some(live_id) = live_account_id else {
                return Ok(true);
            };
            let found = accounts.iter().any(|a| {
                a.account_type == profile::AccountType::Plan
                    && a.auth
                        .as_ref()
                        .and_then(|a| a.get("tokens"))
                        .and_then(|t| t.get("account_id"))
                        .and_then(|v| v.as_str())
                        == Some(live_id)
            });
            Ok(!found)
        }
        profile::AccountType::Api => {
            let live_key = auth
                .as_ref()
                .and_then(|a| a.get("OPENAI_API_KEY"))
                .and_then(|v| v.as_str());
            let Some(live_key) = live_key else {
                return Ok(true);
            };
            let found = accounts.iter().any(|a| {
                a.account_type == profile::AccountType::Api
                    && a.auth
                        .as_ref()
                        .and_then(|a| a.get("OPENAI_API_KEY"))
                        .and_then(|v| v.as_str())
                        == Some(live_key)
            });
            Ok(!found)
        }
    }
}

#[tauri::command]
pub async fn fetch_quota(agent: String) -> Result<serde_json::Value, String> {
    let agent_type: AgentType = agent.parse()?;

    let result = match agent_type {
        AgentType::Claude => {
            let cred = credential::read_claude_credential()?;
            if cred.expired {
                return Err("Claude credential expired".into());
            }
            quota::fetch_claude_quota(&cred.access_token).await
        }
        AgentType::Codex => {
            let cred = credential::read_codex_credential()?;
            if cred.expired {
                return Err("Codex credential expired".into());
            }
            quota::fetch_codex_quota(&cred.access_token, cred.account_id.as_deref()).await
        }
    };

    serde_json::to_value(&result).map_err(|e| e.to_string())
}

/// 读取 Agent 当前 live 配置中的模型和推理等级
#[tauri::command]
pub async fn read_model_info(agent: String) -> Result<serde_json::Value, String> {
    let agent_type: AgentType = agent.parse()?;
    match agent_type {
        AgentType::Claude => read_claude_model_info(),
        AgentType::Codex => read_codex_model_info(),
    }
}

fn read_claude_model_info() -> Result<serde_json::Value, String> {
    let settings_path = crate::agent::claude::settings_path();
    let settings = if settings_path.exists() {
        let text = std::fs::read_to_string(&settings_path).map_err(|e| e.to_string())?;
        serde_json::from_str::<serde_json::Value>(&text).ok()
    } else {
        None
    };

    let model = settings
        .as_ref()
        .and_then(|s| s.get("model"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let thinking = settings
        .as_ref()
        .and_then(|s| s.get("env"))
        .and_then(|e| e.get("CLAUDE_CODE_EFFORT_LEVEL"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(serde_json::json!({ "model": model, "thinking": thinking }))
}

fn read_codex_model_info() -> Result<serde_json::Value, String> {
    let config_path = crate::agent::codex::config_path();
    let doc = if config_path.exists() {
        let text = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
        text.parse::<toml_edit::DocumentMut>().ok()
    } else {
        None
    };

    let model = doc
        .as_ref()
        .and_then(|d| d.get("model"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let thinking = doc
        .as_ref()
        .and_then(|d| d.get("model_reasoning_effort"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(serde_json::json!({ "model": model, "thinking": thinking }))
}

#[tauri::command]
pub async fn rename_account(
    agent: String,
    account_id: String,
    new_label: String,
) -> Result<(), String> {
    let agent_type: AgentType = agent.parse()?;
    let mut config = profile::load_profiles();
    config.rename_account(agent_type, &account_id, &new_label)?;
    profile::save_profiles(&config)?;
    Ok(())
}

// -- Session 命令 --

#[tauri::command]
pub async fn scan_sessions(
    agent_filter: Option<String>,
    offset: usize,
    limit: usize,
) -> Result<Vec<session::SessionMeta>, String> {
    Ok(session::scan_sessions(
        agent_filter.as_deref(),
        offset,
        limit,
    ))
}

#[tauri::command]
pub async fn load_session_messages(
    agent: String,
    source_path: String,
) -> Result<Vec<session::SessionMessage>, String> {
    session::load_messages(&agent, &source_path)
}

/// 在终端中恢复会话
#[tauri::command]
pub async fn resume_session(command: String, cwd: Option<String>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || session::launch_terminal(&command, cwd.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_session_window(app: tauri::AppHandle) -> Result<(), String> {
    // 切换到 Regular 策略，使会话窗口拥有正常窗口行为（全屏、保持可见）
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);

    if let Some(win) = app.get_webview_window("sessions") {
        win.show().map_err(|e| e.to_string())?;
        win.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    let win = tauri::WebviewWindowBuilder::new(
        &app,
        "sessions",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("Flip — Sessions")
    .inner_size(960.0, 640.0)
    .min_inner_size(640.0, 400.0)
    .build()
    .map_err(|e| e.to_string())?;

    // 关闭会话窗口时切回 Accessory 策略
    let app_handle = app.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            #[cfg(target_os = "macos")]
            let _ = app_handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
        }
    });

    Ok(())
}

/// 清除指定天数之前的会话文件
#[tauri::command]
pub async fn purge_sessions(older_than_days: u32) -> Result<u32, String> {
    let count = session::purge_sessions(older_than_days);
    Ok(count as u32)
}

/// 从 CC Switch 导入账号
#[tauri::command]
pub async fn import_from_ccswitch() -> Result<serde_json::Value, String> {
    let result = crate::import::import_from_ccswitch()?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

/// 手动添加 API 账号
#[tauri::command]
pub async fn enroll_api_account(
    agent: String,
    label: String,
    api_key: String,
    base_url: Option<String>,
) -> Result<serde_json::Value, String> {
    let agent_type: profile::AgentType = agent.parse()?;
    let mut config = profile::load_profiles();
    let account = crate::import::enroll_api_account(
        &mut config,
        agent_type,
        label,
        api_key,
        base_url,
    )?;
    serde_json::to_value(&account).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reveal_config_file() -> Result<(), String> {
    let path = profile::config_path();
    if !path.exists() {
        // 没有配置文件时先生成一份默认的
        let config = profile::load_profiles();
        profile::save_profiles(&config)?;
    }
    open::that(&path).map_err(|e| format!("failed to open config: {}", e))
}
