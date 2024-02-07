use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, PartialOrd, Deserialize)]
pub struct Block {
    pub execute: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub interval: Option<f32>,
}

impl Block {
    pub fn is_empty(&self) -> bool {
        self.execute.is_empty()
    }
    pub fn print(&self, content: String) -> String {
        format!(
            "{}{}{}",
            self.before.as_deref().unwrap_or_default(),
            content,
            self.after.as_deref().unwrap_or_default(),
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub delimiter: Option<String>,
    pub block: Vec<Block>,
}

impl Config {
    pub fn load_from_file(path: PathBuf) -> crate::types::Result<Self> {
        log::info!("Loading config from file {}", path.display());
        if !path.exists() {
            return Err(crate::error::AtomBlocksError::Config(
                format!("Config file does not exist: {}", path.display()).to_owned(),
            ));
        }
        let config_str = std::fs::read_to_string(path)?;
        let config = toml::from_str(&config_str)?;
        Ok(config)
    }
}
