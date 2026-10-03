use serde::Deserialize;
use std::{path::PathBuf, time::Duration};

pub const DEFAULT_OUTPUT_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, PartialOrd, Deserialize)]
pub struct Block {
    pub execute: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub interval: Option<f32>,
    pub timeout: Option<f32>,
    pub output_limit: Option<usize>,
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

#[derive(Debug, Clone)]
pub(crate) struct PreparedBlock {
    pub block: Block,
    pub interval: Option<Duration>,
    pub timeout: Option<Duration>,
    pub output_limit: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedConfig {
    pub delimiter: Option<String>,
    pub blocks: Vec<PreparedBlock>,
}

impl Config {
    pub fn load_from_file(path: PathBuf) -> crate::types::Result<Self> {
        log::info!("Loading config from file {}", path.display());
        let config_str = std::fs::read_to_string(&path).map_err(|source| {
            crate::error::AtomBlocksError::ConfigRead {
                path: path.clone(),
                source,
            }
        })?;
        toml::from_str(&config_str)
            .map_err(|source| crate::error::AtomBlocksError::ConfigParse { path, source })
    }

    pub fn validate(&self) -> crate::types::Result<()> {
        self.clone().prepare().map(|_| ())
    }

    pub(crate) fn prepare(self) -> crate::types::Result<PreparedConfig> {
        let mut blocks = Vec::with_capacity(self.block.len());

        for (index, block) in self.block.into_iter().enumerate() {
            if block.execute.trim().is_empty() {
                return Err(crate::error::AtomBlocksError::Config(format!(
                    "block {index}: execute must not be empty"
                )));
            }

            let interval = optional_duration(index, "interval", block.interval, true)?;
            let timeout = optional_duration(index, "timeout", block.timeout, false)?;
            let output_limit = block.output_limit.unwrap_or(DEFAULT_OUTPUT_LIMIT);
            if output_limit == 0 {
                return Err(crate::error::AtomBlocksError::Config(format!(
                    "block {index}: output_limit must be greater than zero"
                )));
            }

            blocks.push(PreparedBlock {
                block,
                interval,
                timeout,
                output_limit,
            });
        }

        Ok(PreparedConfig {
            delimiter: self.delimiter,
            blocks,
        })
    }
}

fn optional_duration(
    block: usize,
    field: &str,
    value: Option<f32>,
    zero_is_none: bool,
) -> crate::types::Result<Option<Duration>> {
    let Some(value) = value else {
        return Ok(None);
    };

    if !value.is_finite() || value < 0.0 {
        return Err(crate::error::AtomBlocksError::Config(format!(
            "block {block}: {field} must be a finite non-negative number"
        )));
    }

    if value == 0.0 {
        if zero_is_none {
            return Ok(None);
        }
        return Err(crate::error::AtomBlocksError::Config(format!(
            "block {block}: {field} must be greater than zero when specified"
        )));
    }

    let seconds = f64::from(value);
    if seconds >= u64::MAX as f64 {
        return Err(crate::error::AtomBlocksError::Config(format!(
            "block {block}: {field} is too large"
        )));
    }

    let duration = Duration::from_secs_f64(seconds);
    if duration.is_zero() {
        return Err(crate::error::AtomBlocksError::Config(format!(
            "block {block}: {field} is too small to represent"
        )));
    }

    Ok(Some(duration))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(interval: Option<f32>) -> Block {
        Block {
            execute: "printf ok".into(),
            before: None,
            after: None,
            interval,
            timeout: None,
            output_limit: None,
        }
    }

    #[test]
    fn zero_and_omitted_intervals_are_manual_only() {
        for interval in [None, Some(0.0)] {
            let config = Config {
                delimiter: None,
                block: vec![block(interval)],
            };
            assert!(config.prepare().unwrap().blocks[0].interval.is_none());
        }
    }

    #[test]
    fn invalid_intervals_return_contextual_errors() {
        for interval in [f32::NAN, f32::INFINITY, -1.0, 1.0e30] {
            let config = Config {
                delimiter: None,
                block: vec![block(Some(interval))],
            };
            let error = config.prepare().unwrap_err().to_string();
            assert!(error.contains("block 0"), "{error}");
            assert!(error.contains("interval"), "{error}");
        }
    }

    #[test]
    fn empty_commands_and_zero_limits_are_rejected() {
        let mut empty = block(None);
        empty.execute = " \t".into();
        let error = Config {
            delimiter: None,
            block: vec![empty],
        }
        .prepare()
        .unwrap_err()
        .to_string();
        assert!(error.contains("execute"));

        let mut limited = block(None);
        limited.output_limit = Some(0);
        let error = Config {
            delimiter: None,
            block: vec![limited],
        }
        .prepare()
        .unwrap_err()
        .to_string();
        assert!(error.contains("output_limit"));
    }
}
