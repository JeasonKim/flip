use std::fs;
use std::path::Path;

/// 确保路径的父目录存在，不存在则递归创建
pub fn ensure_parent_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

/// 原子写入文件：先写临时文件，再 rename 到目标路径
/// Unix 上 rename 是原子操作；Windows 上先删目标再 rename
pub fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    ensure_parent_dir(path)?;

    let tmp_path = path.with_extension(format!(
        "tmp.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    fs::write(&tmp_path, data)?;

    #[cfg(windows)]
    {
        if path.exists() {
            if let Err(e) = fs::remove_file(path) {
                let _ = fs::remove_file(&tmp_path);
                return Err(e);
            }
        }
    }

    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    Ok(())
}

/// 原子写入 JSON：序列化后调用 atomic_write
pub fn write_json_file(path: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    let data = serde_json::to_string_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    atomic_write(path, data.as_bytes())
}

/// 原子写入文本文件
pub fn write_text_file(path: &Path, text: &str) -> std::io::Result<()> {
    atomic_write(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flip_test_{}_{}", std::process::id(), name,));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn atomic_write_creates_file() {
        let dir = tmp_dir("create");
        let path = dir.join("test.txt");
        atomic_write(&path, b"hello").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = tmp_dir("overwrite");
        let path = dir.join("test.txt");
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_creates_parent_dirs() {
        let dir = tmp_dir("nested");
        let path = dir.join("a").join("b").join("c.txt");
        atomic_write(&path, b"nested").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "nested");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_json_file_roundtrip() {
        let dir = tmp_dir("json");
        let path = dir.join("data.json");
        let value = serde_json::json!({"key": "value", "num": 42});
        write_json_file(&path, &value).unwrap();
        let read: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(read, value);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_tmp_file_left_after_write() {
        let dir = tmp_dir("notmp");
        let path = dir.join("clean.txt");
        atomic_write(&path, b"data").unwrap();
        let entries: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
