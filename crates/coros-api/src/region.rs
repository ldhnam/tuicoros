use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The COROS server cluster an account lives on.
///
/// A token from one region does not work on another, and the right region is
/// not always the one matching where the user lives (see `docs/COROS_API.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    /// Americas and "rest of world" (`teamapi.coros.com`). Some clients call it `en`.
    #[default]
    #[serde(alias = "en")]
    Us,
    /// Europe (`teameuapi.coros.com`).
    Eu,
    /// Asia and mainland China (`teamcnapi.coros.com`). Some clients call it `cn`.
    #[serde(alias = "cn")]
    Asia,
}

impl Region {
    pub const ALL: [Region; 3] = [Region::Us, Region::Eu, Region::Asia];

    /// Base URL of the Training Hub API for this region.
    pub const fn base_url(self) -> &'static str {
        match self {
            Region::Us => "https://teamapi.coros.com",
            Region::Eu => "https://teameuapi.coros.com",
            Region::Asia => "https://teamcnapi.coros.com",
        }
    }

    /// Short key used in config files and on the command line.
    pub const fn as_str(self) -> &'static str {
        match self {
            Region::Us => "us",
            Region::Eu => "eu",
            Region::Asia => "asia",
        }
    }

    /// Human-readable name for the login screen.
    pub const fn label(self) -> &'static str {
        match self {
            Region::Us => "Americas / rest of world",
            Region::Eu => "Europe",
            Region::Asia => "Asia / China",
        }
    }
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown region `{0}` (expected one of: us, eu, asia)")]
pub struct ParseRegionError(pub String);

impl FromStr for Region {
    type Err = ParseRegionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "us" | "en" => Ok(Region::Us),
            "eu" => Ok(Region::Eu),
            "asia" | "cn" => Ok(Region::Asia),
            _ => Err(ParseRegionError(s.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys_and_aliases_case_insensitively() {
        assert_eq!("us".parse(), Ok(Region::Us));
        assert_eq!("EN".parse(), Ok(Region::Us));
        assert_eq!(" eu ".parse(), Ok(Region::Eu));
        assert_eq!("Asia".parse(), Ok(Region::Asia));
        assert_eq!("cn".parse(), Ok(Region::Asia));
        assert!("mars".parse::<Region>().is_err());
    }

    #[test]
    fn display_round_trips() {
        for region in Region::ALL {
            assert_eq!(region.to_string().parse(), Ok(region));
        }
    }

    #[test]
    fn base_urls_are_https_and_distinct() {
        let urls: Vec<_> = Region::ALL.iter().map(|r| r.base_url()).collect();
        assert!(urls.iter().all(|u| u.starts_with("https://")));
        assert_eq!(
            urls.len(),
            urls.iter().collect::<std::collections::HashSet<_>>().len()
        );
    }

    #[test]
    fn deserializes_from_config_values() {
        #[derive(Deserialize)]
        struct C {
            region: Region,
        }
        let c: C = toml::from_str(r#"region = "cn""#).unwrap();
        assert_eq!(c.region, Region::Asia);
        let c: C = toml::from_str(r#"region = "en""#).unwrap();
        assert_eq!(c.region, Region::Us);
        assert!(toml::from_str::<C>(r#"region = "moon""#).is_err());
    }
}
