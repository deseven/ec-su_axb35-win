//! Configuration module for the tray monitor
//! Loads settings from the same config file as the client

use anyhow::{Context, Result};
use dirs::config_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub server_ip: String,
    pub server_port: u16,
    #[serde(default)]
    pub autostart: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_ip: "127.0.0.1".to_string(),
            server_port: 8395,
            autostart: true,
        }
    }
}

impl Config {
    pub fn server_url(&self) -> String {
        format!("http://{}:{}", self.server_ip, self.server_port)
    }
}

/// Get the path to the tray config file
pub fn get_config_path() -> Result<PathBuf> {
    let config_dir = config_dir().context("Failed to get config directory")?;
    Ok(config_dir.join("ec-su_axb35-win").join("tray.json"))
}

/// Get the path to the client config file (to read server settings)
pub fn get_client_config_path() -> Result<PathBuf> {
    let config_dir = config_dir().context("Failed to get config directory")?;
    Ok(config_dir.join("ec-su_axb35-win").join("client.json"))
}

/// Load configuration, merging client config for server settings
pub fn load_config() -> Result<Config> {
    let tray_config_path = get_config_path()?;
    let client_config_path = get_client_config_path()?;
    
    let mut config = Config::default();
    
    // First try to load client config for server IP/port
    if client_config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&client_config_path) {
            if let Ok(client_config) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(ip) = client_config.get("server_ip").and_then(|v| v.as_str()) {
                    config.server_ip = ip.to_string();
                }
                if let Some(port) = client_config.get("server_port").and_then(|v| v.as_u64()) {
                    config.server_port = port as u16;
                }
            }
        }
    }
    
    // Then load tray-specific config (autostart preference)
    if tray_config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&tray_config_path) {
            if let Ok(tray_config) = serde_json::from_str::<Config>(&content) {
                config.autostart = tray_config.autostart;
            }
        }
    }
    
    Ok(config)
}

/// Save tray-specific configuration
pub fn save_config(config: &Config) -> Result<()> {
    let config_path = get_config_path()?;
    
    // Ensure directory exists
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    
    let content = serde_json::to_string_pretty(config)?;
    std::fs::write(&config_path, content)?;
    
    Ok(())
}

#[cfg(windows)]
pub mod autostart {
    use anyhow::Result;
    use winreg::enums::*;
    use winreg::RegKey;

    const APP_NAME: &str = "ec-su_axb35-win-tray";
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    /// Check if autostart is enabled in registry
    pub fn is_enabled() -> bool {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(run_key) = hkcu.open_subkey(RUN_KEY) {
            run_key.get_value::<String, _>(APP_NAME).is_ok()
        } else {
            false
        }
    }

    /// Enable or disable autostart
    pub fn set_enabled(enable: bool) -> Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run_key = hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE | KEY_QUERY_VALUE)?;
        
        if enable {
            // Get current executable path
            let exe_path = std::env::current_exe()?;
            let exe_str = exe_path.to_string_lossy().to_string();
            run_key.set_value(APP_NAME, &exe_str)?;
        } else {
            let _ = run_key.delete_value(APP_NAME); // Ignore error if not exists
        }
        
        Ok(())
    }
}
