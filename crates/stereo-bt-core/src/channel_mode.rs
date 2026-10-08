use crate::Frame;

/// Which part of the stereo signal a device plays.
///
/// Two devices set to [`ChannelMode::LeftOnly`] and [`ChannelMode::RightOnly`]
/// form a stereo pair.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum ChannelMode {
    /// Left to left, right to right.
    #[default]
    Stereo,
    /// The left channel on both speakers of the device.
    LeftOnly,
    /// The right channel on both speakers of the device.
    RightOnly,
    /// The average of both channels on both speakers of the device.
    MonoMix,
}

impl ChannelMode {
    /// Maps one input frame to the frame this device should play.
    #[must_use]
    pub fn apply(self, frame: Frame) -> Frame {
        match self {
            Self::Stereo => frame,
            Self::LeftOnly => Frame::new(frame.left, frame.left),
            Self::RightOnly => Frame::new(frame.right, frame.right),
            Self::MonoMix => {
                let mono = f32::midpoint(frame.left, frame.right);
                Frame::new(mono, mono)
            }
        }
    }
}
