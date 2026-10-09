use crate::Error;

/// A validated high-pass cutoff in hertz that keeps bass a small speaker
/// cannot play out of it: off (`0`) or `MIN_HZ..=MAX_HZ`.
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
#[serde(try_from = "u16", into = "u16")]
pub struct BassCut(u16);

impl BassCut {
    /// No high-pass filter.
    pub const OFF: Self = Self(0);
    /// The lowest cutoff.
    pub const MIN_HZ: u16 = 40;
    /// The highest cutoff.
    pub const MAX_HZ: u16 = 300;

    /// Creates a cutoff; `0` means off.
    ///
    /// # Errors
    ///
    /// Returns [`Error::BassCutOutOfRange`] for values between 1 and
    /// [`Self::MIN_HZ`] or above [`Self::MAX_HZ`].
    pub const fn from_hz(hz: u16) -> Result<Self, Error> {
        if hz == 0 || (hz >= Self::MIN_HZ && hz <= Self::MAX_HZ) {
            Ok(Self(hz))
        } else {
            Err(Error::BassCutOutOfRange { hz })
        }
    }

    /// The cutoff in hertz, or `None` when off.
    #[must_use]
    pub const fn hz(self) -> Option<u16> {
        if self.0 == 0 { None } else { Some(self.0) }
    }

    /// The raw value: `0` when off.
    #[must_use]
    pub const fn as_hz(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for BassCut {
    type Error = Error;

    fn try_from(hz: u16) -> Result<Self, Error> {
        Self::from_hz(hz)
    }
}

impl From<BassCut> for u16 {
    fn from(cut: BassCut) -> Self {
        cut.as_hz()
    }
}

/// A validated amount of virtual bass in whole percent (`0..=100`, `0` off).
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
pub struct VirtualBass(u8);

impl VirtualBass {
    /// No virtual bass.
    pub const OFF: Self = Self(0);

    /// Creates an amount.
    ///
    /// # Errors
    ///
    /// Returns [`Error::VirtualBassOutOfRange`] when `percent` exceeds 100.
    pub const fn from_percent(percent: u8) -> Result<Self, Error> {
        if percent <= 100 {
            Ok(Self(percent))
        } else {
            Err(Error::VirtualBassOutOfRange { percent })
        }
    }

    /// The amount in percent.
    #[must_use]
    pub const fn as_percent(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for VirtualBass {
    type Error = Error;

    fn try_from(percent: u8) -> Result<Self, Error> {
        Self::from_percent(percent)
    }
}

impl From<VirtualBass> for u8 {
    fn from(amount: VirtualBass) -> Self {
        amount.as_percent()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn zero_is_off() {
        assert_eq!(BassCut::from_hz(0).unwrap(), BassCut::OFF);
        assert_eq!(BassCut::OFF.hz(), None);
        assert_eq!(BassCut::default(), BassCut::OFF);
    }

    #[test]
    fn cutoffs_outside_the_range_are_rejected() {
        assert_eq!(
            BassCut::from_hz(39),
            Err(Error::BassCutOutOfRange { hz: 39 })
        );
        assert_eq!(
            BassCut::from_hz(301),
            Err(Error::BassCutOutOfRange { hz: 301 })
        );
        assert_eq!(BassCut::from_hz(150).unwrap().hz(), Some(150));
    }

    #[test]
    fn virtual_bass_above_100_percent_is_rejected() {
        assert_eq!(
            VirtualBass::from_percent(101),
            Err(Error::VirtualBassOutOfRange { percent: 101 })
        );
        assert_eq!(VirtualBass::from_percent(60).unwrap().as_percent(), 60);
    }

    proptest! {
        #[test]
        fn valid_cutoffs_round_trip_through_serde(hz in BassCut::MIN_HZ..=BassCut::MAX_HZ) {
            #[derive(serde::Serialize, serde::Deserialize)]
            struct Wrapper {
                cut: BassCut,
            }
            let text = toml::to_string(&Wrapper { cut: BassCut::from_hz(hz).unwrap() }).unwrap();
            let back: Wrapper = toml::from_str(&text).unwrap();
            prop_assert_eq!(back.cut.as_hz(), hz);
        }
    }
}
