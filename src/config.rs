use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const CONFIG_FILE: &str = "lightcord_config.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub token: String,
    pub save_token: bool,
    pub auto_login: bool,
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        PathBuf::from(CONFIG_FILE)
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(cfg) = serde_json::from_str::<AppConfig>(&content) {
                    return cfg;
                }
            }
        }
        AppConfig::default()
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::config_path();
        if self.save_token {
            let data = serde_json::to_string_pretty(self)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            fs::write(path, data)?;
        } else if path.exists() {
            let _ = fs::remove_file(path);
        }
        Ok(())
    }
}
