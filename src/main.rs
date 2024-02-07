use atomblocks::{
    cli::{AtomBlocksCli, CliActions},
    config::Config,
    error::AtomBlocksError,
    types::Result,
    AtomBlocks,
};
use simple_logger::SimpleLogger;
use std::path::PathBuf;

const CONFIG_FILE: &str = "config.toml";

fn main() {
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

    let result = match cli.action() {
        CliActions::Run(params) => {
            let config_file = if let Some(path) = params.config() {
                Ok(path)
            } else {
                get_config_path()
            };
            log::info!("Starting AtomBlocks");
            if let Ok(config_file) = config_file {
                run(config_file)
            } else {
                Err(AtomBlocksError::Config("Failed to load config".into()))
            }
        }
        CliActions::Hit(params) => hit(params.id()),
    };

    match result {
        Ok(_) => (),
        Err(err) => {
            log::error!("{}", err);
            eprintln!("{}", err);
        }
    }
}

fn run(config: PathBuf) -> atomblocks::types::Result<()> {
    log::debug!("Starting AtomBlocks");
    Config::load_from_file(config)
        .and_then(|config| AtomBlocks::new(config).and_then(|mut a| a.run()))
}

fn hit(id: u32) -> atomblocks::types::Result<()> {
    let hitman = atomblocks::HitMan::new()?;
    hitman.hit_block(id)
}

fn get_config_path() -> Result<PathBuf> {
    let mut path: PathBuf;
    if let Some(home_config_dir) = std::env::var_os("XDG_CONFIG_HOME") {
        path = PathBuf::from(home_config_dir);
        path.push("atomblocks");
    } else if let Some(home_dir) = std::env::var_os("HOME") {
        path = PathBuf::from(home_dir);
        path.push(".config/atomblocks");
    } else {
        path = PathBuf::from("/etc/atomblocks");
    }
    path.push(CONFIG_FILE);

    if !path.exists() {
        return Err(AtomBlocksError::Config(
            "Config file not specified or not found".to_owned(),
        ));
    }
    Ok(path)
}
