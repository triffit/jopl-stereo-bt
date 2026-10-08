use crate::Error;

/// A validated audio sample rate in hertz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SampleRate(u32);

impl SampleRate {
    /// Lowest supported sample rate.
    pub const MIN_HZ: u32 = 8_000;
    /// Highest supported sample rate.
    pub const MAX_HZ: u32 = 384_000;
    /// 48 kHz, the internal stream rate used on every endpoint.
    pub const HZ_48000: Self = Self(48_000);

    /// Creates a sample rate.
    ///
    /// # Errors
    ///
    /// Returns [`Error::SampleRateOutOfRange`] when `hz` is outside
    /// [`Self::MIN_HZ`]`..=`[`Self::MAX_HZ`].
    pub const fn from_hz(hz: u32) -> Result<Self, Error> {
        if hz >= Self::MIN_HZ && hz <= Self::MAX_HZ {
            Ok(Self(hz))
        } else {
            Err(Error::SampleRateOutOfRange { hz })
        }
    }

    /// The rate in hertz.
    #[must_use]
    pub const fn as_hz(self) -> u32 {
        self.0
    }
}
