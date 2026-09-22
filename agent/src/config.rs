use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub server_url: String,
    pub agent_token: String,
    pub heartbeat_interval_seconds: u64,
    pub t1_interval_seconds: u64,
    pub t2_interval_seconds: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            server_url: std::env::var("BELARC_SERVER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1".into()),
            agent_token: std::env::var("BELARC_AGENT_TOKEN").unwrap_or_default(),
            heartbeat_interval_seconds: 180,
            t1_interval_seconds: 6 * 3600,
            t2_interval_seconds: 24 * 3600,
        }
    }
}

impl AgentConfig {
    pub fn load() -> Self {
        let path = config_path();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut cfg) = toml::from_str::<AgentConfig>(&content) {
                    if cfg.agent_token.is_empty() {
                        cfg.agent_token = std::env::var("BELARC_AGENT_TOKEN").unwrap_or_default();
                    }
                    if let Ok(url) = std::env::var("BELARC_SERVER_URL") {
                        cfg.server_url = url;
                    }
                    return cfg;
                }
            }
        }

        let cfg = Self::default();
        if cfg.agent_token.is_empty() {
            tracing::warn!("BELARC_AGENT_TOKEN not set; register token via server /api/tokens");
        }
        cfg
    }

    #[allow(dead_code)]
    pub fn save(&self) -> std::io::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self).unwrap_or_default();
        std::fs::write(path, content)
    }
}

pub fn config_path() -> PathBuf {
    let base = std::env::var("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("C:\\ProgramData"));
    base.join("BelarcInventory").join("config.toml")
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("C:\\ProgramData"));
    let dir = base.join("BelarcInventory");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn collectors_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let next_to_exe = dir.join("collectors");
            if next_to_exe.exists() {
                return next_to_exe;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("collectors")
}
