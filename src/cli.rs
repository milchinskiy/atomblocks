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

    /// write bar updates to stdout instead of the X11 root window
    #[argh(switch, long = "stdout")]
    stdout: bool,
}
impl CliActionRun {
    pub fn stdout(&self) -> bool {
        self.stdout
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_defaults_to_x11() {
        let cli = AtomBlocksCli::from_args(&["atomblocks"], &["run"]).unwrap();
        let Some(CliActions::Run(run)) = cli.action() else {
            panic!("expected run command");
        };
        assert!(!run.stdout());
    }

    #[test]
    fn stdout_accepts_custom_config_and_verbose_logging() {
        let cli = AtomBlocksCli::from_args(
            &["atomblocks"],
            &["--verbose", "run", "--stdout", "--config", "bar.toml"],
        )
        .unwrap();
        let Some(CliActions::Run(run)) = cli.action() else {
            panic!("expected run command");
        };
        assert!(run.stdout());
        assert_eq!(run.config(), Some(PathBuf::from("bar.toml")));
        assert!(cli.verbose());
    }

    #[test]
    fn stdout_is_only_a_run_option() {
        assert!(AtomBlocksCli::from_args(&["atomblocks"], &["hit", "0", "--stdout"]).is_err());
        assert!(AtomBlocksCli::from_args(&["atomblocks"], &["--stdout"]).is_err());
    }
}
