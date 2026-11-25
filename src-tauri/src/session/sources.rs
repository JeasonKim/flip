use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_SOURCE_ID: &str = "codex-default";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSourceConfig {
    pub id: String,
    pub label: String,
    pub path: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSource {
    #[serde(flatten)]
    pub config: SessionSourceConfig,
    pub is_default: bool,
    pub issue: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSourceRef {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionSourcesConfig {
    #[serde(default)]
    codex: Vec<SessionSourceConfig>,
}

fn enabled_by_default() -> bool {
    true
}

pub fn config_path() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|home| home.join(".flip/session-sources.yaml"))
        .ok_or_else(|| "无法确定用户目录".into())
}

pub fn native_codex_home() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|home| home.join(".codex"))
        .ok_or_else(|| "无法确定 Codex 默认目录".into())
}

pub fn configured_sources() -> Result<Vec<SessionSource>, String> {
    let path = config_path()?;
    let native = native_codex_home()?;
    read_config(&path)
        .and_then(|config| resolve_sources(config, &native))
        .map_err(|error| {
            log::warn!(
                "[session-sources] configuration unavailable path={}: {}",
                path.display(),
                error
            );
            error
        })
}

pub fn configure_sources(sources: Vec<SessionSourceConfig>) -> Result<Vec<SessionSource>, String> {
    let path = config_path()?;
    let native = native_codex_home()?;
    persist_config(&path, &native, sources)?;
    configured_sources()
}

fn read_config(path: &Path) -> Result<SessionSourcesConfig, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_yaml::from_str(&text)
            .map_err(|error| format!("采集目录配置格式错误（{}）：{}", path.display(), error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(SessionSourcesConfig::default())
        }
        Err(error) => Err(format!(
            "读取采集目录配置失败（{}）：{}",
            path.display(),
            error
        )),
    }
}

fn persist_config(
    path: &Path,
    native: &Path,
    sources: Vec<SessionSourceConfig>,
) -> Result<(), String> {
    let previous = read_config(path)?;
    let config = SessionSourcesConfig {
        codex: validate_sources(sources, native)?,
    };
    let text = serde_yaml::to_string(&config).map_err(|error| error.to_string())?;
    crate::file_ops::write_text_file(path, &text)
        .map_err(|error| format!("保存采集目录配置失败：{}", error))?;
    for source in &previous.codex {
        let next = config.codex.iter().find(|next| next.id == source.id);
        if next.map(|next| (&next.label, &next.path, next.enabled))
            != Some((&source.label, &source.path, source.enabled))
        {
            log::info!("[session-sources] source configuration changed id={} previous={:?} next={:?}; reason=user configuration; source files retained", source.id, source, next);
        }
    }
    Ok(())
}

fn validate_sources(
    mut sources: Vec<SessionSourceConfig>,
    native: &Path,
) -> Result<Vec<SessionSourceConfig>, String> {
    let mut ids = HashSet::from([DEFAULT_SOURCE_ID.to_string()]);
    let mut labels = HashSet::from(["codex 默认".to_string()]);
    let mut paths = HashSet::from([normalize_path(&native.to_string_lossy())?]);
    for source in &mut sources {
        source.id = source.id.trim().to_string();
        source.label = source.label.trim().to_string();
        if source.id.is_empty()
            || source.id.len() > 100
            || !source
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            || !ids.insert(source.id.clone())
        {
            return Err("来源 ID 必须唯一，且由字母、数字、短横线或下划线组成".into());
        }
        if source.label.is_empty()
            || source.label.chars().count() > 40
            || source.label.chars().any(char::is_control)
            || !labels.insert(source.label.to_lowercase())
        {
            return Err("来源标签需为 1～40 个字符，并与其他来源区分".into());
        }
        let path = normalize_path(&source.path)?;
        if !paths.insert(path.clone()) {
            return Err(format!("目录已被其他来源使用：{}", path.display()));
        }
        source.path = path.to_string_lossy().into_owned();
    }
    Ok(sources)
}

fn normalize_path(value: &str) -> Result<PathBuf, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err("请填写 Codex 数据根目录的完整路径".into());
    }
    let path = if value == "~" || value.starts_with("~/") {
        dirs::home_dir()
            .ok_or("无法展开用户目录")?
            .join(value.strip_prefix("~/").unwrap_or(""))
    } else {
        PathBuf::from(value)
    };
    if !path.is_absolute() {
        return Err("采集目录需要使用绝对路径，或以 ~/ 开头".into());
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(std::fs::canonicalize(&normalized).unwrap_or(normalized))
}

fn resolve_sources(
    config: SessionSourcesConfig,
    native: &Path,
) -> Result<Vec<SessionSource>, String> {
    let custom = validate_sources(config.codex, native)?;
    let default = SessionSourceConfig {
        id: DEFAULT_SOURCE_ID.into(),
        label: "Codex 默认".into(),
        path: native.to_string_lossy().into_owned(),
        enabled: true,
    };
    Ok(std::iter::once(default)
        .chain(custom)
        .map(|config| {
            let root = Path::new(&config.path);
            let issue = match std::fs::read_dir(root) {
                Err(error) => Some(format!("目录暂不可用：{}", error)),
                Ok(_)
                    if !root.join("sessions").is_dir()
                        && !root.join("archived_sessions").is_dir() =>
                {
                    Some("尚未发现 sessions 或 archived_sessions，请选择 Codex 数据根目录".into())
                }
                Ok(_) => None,
            };
            SessionSource {
                is_default: config.id == DEFAULT_SOURCE_ID,
                config,
                issue,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn source(id: &str, label: &str, path: &Path) -> SessionSourceConfig {
        SessionSourceConfig {
            id: id.into(),
            label: label.into(),
            path: path.to_string_lossy().into_owned(),
            enabled: true,
        }
    }

    #[test]
    fn missing_config_preserves_the_native_source() {
        let temp = tempdir().unwrap();
        let config = read_config(&temp.path().join("sources.yaml")).unwrap();
        let sources = resolve_sources(config, temp.path()).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].config.id, DEFAULT_SOURCE_ID);
        assert!(sources[0].is_default);
    }

    #[test]
    fn config_round_trip_keeps_identity_when_a_label_changes() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("sources.yaml");
        let native = temp.path().join("native");
        let mut config = vec![source("dev", "开发", &temp.path().join("dev"))];
        persist_config(&path, &native, config.clone()).unwrap();
        config[0].label = "SkillFlow Dev".into();
        persist_config(&path, &native, config).unwrap();
        let sources = resolve_sources(read_config(&path).unwrap(), &native).unwrap();
        assert_eq!(sources[1].config.id, "dev");
        assert_eq!(sources[1].config.label, "SkillFlow Dev");
        assert!(!sources[1].is_default);
        assert!(sources[1].issue.is_some());
    }

    #[test]
    fn validation_rejects_duplicate_paths_labels_and_reserved_ids() {
        let temp = tempdir().unwrap();
        let native = temp.path().join("native");
        let a = source("a", "开发", &temp.path().join("dev"));
        let mut b = source("b", "正式", &temp.path().join("dev/../dev"));
        assert!(validate_sources(vec![a.clone(), b.clone()], &native).is_err());
        b.path = temp.path().join("prod").to_string_lossy().into_owned();
        b.label = " 开发 ".into();
        assert!(validate_sources(vec![a, b.clone()], &native).is_err());
        b.id = DEFAULT_SOURCE_ID.into();
        assert!(validate_sources(vec![b], &native).is_err());
        assert!(validate_sources(vec![source("other", "Other", &native)], &native).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn validation_resolves_directory_aliases() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("root");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        assert!(validate_sources(
            vec![source("a", "A", &root), source("b", "B", &alias)],
            &temp.path().join("native")
        )
        .is_err());
    }

    #[test]
    fn invalid_config_is_reported_and_preserved() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("sources.yaml");
        std::fs::write(&path, "codex: [invalid").unwrap();
        assert!(read_config(&path).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "codex: [invalid");
    }

    #[test]
    fn invalid_edits_keep_the_saved_sources() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("sources.yaml");
        let native = temp.path().join("native");
        let mut sources = vec![source("dev", "开发", &temp.path().join("dev"))];
        persist_config(&path, &native, sources.clone()).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        sources[0].label = " ".into();
        assert!(persist_config(&path, &native, sources).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), saved);
    }

    #[test]
    fn legacy_entries_are_enabled_and_relative_paths_are_reported() {
        let config: SessionSourceConfig = serde_json::from_value(serde_json::json!({
            "id": "dev", "label": "开发", "path": "relative/codex"
        }))
        .unwrap();
        assert!(config.enabled);
        assert!(validate_sources(vec![config], Path::new("/native")).is_err());
    }
}
