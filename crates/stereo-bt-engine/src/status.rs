use stereo_bt_core::DeviceId;

/// Lifecycle of one stream thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// The thread is opening its stream.
    Starting,
    /// Audio is flowing.
    Running,
    /// The stream could not open or the device failed (e.g. disconnected).
    Failed,
    /// The stream was stopped on request.
    Stopped,
}

/// Health of one output, for the status bar (buffer health).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputStatus {
    /// The device.
    pub id: DeviceId,
    /// Stream lifecycle.
    pub state: StreamState,
    /// Times the queue ran dry (audible gap).
    pub underruns: u64,
    /// Times captured audio was dropped because the queue was full.
    pub overruns: u64,
    /// Times clock drift was corrected by trimming the queue.
    pub trims: u64,
    /// Why the stream failed, when `state` is [`StreamState::Failed`].
    pub error: Option<String>,
    /// How loud it plays right now, as a meter fill in percent (-60 to 0 dB).
    pub level_percent: u8,
}
