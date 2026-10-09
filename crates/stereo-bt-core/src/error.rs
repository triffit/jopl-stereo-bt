/// Errors raised when validating user or device supplied values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The requested sync delay exceeds [`crate::Delay::MAX_MILLIS`].
    #[error("delay of {millis} ms is out of range (0..={max} ms)", max = crate::Delay::MAX_MILLIS)]
    DelayOutOfRange {
        /// The rejected value in milliseconds.
        millis: u16,
    },
    /// The requested volume exceeds 100 %.
    #[error("volume of {percent} % is out of range (0..=100 %)")]
    VolumeOutOfRange {
        /// The rejected value in percent.
        percent: u8,
    },
    /// The bass cut is neither off nor within its range.
    #[error(
        "bass cut of {hz} Hz is out of range (0 for off, or {min}..={max} Hz)",
        min = crate::BassCut::MIN_HZ,
        max = crate::BassCut::MAX_HZ
    )]
    BassCutOutOfRange {
        /// The rejected value in hertz.
        hz: u16,
    },
    /// The requested virtual bass exceeds 100 %.
    #[error("virtual bass of {percent} % is out of range (0..=100 %)")]
    VirtualBassOutOfRange {
        /// The rejected value in percent.
        percent: u8,
    },
    /// The sample rate is outside the supported range.
    #[error(
        "sample rate of {hz} Hz is out of range ({min}..={max} Hz)",
        min = crate::SampleRate::MIN_HZ,
        max = crate::SampleRate::MAX_HZ
    )]
    SampleRateOutOfRange {
        /// The rejected value in hertz.
        hz: u32,
    },
    /// A device identifier is empty, too long or contains control characters.
    #[error("invalid device identifier")]
    InvalidDeviceId,
}
