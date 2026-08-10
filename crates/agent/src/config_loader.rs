use anyhow::{Context, Result};
use common::config::AgentConfig;
use std::fs;

pub fn load_config(cli: &crate::Cli) -> Result<AgentConfig> {
    let config_path = &cli.config;
    let mut config = AgentConfig::default();
    
    // Try to load from file
    if config_path.exists() {
        let content = fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read config file: {:?}", config_path))?;
        
        let parsed: AgentConfig = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {:?}", config_path))?;
        
        config = parsed;
    }
    
    // CLI overrides
    if let Some(addr) = &cli.server_addr {
        config.server_addr = addr.clone();
    }
    
    if let Some(node_id) = &cli.node_id {
        config.node_id = Some(node_id.clone());
    }
    
    // Override node_id_file if specified via config_dir
    if let Some(config_dir) = &cli.config_dir {
        config.node_id_file = config_dir.join("node_id");
        config.log_dir = config_dir.join("logs");
    }
    
    // Ensure log directory exists
    fs::create_dir_all(&config.log_dir)
        .with_context(|| format!("Failed to create log directory: {:?}", config.log_dir))?;
    
    Ok(config)
}
