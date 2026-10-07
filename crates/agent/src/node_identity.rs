use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NodeIdentity {
    pub id: String,
}

impl NodeIdentity {
    /// 读取已持久化的身份;没有可用的就新建文件——**连同它的父目录**。
    ///
    /// `create_dir_all` 是 `FIX-05` 的关键:干净的用户账号下 `$HOME/.config` 还不存在,
    /// agent 不能因为一个光秃秃的写失败就退出(现状:`Failed to write node identity` + exit 1)。
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if let Some(id) = read_identity(path)? {
            return Ok(Self { id });
        }

        // `node_id` 这种没有父目录的相对路径也要能用。
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create node identity directory: {}",
                    parent.display()
                )
            })?;
        }

        // Generate new identity
        let new_id = format!("gpu-node-{}", Uuid::new_v4().simple());
        fs::write(path, &new_id)
            .with_context(|| format!("Failed to write node identity to {:?}", path))?;

        Ok(Self { id: new_id })
    }
}

/// 已有的非空身份;文件不存在或内容为空时返回 `None`。
fn read_identity(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let id = fs::read_to_string(path)
        .with_context(|| format!("Failed to read node identity from {:?}", path))?
        .trim()
        .to_string();

    Ok((!id.is_empty()).then_some(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "clusterscope-node-identity-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn creates_the_missing_parent_directory_and_writes_the_identity() {
        let dir = scratch("creates-parent");
        let path = dir.join(".config").join("node_id");
        assert!(!dir.join(".config").exists(), "前提:父目录不存在");

        let identity = NodeIdentity::load_or_create(&path).expect("新建身份");

        assert!(identity.id.starts_with("gpu-node-"), "{}", identity.id);
        let stored = fs::read_to_string(&path).expect("读回身份文件");
        assert_eq!(stored.trim(), identity.id);
    }

    #[test]
    fn reuses_the_persisted_identity() {
        let dir = scratch("reuses");
        let path = dir.join("node_id");
        fs::write(&path, "gpu-node-fixed\n").unwrap();

        let identity = NodeIdentity::load_or_create(&path).expect("读已有身份");

        assert_eq!(identity.id, "gpu-node-fixed");
    }

    #[test]
    fn replaces_an_empty_identity_file() {
        let dir = scratch("empty");
        let path = dir.join("node_id");
        fs::write(&path, "   \n").unwrap();

        let identity = NodeIdentity::load_or_create(&path).expect("替换空身份");

        assert!(identity.id.starts_with("gpu-node-"), "{}", identity.id);
        assert_eq!(fs::read_to_string(&path).unwrap().trim(), identity.id);
    }
}
