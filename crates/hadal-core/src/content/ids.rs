//! Stable content identifiers.
//!
//! Every identifier serializes as a stable string. New variants may only be
//! appended; existing names must never change.

use core::fmt;
use core::str::FromStr;

use thiserror::Error;

/// Error returned when a string does not match any known content ID.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown id: {0}")]
pub struct UnknownId(pub String);

/// Declares a C-like enum with stable string IDs, `ALL`, `as_str`, `index`,
/// `Display`, `FromStr`, and serde string support.
macro_rules! id_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// Every variant in stable declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The stable string identifier.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $s),+
                }
            }

            /// Dense index in [`Self::ALL`].
            pub const fn index(self) -> usize {
                self as usize
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = UnknownId;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($s => Ok(Self::$variant),)+
                    _ => Err(UnknownId(s.to_owned())),
                }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct IdVisitor;

                impl serde::de::Visitor<'_> for IdVisitor {
                    type Value = $name;

                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        write!(f, "a {} id string", stringify!($name))
                    }

                    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<$name, E> {
                        $name::from_str(v).map_err(E::custom)
                    }
                }

                deserializer.deserialize_str(IdVisitor)
            }
        }
    };
}

id_enum! {
    /// Echo generators.
    GeneratorId {
        Hydrophone => "hydrophone",
        Drifter => "drifter",
        SonarMast => "sonar_mast",
        TidalCore => "tidal_core",
    }
}

id_enum! {
    /// In-run Signal Chain upgrades.
    UpgradeId {
        ImpedanceMatch => "impedance_match",
        SweepProfile => "sweep_profile",
        CascadedGain => "cascaded_gain",
        StableDrift => "stable_drift",
        CoherentAperture => "coherent_aperture",
        MastPhasing => "mast_phasing",
        CoreResonance => "core_resonance",
        FullSpectrum => "full_spectrum",
    }
}

id_enum! {
    /// Permanent hull upgrades.
    HullId {
        AutoPulse => "auto_pulse",
        BallastRecall => "ballast_recall",
        TunedBallast => "tuned_ballast",
        AutoBuyI => "auto_buy_i",
        DeepMemory => "deep_memory",
        CoiledHull => "coiled_hull",
        AutoBuyII => "auto_buy_ii",
        BallastGovernor => "ballast_governor",
        LongStorage => "long_storage",
        ChainAutomator => "chain_automator",
        NullArray => "null_array",
        AutoBuyIII => "auto_buy_iii",
        ExtendedBallast => "extended_ballast",
        TidalLoop => "tidal_loop",
        AbyssalReservoir => "abyssal_reservoir",
        DeepField => "deep_field",
        FractalHull => "fractal_hull",
    }
}

id_enum! {
    /// Achievements.
    AchievementId {
        FirstContact => "first_contact",
        Hydrophile => "hydrophile",
        Chorus => "chorus",
        Drifter => "drifter",
        Mast => "mast",
        Core => "core",
        FirstDescent => "first_descent",
        SixDeep => "six_deep",
        Century => "century",
        AllBands => "all_bands",
        Signal => "signal",
    }
}

id_enum! {
    /// Archive log entries.
    LogId {
        Boot => "boot",
        FirstPulse => "first_pulse",
        FirstHydro => "first_hydro",
        CarrierLock => "carrier_lock",
        FirstDescent => "first_descent",
        Depth9000 => "depth_9000",
        Peak1e9 => "peak_1e9",
        Fathoms100 => "fathoms_100",
        AllBands => "all_bands",
        AutoPulse => "auto_pulse",
    }
}

id_enum! {
    /// Carrier bands 0..=5.
    CarrierBand {
        Zero => "0",
        One => "1",
        Two => "2",
        Three => "3",
        Four => "4",
        Five => "5",
    }
}

impl CarrierBand {
    /// Total number of bands.
    pub const COUNT: u8 = 6;

    /// Converts a raw band index into a [`CarrierBand`].
    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(v as usize).copied()
    }

    /// The raw band index.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fmt::Debug;

    use serde::Serialize;
    use serde::de::DeserializeOwned;

    use super::*;

    fn check_id_enum<T>(all: &'static [T])
    where
        T: Copy + PartialEq + Debug + fmt::Display + FromStr + Serialize + DeserializeOwned,
        T::Err: Debug,
    {
        let mut seen = BTreeSet::new();
        for &id in all {
            let text = id.to_string();
            assert!(seen.insert(text.clone()), "duplicate id string: {text}");
            let parsed: T = text.parse().unwrap();
            assert_eq!(parsed, id, "FromStr round-trip failed for {text}");
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(json, format!("\"{text}\""));
            let back: T = serde_json::from_str(&json).unwrap();
            assert_eq!(back, id);
        }
        assert_eq!(seen.len(), all.len());
    }

    #[test]
    fn test_generator_ids() {
        check_id_enum(GeneratorId::ALL);
        assert_eq!(GeneratorId::ALL.len(), 4);
        assert_eq!(GeneratorId::Hydrophone.index(), 0);
        assert_eq!(GeneratorId::TidalCore.as_str(), "tidal_core");
    }

    #[test]
    fn test_upgrade_ids() {
        check_id_enum(UpgradeId::ALL);
        assert_eq!(UpgradeId::ALL.len(), 8);
        assert_eq!(UpgradeId::FullSpectrum.index(), 7);
    }

    #[test]
    fn test_hull_ids() {
        check_id_enum(HullId::ALL);
        assert_eq!(HullId::ALL.len(), 17);
        assert_eq!(HullId::FractalHull.index(), 16);
    }

    #[test]
    fn test_achievement_ids() {
        check_id_enum(AchievementId::ALL);
        assert_eq!(AchievementId::ALL.len(), 11);
    }

    #[test]
    fn test_log_ids() {
        check_id_enum(LogId::ALL);
        assert_eq!(LogId::ALL.len(), 10);
        assert_eq!(LogId::Boot.as_str(), "boot");
    }

    #[test]
    fn test_carrier_bands() {
        check_id_enum(CarrierBand::ALL);
        assert_eq!(CarrierBand::ALL.len() as u8, CarrierBand::COUNT);
        assert_eq!(CarrierBand::from_u8(3), Some(CarrierBand::Three));
        assert_eq!(CarrierBand::from_u8(6), None);
        assert_eq!(CarrierBand::Five.as_u8(), 5);
    }

    #[test]
    fn test_unknown_id_rejected() {
        assert!(GeneratorId::from_str("warp_core").is_err());
        assert!(LogId::from_str("").is_err());
    }
}
