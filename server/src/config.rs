use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FanConfig {
    pub mode: String,
    pub level: u8,
    pub rampup_curve: [u8; 5],
    pub rampdown_curve: [u8; 5],
}

impl Default for FanConfig {
    fn default() -> Self {
        FanConfig {
            mode: "auto".to_string(),
            level: 0,
            rampup_curve: [60, 70, 83, 95, 97],
            rampdown_curve: [40, 50, 80, 94, 96],
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub log_path: String,
    pub driver_path: String,
    pub apu_power_mode: Option<String>,
    pub fan1: Option<FanConfig>,
    pub fan2: Option<FanConfig>,
    pub fan3: Option<FanConfig>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        let system_drive = std::env::var("SYSTEMDRIVE").unwrap_or_else(|_| "C:".to_string());
        
        // Fan3 has different default curves from Linux driver
        let mut fan3_config = FanConfig::default();
        fan3_config.rampup_curve = [20, 60, 83, 95, 97];
        fan3_config.rampdown_curve = [0, 50, 80, 94, 96];
        
        ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 8395,
            log_path: format!("{}\\ProgramData\\ec-su_axb35-win\\server.log", system_drive),
            driver_path: format!("{}\\ProgramData\\ec-su_axb35-win\\winring0", system_drive),
            apu_power_mode: None,
            fan1: Some(FanConfig::default()),
            fan2: Some(FanConfig::default()),
            fan3: Some(fan3_config),
        }
    }
}

impl ServerConfig {
    pub fn load() -> Result<Self, String> {
        let system_drive = std::env::var("SYSTEMDRIVE").unwrap_or_else(|_| "C:".to_string());
        let config_path = format!("{}\\ProgramData\\ec-su_axb35-win\\config.json", system_drive);
        
        if !Path::new(&config_path).exists() {
            // Create default config if it doesn't exist
            let default_config = ServerConfig::default();
            
            // Create directory if it doesn't exist
            let config_dir = Path::new(&config_path).parent().unwrap();
            if !config_dir.exists() {
                fs::create_dir_all(config_dir)
                    .map_err(|e| format!("Failed to create config directory: {}", e))?;
            }
            
            // Write default config
            let config_json = serde_json::to_string_pretty(&default_config)
                .map_err(|e| format!("Failed to serialize default config: {}", e))?;
            
            fs::write(&config_path, config_json)
                .map_err(|e| format!("Failed to write default config: {}", e))?;
            
            return Ok(default_config);
        }
        
        let config_content = fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config file {}: {}", config_path, e))?;
        
        let mut config: ServerConfig = serde_json::from_str(&config_content)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;
        
        // Ensure paths are absolute
        if !config.log_path.contains(':') {
            config.log_path = format!("{}\\ProgramData\\ec-su_axb35-win\\{}", system_drive, config.log_path);
        }
        
        if !config.driver_path.contains(':') {
            config.driver_path = format!("{}\\ProgramData\\ec-su_axb35-win\\{}", system_drive, config.driver_path);
        }
        
        Ok(config)
    }
    
    pub fn save(&self) -> Result<(), String> {
        let system_drive = std::env::var("SYSTEMDRIVE").unwrap_or_else(|_| "C:".to_string());
        let config_path = format!("{}\\ProgramData\\ec-su_axb35-win\\config.json", system_drive);
        
        // Create directory if it doesn't exist
        let config_dir = Path::new(&config_path).parent().unwrap();
        if !config_dir.exists() {
            fs::create_dir_all(config_dir)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }
        
        // Serialize and write config
        let config_json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        
        fs::write(&config_path, config_json)
            .map_err(|e| format!("Failed to write config file: {}", e))?;
        
        Ok(())
    }
}