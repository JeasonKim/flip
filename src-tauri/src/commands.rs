use serde::{Deserialize, Serialize};
use tauri::Manager;

// -- Profile 命令 --

#[tauri::command]
pub async fn list_profiles() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "claude": { "current": null, "accounts": [] }, "codex": { "current": null, "accounts": [] } }))
}

#[tauri::command]
pub async fn flip_account(agent: String, account_id: String) -> Result<(), String> {
    let _ = (&agent, &account_id);
    Ok(())
}

#[tauri::command]
pub async fn capture_current(agent: String) -> Result<serde_json::Value, String> {
    let _ = &agent;
    Err("Not implemented yet".into())
}

#[tauri::command]
pub async fn dismiss_account(agent: String, account_id: String) -> Result<(), String> {
    let _ = (&agent, &account_id);
    Ok(())
}

#[tauri::command]
pub async fn detect_unsaved(agent: String) -> Result<bool, String> {
    let _ = &agent;
    Ok(false)
}

#[tauri::command]
pub async fn fetch_quota(agent: String) -> Result<serde_json::Value, String> {
    let _ = &agent;
    Ok(serde_json::json!({ "tiers": [] }))
}

#[tauri::command]
pub async fn rename_account(
    agent: String,
    account_id: String,
    new_label: String,
) -> Result<(), String> {
    let _ = (&agent, &account_id, &new_label);
    Ok(())
}

// -- Session 命令 --

#[derive(Serialize, Deserialize)]
pub struct SessionMeta {
    pub agent: String,
    pub session_id: String,
    pub title: String,
    pub project_dir: Option<String>,
    pub last_active_at: Option<i64>,
    pub source_path: String,
}

#[tauri::command]
pub async fn scan_sessions(
    agent_filter: Option<String>,
    project_filter: Option<String>,
    offset: usize,
    limit: usize,
) -> Result<Vec<SessionMeta>, String> {
    let _ = (&agent_filter, &project_filter, offset, limit);
    Ok(vec![])
}

#[tauri::command]
pub async fn load_session_messages(
    agent: String,
    source_path: String,
) -> Result<Vec<serde_json::Value>, String> {
    let _ = (&agent, &source_path);
    Ok(vec![])
}

#[tauri::command]
pub async fn list_session_projects() -> Result<Vec<String>, String> {
    Ok(vec![])
}

#[tauri::command]
pub async fn open_session_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("sessions") {
        win.show().map_err(|e| e.to_string())?;
        win.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        "sessions",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("Flip — Sessions")
    .inner_size(960.0, 640.0)
    .min_inner_size(640.0, 400.0)
    .build()
    .map_err(|e| e.to_string())?;

    Ok(())
}
