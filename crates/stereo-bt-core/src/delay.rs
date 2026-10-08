use crate::{Error, SampleRate};

/// A validated per-device sync delay in whole milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Delay(u16);

impl Delay {
    /// Largest supported delay in milliseconds.
    pub const MAX_MILLIS: u16 = 500;
    /// The largest supported delay.
    pub const MAX: Self = Self(Self::MAX_MILLIS);
    /// No delay.
    pub const ZERO: Self = Self(0);

    /// Creates a delay.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DelayOutOfRange`] when `millis` exceeds [`Self::MAX_MILLIS`].
    pub const fn from_millis(millis: u16) -> Result<Self, Error> {
        if millis <= Self::MAX_MILLIS {
            Ok(Self(millis))
        } else {
            Err(Error::DelayOutOfRange { millis })
        }
    }

    /// The delay in milliseconds.
    #[must_use]
    pub const fn as_millis(self) -> u16 {
        self.0
    }

    /// The delay expressed in whole frames at `rate`, rounded down.
    #[must_use]
    pub const fn frames_at(self, rate: SampleRate) -> usize {
        // At most 500 ms * 384 kHz = 192_000_000, which fits in u32 and usize.
        (self.0 as u32 * rate.as_hz() / 1_000) as usize
    }
}
