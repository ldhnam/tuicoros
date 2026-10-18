//! `config.toml` loading. See `docs/SECURITY_AND_CONFIG.md` §5.
//!
//! Precedence (highest wins): CLI flags → env vars → config file → defaults.
//! CLI flags are applied by `main`; this module handles the rest.

use std::path::{Path, PathBuf};

use color_eyre::Result;
use color_eyre::eyre::{WrapErr, bail};
use coros_api::Region;
use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

use crate::storage::secrets::SecretsBackend;

pub const DEFAULT_USER_AGENT: &str = coros_api::DEFAULT_USER_AGENT;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub region: Region,
    pub display: DisplayConfig,
    pub activities: ActivitiesConfig,
    pub analysis: AnalysisConfig,
    pub network: NetworkConfig,
    pub export: ExportConfig,
    pub secrets: SecretsConfig,
    pub log: LogConfig,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SecretsConfig {
    /// `auto` (keychain, falling back to a private file), `keychain` or `file`.
    pub backend: SecretsBackend,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    pub units: Units,
    pub theme: ThemeName,
    pub icons: Icons,
    pub week_starts_on: WeekStart,
    pub time_zone: TimeZoneMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    #[default]
    Metric,
    Imperial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeName {
    #[default]
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Icons {
    #[default]
    Text,
    Nerdfont,
    Emoji,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WeekStart {
    #[default]
    Monday,
    Sunday,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeZoneMode {
    /// Show times in the UTC offset recorded with each activity.
    #[default]
    Activity,
    /// Show times in this computer's time zone.
    Local,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActivitiesConfig {
    pub page_size: u16,
    pub default_range_days: u32,
}

impl Default for ActivitiesConfig {
    fn default() -> Self {
        Self {
            page_size: 50,
            default_range_days: 90,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisConfig {
    pub default_weeks: u8,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self { default_weeks: 12 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub max_concurrent_requests: u8,
    pub timeout_secs: u64,
    pub user_agent: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: 2,
            timeout_secs: 20,
            user_agent: DEFAULT_USER_AGENT.to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    #[default]
    Fit,
    Gpx,
    Tcx,
    Kml,
    Csv,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportConfig {
    /// May start with `~/`; expanded when used.
    pub directory: PathBuf,
    pub default_format: ExportFormat,
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            directory: PathBuf::from("~/Downloads"),
            default_format: ExportFormat::Fit,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    /// A `tracing` filter such as `info` or `tuicoros=debug,info`.
    pub level: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".to_owned(),
        }
    }
}

/// The merged configuration plus where it came from.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: Config,
    pub path: PathBuf,
    pub file_found: bool,
    /// Non-fatal problems (e.g. unknown keys), shown to the user and logged.
    pub warnings: Vec<String>,
    /// The region came from `--region` or `COROS_REGION`, not the file/default.
    /// `tuicoros login` then tries only that region.
    pub region_explicit: bool,
}

/// Load `path` (a missing file means defaults), apply env overrides, validate.
pub fn load(path: &Path, env: &impl Fn(&str) -> Option<String>) -> Result<LoadedConfig> {
    let mut warnings = Vec::new();
    let (mut config, file_found) = match std::fs::read_to_string(path) {
        Ok(text) => {
            let config = parse(&text, &mut warnings)
                .wrap_err_with(|| format!("invalid config file {}", path.display()))?;
            (config, true)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (Config::default(), false),
        Err(err) => {
            return Err(err)
                .wrap_err_with(|| format!("cannot read config file {}", path.display()));
        }
    };
    let region_explicit = config.apply_env(env)?;
    config.validate()?;
    Ok(LoadedConfig {
        config,
        path: path.to_path_buf(),
        file_found,
        warnings,
        region_explicit,
    })
}

/// Parse TOML text. Unknown keys are reported in `warnings` instead of failing.
pub fn parse(text: &str, warnings: &mut Vec<String>) -> Result<Config> {
    // Strict pass first: gives good error messages with line numbers.
    let config: Config = toml::from_str(text)?;
    // Second pass only to find keys serde ignored.
    let table: toml::Table = toml::from_str(text)?;
    let _: std::result::Result<Config, _> =
        serde_ignored::deserialize(toml::Value::Table(table), |path| {
            warnings.push(format!("unknown key `{path}` (ignored)"));
        });
    Ok(config)
}

impl Config {
    /// Apply `COROS_REGION`, `TUICOROS_LOG` and `TUICOROS_SECRETS`.
    /// Returns whether the region was set by the environment.
    pub fn apply_env(&mut self, env: &impl Fn(&str) -> Option<String>) -> Result<bool> {
        let mut region_set = false;
        if let Some(region) = env("COROS_REGION").filter(|v| !v.is_empty()) {
            self.region = region.parse().wrap_err("COROS_REGION")?;
            region_set = true;
        }
        if let Some(level) = env("TUICOROS_LOG").filter(|v| !v.is_empty()) {
            self.log.level = level;
        }
        if let Some(backend) = env("TUICOROS_SECRETS").filter(|v| !v.is_empty()) {
            self.secrets.backend = backend
                .parse()
                .map_err(color_eyre::eyre::Report::msg)
                .wrap_err("TUICOROS_SECRETS")?;
        }
        Ok(region_set)
    }

    /// Reject values that would break the app later. Reports every problem at once.
    pub fn validate(&self) -> Result<()> {
        let mut problems = Vec::new();
        if !(1..=200).contains(&self.activities.page_size) {
            problems.push(format!(
                "activities.page_size must be 1..=200 (the API maximum), got {}",
                self.activities.page_size
            ));
        }
        if self.activities.default_range_days == 0 {
            problems.push("activities.default_range_days must be at least 1".to_owned());
        }
        if ![4, 12, 24].contains(&self.analysis.default_weeks) {
            problems.push(format!(
                "analysis.default_weeks must be 4, 12 or 24, got {}",
                self.analysis.default_weeks
            ));
        }
        if !(1..=4).contains(&self.network.max_concurrent_requests) {
            problems.push(format!(
                "network.max_concurrent_requests must be 1..=4 (COROS rate-limits hard), got {}",
                self.network.max_concurrent_requests
            ));
        }
        if !(1..=300).contains(&self.network.timeout_secs) {
            problems.push(format!(
                "network.timeout_secs must be 1..=300, got {}",
                self.network.timeout_secs
            ));
        }
        if self.network.user_agent.trim().is_empty() {
            problems.push("network.user_agent must not be empty".to_owned());
        }
        if let Err(err) = EnvFilter::try_new(&self.log.level) {
            problems.push(format!(
                "log.level `{}` is not a valid filter: {err}",
                self.log.level
            ));
        }
        if problems.is_empty() {
            Ok(())
        } else {
            bail!("invalid configuration:\n  - {}", problems.join("\n  - "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    /// The example from docs/SECURITY_AND_CONFIG.md must stay valid.
    const DOC_EXAMPLE: &str = r#"
region = "eu"

[display]
units = "metric"
theme = "dark"
icons = "text"
week_starts_on = "monday"
time_zone = "activity"

[activities]
page_size = 50
default_range_days = 90

[analysis]
default_weeks = 12

[network]
max_concurrent_requests = 2
timeout_secs = 20
user_agent = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36"

[export]
directory = "~/Downloads"
default_format = "fit"

[secrets]
backend = "auto"

[log]
level = "info"
"#;

    #[test]
    fn empty_file_gives_defaults() {
        let mut warnings = Vec::new();
        assert_eq!(parse("", &mut warnings).unwrap(), Config::default());
        assert!(warnings.is_empty());
        assert!(Config::default().validate().is_ok());
    }

    #[test]
    fn doc_example_parses_without_warnings() {
        let mut warnings = Vec::new();
        let config = parse(DOC_EXAMPLE, &mut warnings).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(config.region, Region::Eu);
        assert!(config.validate().is_ok());
        // Apart from the region, the example is exactly the defaults.
        assert_eq!(
            Config {
                region: Region::Us,
                ..config
            },
            Config::default()
        );
    }

    #[test]
    fn partial_file_keeps_other_defaults() {
        let mut warnings = Vec::new();
        let config = parse(
            "region = \"asia\"\n[display]\nunits = \"imperial\"\n",
            &mut warnings,
        )
        .unwrap();
        assert_eq!(config.region, Region::Asia);
        assert_eq!(config.display.units, Units::Imperial);
        assert_eq!(config.display.theme, ThemeName::Dark);
        assert_eq!(config.activities.page_size, 50);
    }

    #[test]
    fn unknown_keys_are_warnings_not_errors() {
        let mut warnings = Vec::new();
        let config = parse("colour = 1\n[display]\nthemes = \"x\"\n", &mut warnings).unwrap();
        assert_eq!(config, Config::default());
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("`colour`")));
        assert!(warnings.iter().any(|w| w.contains("`display.themes`")));
    }

    #[test]
    fn bad_values_are_errors() {
        let mut warnings = Vec::new();
        assert!(parse("region = \"moon\"", &mut warnings).is_err());
        assert!(parse("[display]\nunits = \"furlongs\"", &mut warnings).is_err());
        assert!(parse("[activities]\npage_size = \"lots\"", &mut warnings).is_err());
        assert!(parse("not toml at all [", &mut warnings).is_err());
    }

    #[test]
    fn validation_reports_every_problem() {
        let mut config = Config::default();
        config.activities.page_size = 500;
        config.analysis.default_weeks = 7;
        config.network.max_concurrent_requests = 0;
        let message = format!("{:#}", config.validate().unwrap_err());
        assert!(message.contains("page_size"), "{message}");
        assert!(message.contains("default_weeks"), "{message}");
        assert!(message.contains("max_concurrent_requests"), "{message}");
    }

    #[test]
    fn env_overrides_file() {
        let env = |key: &str| match key {
            "COROS_REGION" => Some("cn".to_owned()),
            "TUICOROS_LOG" => Some("debug".to_owned()),
            "TUICOROS_SECRETS" => Some("file".to_owned()),
            _ => None,
        };
        let mut config = Config {
            region: Region::Eu,
            ..Config::default()
        };
        assert!(config.apply_env(&env).unwrap(), "region came from env");
        assert_eq!(config.region, Region::Asia);
        assert_eq!(config.log.level, "debug");
        assert_eq!(config.secrets.backend, SecretsBackend::File);
        assert!(!Config::default().apply_env(&|_: &str| None).unwrap());
    }

    #[test]
    fn invalid_env_region_is_an_error() {
        let env = |key: &str| (key == "COROS_REGION").then(|| "moon".to_owned());
        assert!(Config::default().apply_env(&env).is_err());
    }

    #[test]
    fn missing_file_means_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let loaded = load(&tmp.path().join("nope.toml"), &no_env).unwrap();
        assert!(!loaded.file_found);
        assert_eq!(loaded.config, Config::default());
    }

    #[test]
    fn load_reads_file_and_mentions_path_on_error() {
        let tmp = tempfile::tempdir().unwrap();
        let good = tmp.path().join("good.toml");
        std::fs::write(&good, "region = \"asia\"\nextra = true\n").unwrap();
        let loaded = load(&good, &no_env).unwrap();
        assert!(loaded.file_found);
        assert_eq!(loaded.config.region, Region::Asia);
        assert_eq!(loaded.warnings.len(), 1);

        let bad = tmp.path().join("bad.toml");
        std::fs::write(&bad, "[activities]\npage_size = 0\n").unwrap();
        let message = format!("{:#}", load(&bad, &no_env).unwrap_err());
        assert!(message.contains("page_size"), "{message}");
    }
}
