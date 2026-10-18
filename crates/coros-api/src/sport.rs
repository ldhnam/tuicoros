//! Sport types used by `activity/query` (`sportType`, `modeList`) and the detail
//! endpoints. Values from the Training Hub's own list (via Pinta365/coros).
//!
//! The workout library uses a *different* numbering (runs are `1`), so workouts
//! keep their raw code instead of using this enum.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::CorosError;

/// Broad grouping that decides how pace/speed is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SportCategory {
    Run,
    Walk,
    Bike,
    Swim,
    Strength,
    Other,
}

macro_rules! sports {
    ($( $variant:ident = $code:literal, $key:literal, $label:literal, $cat:ident; )*) => {
        /// A COROS sport. Unknown codes are kept (`Unknown`), never rejected:
        /// COROS adds sports over time.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum SportType {
            $( $variant, )*
            Unknown(u32),
        }

        impl SportType {
            pub const ALL: &'static [SportType] = &[ $( SportType::$variant, )* ];

            pub fn from_code(code: u32) -> SportType {
                match code {
                    $( $code => SportType::$variant, )*
                    other => SportType::Unknown(other),
                }
            }

            pub fn code(self) -> u32 {
                match self {
                    $( SportType::$variant => $code, )*
                    SportType::Unknown(code) => code,
                }
            }

            /// Stable key used on the command line and in JSON (`trailRun`).
            pub fn key(self) -> &'static str {
                match self {
                    $( SportType::$variant => $key, )*
                    SportType::Unknown(_) => "unknown",
                }
            }

            /// Name as shown in the UI.
            pub fn label(self) -> &'static str {
                match self {
                    $( SportType::$variant => $label, )*
                    SportType::Unknown(_) => "Other",
                }
            }

            pub fn category(self) -> SportCategory {
                match self {
                    $( SportType::$variant => SportCategory::$cat, )*
                    SportType::Unknown(_) => SportCategory::Other,
                }
            }
        }
    };
}

sports! {
    Run = 100, "run", "Run", Run;
    IndoorRun = 101, "indoorRun", "Indoor Run", Run;
    TrailRun = 102, "trailRun", "Trail Run", Run;
    TrackRun = 103, "trackRun", "Track Run", Run;
    Hike = 104, "hike", "Hike", Walk;
    MountainClimb = 105, "mtnClimb", "Mountain Climb", Walk;
    Climb = 106, "climb", "Climb", Other;
    Bike = 200, "bike", "Bike", Bike;
    IndoorBike = 201, "indoorBike", "Indoor Bike", Bike;
    RoadEbike = 202, "roadEbike", "Road E-Bike", Bike;
    GravelBike = 203, "gravelRoadBike", "Gravel Bike", Bike;
    MountainBike = 204, "mountainRiding", "Mountain Bike", Bike;
    MountainEbike = 205, "mountainEbike", "E-MTB", Bike;
    HelmetBike = 299, "helmetBike", "Bike (Helmet)", Bike;
    PoolSwim = 300, "poolSwim", "Pool Swim", Swim;
    OpenWater = 301, "openWater", "Open Water", Swim;
    GymCardio = 400, "gymCardio", "Gym Cardio", Other;
    GpsCardio = 401, "gpsCardio", "GPS Cardio", Other;
    Strength = 402, "strength", "Strength", Strength;
    Ski = 500, "ski", "Ski", Other;
    Snowboard = 501, "snowboard", "Snowboard", Other;
    XcSki = 502, "xcSki", "XC Ski", Other;
    SkiTouring = 503, "skiTouring", "Ski Touring", Other;
    Row = 700, "row", "Row", Other;
    IndoorRow = 701, "indoorRow", "Indoor Row", Other;
    Whitewater = 702, "whitewater", "Whitewater", Other;
    Flatwater = 704, "flatwater", "Flatwater", Other;
    Windsurfing = 705, "windsurfing", "Windsurfing", Other;
    Speedsurfing = 706, "speedsurfing", "Speedsurfing", Other;
    BoatFishing = 707, "boatFishing", "Boat Fishing", Other;
    ShoreFishing = 708, "shoreFishing", "Shore Fishing", Other;
    KayakFishing = 710, "kayakFishing", "Kayak Fishing", Other;
    InshoreFishing = 711, "inshoreFishing", "Inshore Fishing", Other;
    OffshoreFishing = 712, "offshoreFishing", "Offshore Fishing", Other;
    BoatFlyFishing = 713, "boatFlyFishing", "Boat Fly Fishing", Other;
    ShoreFlyFishing = 714, "shoreFlyFishing", "Shore Fly Fishing", Other;
    IndoorClimb = 800, "indoorClimb", "Indoor Climb", Other;
    Bouldering = 801, "bouldering", "Bouldering", Other;
    OutdoorClimb = 802, "outdoorClimb", "Outdoor Climb", Other;
    Walk = 900, "walk", "Walk", Walk;
    JumpRope = 901, "jumpRope", "Jump Rope", Other;
    ClimbStairs = 902, "climbStairs", "Climb Stairs", Other;
    Elliptical = 903, "elliptical", "Elliptical", Other;
    Yoga = 904, "yoga", "Yoga", Other;
    Pilates = 905, "pilates", "Pilates", Other;
    Boxing = 906, "boxing", "Boxing", Other;
    Badminton = 1000, "badminton", "Badminton", Other;
    TableTennis = 1001, "tableTennis", "Table Tennis", Other;
    Basketball = 1002, "basketball", "Basketball", Other;
    Soccer = 1003, "soccer", "Soccer", Other;
    Pickleball = 1004, "pickleball", "Pickleball", Other;
    Tennis = 1005, "tennis", "Tennis", Other;
    Padel = 1006, "padel", "Padel", Other;
    Frisbee = 1100, "frisbee", "Frisbee", Other;
    Skateboard = 1101, "skateboard", "Skateboard", Other;
    HybridFitness = 1200, "hybridFitness", "Hybrid Fitness", Other;
    Triathlon = 10000, "triathlon", "Triathlon", Other;
    MultiSport = 10001, "multiSport", "Multisport", Other;
    SkiTouringOld = 10002, "skiTouringOld", "Ski Touring (old)", Other;
    MultiPitch = 10003, "multiPitch", "Multi-pitch Climb", Other;
    CustomSport = 98, "customSport", "Custom Sport", Other;
}

impl SportType {
    /// All running variants, for "runs only" filters.
    pub const RUNS: [SportType; 4] = [
        SportType::Run,
        SportType::IndoorRun,
        SportType::TrailRun,
        SportType::TrackRun,
    ];
}

impl fmt::Display for SportType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SportType::Unknown(code) => write!(f, "unknown:{code}"),
            other => f.pad(other.key()),
        }
    }
}

impl Serialize for SportType {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SportType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Accepts a key in any case/separator style (`trail-run`, `TRAIL_RUN`,
/// `trailRun`) or a numeric code (`102`).
impl FromStr for SportType {
    type Err = CorosError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let s = s.strip_prefix("unknown:").unwrap_or(s);
        if let Ok(code) = s.parse::<u32>() {
            return Ok(SportType::from_code(code));
        }
        let wanted = normalize(s);
        SportType::ALL
            .iter()
            .copied()
            .find(|sport| normalize(sport.key()) == wanted)
            .ok_or_else(|| CorosError::InvalidInput(format!("unknown sport `{s}`")))
    }
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn codes_and_keys_are_unique_and_round_trip() {
        let codes: HashSet<_> = SportType::ALL.iter().map(|s| s.code()).collect();
        let keys: HashSet<_> = SportType::ALL.iter().map(|s| normalize(s.key())).collect();
        assert_eq!(codes.len(), SportType::ALL.len());
        assert_eq!(keys.len(), SportType::ALL.len());
        assert_eq!(SportType::ALL.len(), 61);
        for sport in SportType::ALL {
            assert_eq!(SportType::from_code(sport.code()), *sport);
            assert_eq!(sport.key().parse::<SportType>().unwrap(), *sport);
        }
    }

    #[test]
    fn unknown_codes_are_kept() {
        let s = SportType::from_code(4242);
        assert_eq!(s, SportType::Unknown(4242));
        assert_eq!(s.code(), 4242);
        assert_eq!(s.to_string(), "unknown:4242");
        assert_eq!(s.category(), SportCategory::Other);
    }

    #[test]
    fn parsing_is_forgiving() {
        assert_eq!(
            "trail-run".parse::<SportType>().unwrap(),
            SportType::TrailRun
        );
        assert_eq!(
            "TRACK_RUN".parse::<SportType>().unwrap(),
            SportType::TrackRun
        );
        assert_eq!("100".parse::<SportType>().unwrap(), SportType::Run);
        assert!("quidditch".parse::<SportType>().is_err());
    }

    #[test]
    fn categories() {
        assert!(
            SportType::RUNS
                .iter()
                .all(|s| s.category() == SportCategory::Run)
        );
        assert_eq!(SportType::Bike.category(), SportCategory::Bike);
        assert_eq!(SportType::PoolSwim.category(), SportCategory::Swim);
        assert_eq!(SportType::Strength.category(), SportCategory::Strength);
        assert_eq!(SportType::Walk.category(), SportCategory::Walk);
    }

    #[test]
    fn serializes_as_key() {
        assert_eq!(
            serde_json::to_string(&SportType::TrailRun).unwrap(),
            "\"trailRun\""
        );
    }
}
