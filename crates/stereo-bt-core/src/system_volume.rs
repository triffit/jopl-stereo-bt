/// The Windows volume of the default (source) device, which every output
/// follows when "Follow system volume" is on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SystemVolume {
    level_db: f32,
    scalar: f32,
    muted: bool,
}

impl SystemVolume {
    /// Full volume, not muted.
    pub const FULL: Self = Self {
        level_db: 0.0,
        scalar: 1.0,
        muted: false,
    };

    /// From the endpoint's attenuation in decibels, its slider position
    /// (`0.0..=1.0`) and its mute state.
    #[must_use]
    pub const fn new(level_db: f32, scalar: f32, muted: bool) -> Self {
        Self {
            level_db,
            scalar,
            muted,
        }
    }

    /// Linear amplitude matching what Windows applies to the device itself:
    /// its decibel attenuation, or silence when muted or the slider is at 0.
    #[must_use]
    pub fn amplitude(self) -> f32 {
        if self.muted || self.scalar <= 0.0 {
            return 0.0;
        }
        10.0_f32.powf(self.level_db.min(0.0) / 20.0)
    }

    /// The slider position in whole percent.
    #[must_use]
    pub fn percent(self) -> u8 {
        // Clamped to 0..=100 first, so the cast cannot truncate or wrap.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let percent = (self.scalar.clamp(0.0, 1.0) * 100.0).round() as u8;
        percent
    }

    /// The slider position, `0.0..=1.0`.
    #[must_use]
    pub const fn scalar(self) -> f32 {
        self.scalar
    }

    /// Whether the device is muted.
    #[must_use]
    pub const fn is_muted(self) -> bool {
        self.muted
    }

    /// The same volume with the mute flag cleared.
    #[must_use]
    pub const fn unmuted(self) -> Self {
        Self {
            muted: false,
            ..self
        }
    }
}
