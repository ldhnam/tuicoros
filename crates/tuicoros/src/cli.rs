use std::path::PathBuf;

use chrono::NaiveDate;
use clap::{Args, Parser, Subcommand};
use coros_api::{FileFormat, Region, SportType};

use crate::format::parse_date;

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

#[derive(Debug, Clone, PartialEq, Eq, Default, Subcommand)]
pub enum Command {
    /// Launch the terminal UI (the default).
    #[default]
    Tui,
    /// Show config, directories and session status.
    Status,
    /// Sign in to COROS. Finds your account's region automatically unless --region is given.
    Login(LoginArgs),
    /// Sign out: remove the stored token and password hash.
    Logout {
        /// Also delete cached activities.
        #[arg(long)]
        purge: bool,
    },
    /// Manage the local cache of activities.
    #[command(subcommand)]
    Cache(CacheCommand),
    /// Call the COROS API directly (debugging, scripting, capturing test fixtures).
    #[command(subcommand)]
    Api(ApiCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum CacheCommand {
    /// Delete everything in the cache (it refills as you browse).
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct LoginArgs {
    /// Account email (or set COROS_EMAIL). Asked for if missing.
    #[arg(long)]
    pub email: Option<String>,
    /// Store the password hash so tuicoros can sign in again by itself when the
    /// token expires. Asked for if neither --remember nor --no-remember is given.
    #[arg(long, conflicts_with = "no_remember")]
    pub remember: bool,
    /// Don't store the password hash.
    #[arg(long)]
    pub no_remember: bool,
}

/// How to print results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Args)]
pub struct OutputArgs {
    /// Print converted data as JSON (metres, seconds, pace in s/km).
    #[arg(long, conflicts_with = "raw")]
    pub json: bool,
    /// Print the API's untouched `data` JSON (for capturing fixtures; scrub before sharing).
    #[arg(long)]
    pub raw: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Args)]
pub struct RangeArgs {
    /// First day (YYYY-MM-DD, today, yesterday, or -N for N days ago).
    #[arg(long, value_parser = parse_date, allow_hyphen_values = true)]
    pub from: Option<NaiveDate>,
    /// Last day (same formats). Defaults to today.
    #[arg(long, value_parser = parse_date, allow_hyphen_values = true)]
    pub to: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum ApiCommand {
    /// Your profile.
    Account {
        #[command(flatten)]
        output: OutputArgs,
    },
    /// List activities (default: the last `activities.default_range_days` days).
    Activities {
        #[command(flatten)]
        range: RangeArgs,
        /// Only these sports (repeatable or comma-separated: run,trail-run,bike).
        #[arg(long = "sport", value_delimiter = ',')]
        sports: Vec<SportType>,
        /// Page number (1-based).
        #[arg(long, default_value_t = 1)]
        page: u32,
        /// Page size, 1..=200 (default: `activities.page_size`).
        #[arg(long)]
        size: Option<u32>,
        /// Follow all pages.
        #[arg(long)]
        all: bool,
        /// Stop after this many activities (with --all).
        #[arg(long)]
        limit: Option<usize>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// One activity's summary and laps.
    Activity {
        /// Activity id (`labelId`, first column of `api activities`).
        id: String,
        /// The activity's sport (key or code). Must match, or COROS returns wrong data.
        #[arg(long, default_value = "run")]
        sport: SportType,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Download an activity file.
    Download {
        id: String,
        #[arg(long, default_value = "run")]
        sport: SportType,
        /// fit, gpx, tcx, kml or csv (default: `export.default_format`).
        #[arg(long)]
        format: Option<FileFormat>,
        /// Output file (default: `<export.directory>/<id>.<ext>`).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// EvoLab summary (training load, fatigue, VO2max). Default: the last 28 days.
    Analyse {
        #[command(flatten)]
        range: RangeArgs,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Daily metrics (RHR, HRV, load) over a longer range. Default: `analysis.default_weeks`.
    Daily {
        #[command(flatten)]
        range: RangeArgs,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Recent overnight HRV.
    Dashboard {
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Training calendar. Default: this week and the next three.
    Schedule {
        #[command(flatten)]
        range: RangeArgs,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Workout library.
    Workouts {
        #[command(flatten)]
        output: OutputArgs,
    },
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("tuicoros").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn no_subcommand_means_tui() {
        let cli = parse(&[]);
        assert_eq!(cli.command.unwrap_or_default(), Command::Tui);
        assert_eq!(cli.region, None);
    }

    #[test]
    fn parses_global_flags_after_subcommand() {
        let cli = parse(&["status", "--region", "cn", "--config", "/x.toml"]);
        assert_eq!(cli.command, Some(Command::Status));
        assert_eq!(cli.region, Some(Region::Asia));
        assert_eq!(cli.config, Some(PathBuf::from("/x.toml")));
    }

    #[test]
    fn rejects_unknown_region() {
        assert!(Cli::try_parse_from(["tuicoros", "--region", "moon"]).is_err());
    }

    #[test]
    fn login_flags() {
        let cli = parse(&["login", "--email", "a@b.c", "--remember"]);
        assert_eq!(
            cli.command,
            Some(Command::Login(LoginArgs {
                email: Some("a@b.c".into()),
                remember: true,
                no_remember: false
            }))
        );
        assert!(Cli::try_parse_from(["tuicoros", "login", "--remember", "--no-remember"]).is_err());
    }

    #[test]
    fn api_activities_flags() {
        let cli = parse(&[
            "api",
            "activities",
            "--from",
            "2026-09-01",
            "--sport",
            "run,trail-run",
            "--sport",
            "bike",
            "--all",
            "--json",
        ]);
        let Some(Command::Api(ApiCommand::Activities {
            range,
            sports,
            all,
            output,
            page,
            ..
        })) = cli.command
        else {
            panic!("wrong command");
        };
        assert_eq!(range.from, NaiveDate::from_ymd_opt(2026, 9, 1));
        assert_eq!(
            sports,
            vec![SportType::Run, SportType::TrailRun, SportType::Bike]
        );
        assert!(all && output.json && !output.raw);
        assert_eq!(page, 1);
        assert!(Cli::try_parse_from(["tuicoros", "api", "account", "--json", "--raw"]).is_err());
    }

    #[test]
    fn relative_dates_with_a_minus_sign() {
        let cli = parse(&["api", "daily", "--from", "-20", "--to", "-1"]);
        let Some(Command::Api(ApiCommand::Daily { range, .. })) = cli.command else {
            panic!()
        };
        assert!(range.from.unwrap() < range.to.unwrap());
    }

    #[test]
    fn api_activity_defaults_to_run() {
        let cli = parse(&["api", "activity", "4711"]);
        let Some(Command::Api(ApiCommand::Activity { id, sport, .. })) = cli.command else {
            panic!()
        };
        assert_eq!((id.as_str(), sport), ("4711", SportType::Run));
        let cli = parse(&[
            "api",
            "download",
            "4711",
            "--sport",
            "102",
            "--format",
            "gpx",
            "-o",
            "/tmp/x.gpx",
        ]);
        let Some(Command::Api(ApiCommand::Download {
            sport,
            format,
            output,
            ..
        })) = cli.command
        else {
            panic!()
        };
        assert_eq!(
            (sport, format, output),
            (
                SportType::TrailRun,
                Some(FileFormat::Gpx),
                Some(PathBuf::from("/tmp/x.gpx"))
            )
        );
    }
}
