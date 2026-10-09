use crate::Error;

/// A validated stereo width in whole percent: `0` is mono, `100` leaves the
/// stereo image unchanged, `200` doubles the difference between the channels.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(try_from = "u8", into = "u8")]
pub struct StereoWidth(u8);

impl StereoWidth {
    /// The unchanged stereo image.
    pub const NORMAL: Self = Self(100);
    /// The widest setting.
    pub const MAX_PERCENT: u8 = 200;

    /// Creates a width.
    ///
    /// # Errors
    ///
    /// Returns [`Error::StereoWidthOutOfRange`] above [`Self::MAX_PERCENT`].
    pub const fn from_percent(percent: u8) -> Result<Self, Error> {
        if percent <= Self::MAX_PERCENT {
            Ok(Self(percent))
        } else {
            Err(Error::StereoWidthOutOfRange { percent })
        }
    }

    /// The width in percent.
    #[must_use]
    pub const fn as_percent(self) -> u8 {
        self.0
    }
}

impl Default for StereoWidth {
    fn default() -> Self {
        Self::NORMAL
    }
}

impl TryFrom<u8> for StereoWidth {
    type Error = Error;

    fn try_from(percent: u8) -> Result<Self, Error> {
        Self::from_percent(percent)
    }
}

impl From<StereoWidth> for u8 {
    fn from(width: StereoWidth) -> Self {
        width.as_percent()
    }
}

/// A validated amount in whole percent (`0..=100`), e.g. reverb or room size.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(try_from = "u8", into = "u8")]
pub struct Percent(u8);

impl Percent {
    /// Zero percent.
    pub const ZERO: Self = Self(0);

    /// Creates an amount.
    ///
    /// # Errors
    ///
    /// Returns [`Error::PercentOutOfRange`] when `percent` exceeds 100.
    pub const fn new(percent: u8) -> Result<Self, Error> {
        if percent <= 100 {
            Ok(Self(percent))
        } else {
            Err(Error::PercentOutOfRange { percent })
        }
    }

    /// The amount in percent.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// The amount as a fraction in `0.0..=1.0`.
    #[must_use]
    pub fn fraction(self) -> f32 {
        f32::from(self.0) / 100.0
    }
}

impl TryFrom<u8> for Percent {
    type Error = Error;

    fn try_from(percent: u8) -> Result<Self, Error> {
        Self::new(percent)
    }
}

impl From<Percent> for u8 {
    fn from(amount: Percent) -> Self {
        amount.get()
    }
}

/// How spacious the sound is, for every device played through the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Space {
    /// Stereo width (mid/side).
    pub width: StereoWidth,
    /// How much reverb is added; `0` is off.
    pub reverb: Percent,
    /// How large the reverb's room sounds.
    pub room_size: Percent,
}

impl Default for Space {
    /// Normal width, no reverb, a medium room ready for when reverb is used.
    fn default() -> Self {
        Self {
            width: StereoWidth::NORMAL,
            reverb: Percent::ZERO,
            room_size: Percent(50),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_above_200_percent_is_rejected() {
        assert_eq!(
            StereoWidth::from_percent(201),
            Err(Error::StereoWidthOutOfRange { percent: 201 })
        );
        assert_eq!(StereoWidth::default(), StereoWidth::NORMAL);
    }

    #[test]
    fn percent_above_100_is_rejected() {
        assert_eq!(
            Percent::new(101),
            Err(Error::PercentOutOfRange { percent: 101 })
        );
        assert!((Percent::new(25).unwrap().fraction() - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn default_space_changes_nothing() {
        let space = Space::default();

        assert_eq!(space.width, StereoWidth::NORMAL);
        assert_eq!(space.reverb, Percent::ZERO);
    }

    #[test]
    fn space_round_trips_through_toml_and_old_files_get_defaults() {
        let space = Space {
            width: StereoWidth::from_percent(150).unwrap(),
            reverb: Percent::new(30).unwrap(),
            room_size: Percent::new(60).unwrap(),
        };
        let text = toml::to_string(&space).unwrap();

        assert_eq!(toml::from_str::<Space>(&text).unwrap(), space);
        assert_eq!(toml::from_str::<Space>("").unwrap(), Space::default());
    }
}
