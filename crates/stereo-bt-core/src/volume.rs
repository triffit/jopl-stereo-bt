use crate::Error;

/// A validated per-device volume in whole percent (`0..=100`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(try_from = "u8", into = "u8")]
pub struct Volume(u8);

impl Volume {
    /// Full volume (unity gain).
    pub const FULL: Self = Self(100);

    /// Creates a volume.
    ///
    /// # Errors
    ///
    /// Returns [`Error::VolumeOutOfRange`] when `percent` exceeds 100.
    pub const fn from_percent(percent: u8) -> Result<Self, Error> {
        if percent <= 100 {
            Ok(Self(percent))
        } else {
            Err(Error::VolumeOutOfRange { percent })
        }
    }

    /// The volume in percent.
    #[must_use]
    pub const fn as_percent(self) -> u8 {
        self.0
    }

    /// Linear amplitude gain in `0.0..=1.0`.
    ///
    /// Uses a squared curve so the slider feels perceptually even:
    /// 50 % is roughly -12 dB rather than -6 dB.
    #[must_use]
    pub fn gain(self) -> f32 {
        let fraction = f32::from(self.0) / 100.0;
        fraction * fraction
    }
}

impl Default for Volume {
    fn default() -> Self {
        Self::FULL
    }
}

impl TryFrom<u8> for Volume {
    type Error = Error;

    fn try_from(percent: u8) -> Result<Self, Error> {
        Self::from_percent(percent)
    }
}

impl From<Volume> for u8 {
    fn from(volume: Volume) -> Self {
        volume.as_percent()
    }
}
