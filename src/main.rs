use atomblocks::{
    cli::{AtomBlocksCli, CliActions},
    config::Config,
    error::AtomBlocksError,
    types::Result,
    AtomBlocks, OutputMode,
};
use simple_logger::SimpleLogger;
use std::{ffi::OsString, io::ErrorKind, path::PathBuf, process::ExitCode};

const CONFIG_FILE: &str = "config.toml";

fn main() -> ExitCode {
    let logger = SimpleLogger::new();
    let cli: AtomBlocksCli = argh::from_env();

    let logger = if cli.trace() {
        logger.with_level(log::LevelFilter::Trace)
    } else if cli.verbose() {
        logger.with_level(log::LevelFilter::Info)
    } else {
        logger.with_level(log::LevelFilter::Error)
    };
    logger.with_colors(true).init().unwrap();
    log::debug!("logger initialized");

    if cli.version() {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    let result = match cli.action() {
        Some(CliActions::Run(params)) => {
            let config_file = match params.config() {
                Some(path) => Ok(path),
                None => get_config_path(),
            };
            let output = if params.stdout() {
                OutputMode::Stdout
            } else {
                OutputMode::X11
            };
            log::info!("Starting AtomBlocks");
            config_file.and_then(|path| run(path, output))
        }
        Some(CliActions::Hit(params)) => hit(params.id()),
        None => {
            println!("Missing command");
            Ok(())
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(AtomBlocksError::IOError(error)) if error.kind() == ErrorKind::BrokenPipe => {
            ExitCode::SUCCESS
        }
        Err(AtomBlocksError::Interrupted(signal)) => {
            ExitCode::from((128_i32.saturating_add(signal)).clamp(1, 255) as u8)
        }
        Err(error) => {
            log::error!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(config: PathBuf, output: OutputMode) -> atomblocks::types::Result<()> {
    log::debug!("Starting AtomBlocks");
    Config::load_from_file(config).and_then(|config| {
        AtomBlocks::new_with_output(config, output).and_then(|mut bar| bar.run())
    })
}

fn hit(id: u32) -> atomblocks::types::Result<()> {
    let hitman = atomblocks::HitMan::new()?;
    hitman.hit_block(id)
}

fn get_config_path() -> Result<PathBuf> {
    let candidates = config_candidates(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    );
    for path in &candidates {
        match std::fs::metadata(path) {
            Ok(_) => return Ok(path.clone()),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Ok(path.clone()),
        }
    }

    Err(AtomBlocksError::Config(format!(
        "config file not found; tried {}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

fn config_candidates(xdg_config_home: Option<OsString>, home: Option<OsString>) -> Vec<PathBuf> {
    let mut candidates = Vec::with_capacity(3);

    if let Some(value) = xdg_config_home.filter(|value| !value.is_empty()) {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            candidates.push(path.join("atomblocks").join(CONFIG_FILE));
        } else {
            log::warn!("ignoring relative XDG_CONFIG_HOME: {}", path.display());
        }
    }

    if let Some(value) = home.filter(|value| !value.is_empty()) {
        candidates.push(
            PathBuf::from(value)
                .join(".config")
                .join("atomblocks")
                .join(CONFIG_FILE),
        );
    }
    candidates.push(PathBuf::from("/etc/atomblocks").join(CONFIG_FILE));
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_discovery_keeps_precedence_and_fallbacks() {
        let candidates = config_candidates(
            Some(OsString::from("/xdg")),
            Some(OsString::from("/home/test")),
        );
        assert_eq!(
            candidates,
            vec![
                PathBuf::from("/xdg/atomblocks/config.toml"),
                PathBuf::from("/home/test/.config/atomblocks/config.toml"),
                PathBuf::from("/etc/atomblocks/config.toml"),
            ]
        );
    }

    #[test]
    fn empty_or_relative_xdg_paths_are_ignored() {
        assert_eq!(
            config_candidates(Some(OsString::new()), None),
            vec![PathBuf::from("/etc/atomblocks/config.toml")]
        );
        assert_eq!(
            config_candidates(Some(OsString::from("relative")), None),
            vec![PathBuf::from("/etc/atomblocks/config.toml")]
        );
    }
}
