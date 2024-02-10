use argh::{self, FromArgs};
use std::path::PathBuf;

#[derive(FromArgs, PartialEq, Debug)]
/// asynchronous, absolutely lightweight
/// and dead simple bar for dwm and similar window managers
pub struct AtomBlocksCli {
    #[argh(subcommand)]
    action: Option<CliActions>,

    /// set log level to INFO
    #[argh(switch, short = 'v', long = "verbose")]
    verbose: bool,

    /// set log level to TRACE (a lot of records, be careful)
    #[argh(switch, long = "trace")]
    trace: bool,

    /// version
    #[argh(switch, long = "version")]
    version: bool,
}

impl AtomBlocksCli {
    pub fn verbose(&self) -> bool {
        self.verbose
    }
    pub fn trace(&self) -> bool {
        self.trace
    }
    pub fn action(&self) -> Option<&CliActions> {
        self.action.as_ref()
    }
    pub fn version(&self) -> bool {
        self.version
    }
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand)]
pub enum CliActions {
    Run(CliActionRun),
    Hit(CliActionHit),
}

#[derive(FromArgs, PartialEq, Debug)]
/// Run the bar
#[argh(subcommand, name = "run")]
pub struct CliActionRun {
    /// configuration file
    #[argh(option, short = 'c', long = "config")]
    config: Option<PathBuf>,
}
impl CliActionRun {
    pub fn config(&self) -> Option<PathBuf> {
        self.config.clone()
    }
}

#[derive(FromArgs, PartialEq, Debug)]
/// Asynchronously update the block specified in the ID
#[argh(subcommand, name = "hit")]
pub struct CliActionHit {
    /// block id
    #[argh(positional)]
    id: u32,
}
impl CliActionHit {
    pub fn id(&self) -> u32 {
        self.id
    }
}
