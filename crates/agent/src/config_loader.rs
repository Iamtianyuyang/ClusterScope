use anyhow::{Context, Result, bail};
use common::config::AgentConfig;
use std::fs;
use std::path::Path;
use tracing::warn;

/// 不带 `-c` 时的默认配置路径。
///
/// 这个文件缺失**不是**硬错误:`deploy/install-agent.sh` 的用户级安装从不写它,
/// agent 必须照常启动(`FIX-04`)——只是回退不再静默,要明确告警点名它。
pub const DEFAULT_CONFIG_PATH: &str = "/etc/clusterscope/agent.yaml";

/// 用户级安装的配置位置。写在"显式配置缺失"的错误里,让照抄系统级 unit
/// (`--config /etc/clusterscope/agent.yaml`)的人一眼看到自己该用的路径(`FIX-02`)。
pub const USER_LEVEL_CONFIG_HINT: &str = "$HOME/.config/clusterscope/agent.yaml";

/// 加载 agent 配置:先取配置文件,再套 CLI 覆盖项,最后保证日志目录存在。
pub fn load_config(cli: &crate::Cli) -> Result<AgentConfig> {
    let mut config = load_config_file(cli.config.as_deref(), Path::new(DEFAULT_CONFIG_PATH))?;

    apply_cli_overrides(&mut config, cli);

    fs::create_dir_all(&config.log_dir)
        .with_context(|| format!("Failed to create log directory: {:?}", config.log_dir))?;

    Ok(config)
}

/// 配置文件的查找语义(`FIX-02`…`FIX-04`),显式与默认严格分开:
///
/// * `explicit` = 命令行给了 `-c/--config`:文件**必须**存在;缺失即硬错误,
///   绝不静默改用内置默认值;
/// * `explicit == None`:试 `default_path`;它也缺失时告警点名路径,再按内置默认值继续。
fn load_config_file(explicit: Option<&Path>, default_path: &Path) -> Result<AgentConfig> {
    match explicit {
        Some(path) => required_config_file(path),
        None if default_path.exists() => read_config_file(default_path),
        None => {
            warn!("{}", default_config_missing_message(default_path));
            Ok(AgentConfig::default())
        }
    }
}

/// 显式请求的配置文件必须在那儿。
fn required_config_file(path: &Path) -> Result<AgentConfig> {
    if !path.exists() {
        bail!("{}", explicit_config_missing_message(path));
    }
    read_config_file(path)
}

fn read_config_file(path: &Path) -> Result<AgentConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {:?}", path))?;

    serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {:?}", path))
}

fn apply_cli_overrides(config: &mut AgentConfig, cli: &crate::Cli) {
    if let Some(addr) = &cli.server_addr {
        config.server_addr = addr.clone();
    }

    if let Some(node_id) = &cli.node_id {
        config.node_id = Some(node_id.clone());
    }

    if let Some(token) = &cli.agent_token {
        config.agent_token = token.clone();
    }

    // Override node_id_file if specified via config_dir
    if let Some(config_dir) = &cli.config_dir {
        config.node_id_file = config_dir.join("node_id");
        config.log_dir = config_dir.join("logs");
    }
}

/// `-c <不存在的文件>` 的错误文案:点名该文件、说明它不存在、给出用户级路径。
fn explicit_config_missing_message(path: &Path) -> String {
    format!(
        "config file not found: {} does not exist. It was requested explicitly with \
         -c/--config, so the agent stops here instead of quietly using built-in defaults. \
         User-level installs keep this file at {}; system-wide installs use {}.",
        path.display(),
        USER_LEVEL_CONFIG_HINT,
        DEFAULT_CONFIG_PATH
    )
}

/// 默认配置缺失的告警文案:点名默认路径 + 说明改用内置默认值。
fn default_config_missing_message(path: &Path) -> String {
    format!(
        "default config file not found: {} does not exist, continuing with built-in defaults \
         (user-level installs keep this file at {}).",
        path.display(),
        USER_LEVEL_CONFIG_HINT
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "clusterscope-agent-config-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn cli(config: Option<PathBuf>, config_dir: Option<PathBuf>) -> crate::Cli {
        crate::Cli {
            config,
            config_dir,
            server_addr: None,
            node_id: None,
            agent_token: None,
        }
    }

    #[test]
    fn explicit_missing_config_is_a_hard_error_that_names_the_file_and_the_user_path() {
        let dir = scratch("explicit-missing");
        let path = dir.join("absent-dir").join("agent.yaml");

        let err = load_config_file(Some(&path), &dir.join("default.yaml"))
            .expect_err("显式路径缺失必须报错");

        let message = err.to_string();
        assert!(message.contains(path.to_str().unwrap()), "{message}");
        assert!(
            message.contains(".config/clusterscope/agent.yaml"),
            "{message}"
        );
        assert!(message.to_lowercase().contains("not found"), "{message}");
    }

    #[test]
    fn missing_default_config_falls_back_to_built_in_defaults() {
        let dir = scratch("default-missing");
        let default_path = dir.join("etc").join("agent.yaml");

        let config = load_config_file(None, &default_path).expect("默认路径缺失不得是硬错误");

        assert_eq!(
            config.server_addr,
            AgentConfig::default().server_addr,
            "应落到内置默认值"
        );
    }

    #[test]
    fn existing_default_config_is_read() {
        let dir = scratch("default-present");
        let default_path = dir.join("agent.yaml");
        fs::write(&default_path, "server_addr: \"http://127.0.0.1:60001\"\n").unwrap();

        let config = load_config_file(None, &default_path).expect("读默认配置");

        assert_eq!(config.server_addr, "http://127.0.0.1:60001");
    }

    #[test]
    fn default_config_missing_message_names_the_default_path_and_the_fallback() {
        let message = default_config_missing_message(Path::new(DEFAULT_CONFIG_PATH));

        assert!(message.contains(DEFAULT_CONFIG_PATH), "{message}");
        assert!(message.to_lowercase().contains("not found"), "{message}");
        assert!(message.contains("built-in defaults"), "{message}");
    }

    #[test]
    fn explicit_config_is_read_and_cli_overrides_win() {
        let dir = scratch("overrides");
        let path = dir.join("agent.yaml");
        fs::write(
            &path,
            "server_addr: \"http://127.0.0.1:59999\"\nnode_id: \"from-file\"\n",
        )
        .unwrap();
        let state_dir = dir.join("state");
        let mut cli = cli(Some(path), Some(state_dir.clone()));
        cli.server_addr = Some("http://127.0.0.1:1234".to_string());

        let config = load_config(&cli).expect("load");

        assert_eq!(config.server_addr, "http://127.0.0.1:1234");
        assert_eq!(config.node_id.as_deref(), Some("from-file"));
        assert_eq!(config.node_id_file, state_dir.join("node_id"));
        assert!(state_dir.join("logs").is_dir(), "日志目录必须被创建");
    }

    #[test]
    fn explicit_config_that_is_not_a_file_is_rejected() {
        let dir = scratch("explicit-dir");
        let path = dir.join("not-a-file.yaml");

        let err = load_config_file(Some(&path), &dir.join("default.yaml"))
            .expect_err("指向不存在文件的显式路径必须报错");

        assert!(err.to_string().contains("not found"), "{err}");
    }
}
