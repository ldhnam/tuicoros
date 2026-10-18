use std::path::PathBuf;

use clap::{Parser, Subcommand};
use coros_api::Region;

/// Terminal UI for COROS Training Hub (unofficial, not affiliated with COROS).
#[derive(Debug, Parser)]
#[command(name = "tuicoros", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Use this config file instead of the default location.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// COROS region of your account: us, eu or asia. Overrides COROS_REGION and the config file.
    #[arg(long, global = true, value_name = "REGION")]
    pub region: Option<Region>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Subcommand)]
pub enum Command {
    /// Launch the terminal UI (the default).
    #[default]
    Tui,
    /// Show config, directories and session status.
    Status,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn no_subcommand_means_tui() {
        let cli = Cli::try_parse_from(["tuicoros"]).unwrap();
        assert_eq!(cli.command.unwrap_or_default(), Command::Tui);
        assert_eq!(cli.region, None);
    }

    #[test]
    fn parses_global_flags_after_subcommand() {
        let cli = Cli::try_parse_from([
            "tuicoros", "status", "--region", "cn", "--config", "/x.toml",
        ])
        .unwrap();
        assert_eq!(cli.command, Some(Command::Status));
        assert_eq!(cli.region, Some(Region::Asia));
        assert_eq!(cli.config, Some(PathBuf::from("/x.toml")));
    }

    #[test]
    fn rejects_unknown_region() {
        assert!(Cli::try_parse_from(["tuicoros", "--region", "moon"]).is_err());
    }
}
