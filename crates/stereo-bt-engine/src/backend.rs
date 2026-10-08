use std::error::Error as StdError;

use stereo_bt_core::{DeviceId, Frame};

/// The audio system the engine runs on: WASAPI in production, fakes in tests.
///
/// Streams are opened on the thread that uses them, because platform audio
/// objects (COM) are tied to their thread.
pub trait Backend: Send + Sync + 'static {
    /// Error returned by every backend operation.
    type Error: StdError + Send + Sync + 'static;
    /// Per-thread setup kept alive while the thread uses audio (e.g. COM).
    type ThreadGuard;
    /// Loopback capture stream.
    type Capture: CaptureStream<Error = Self::Error>;
    /// Output stream on one device.
    type Render: RenderSink<Error = Self::Error>;

    /// Prepares the current thread for audio work.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread cannot be prepared.
    fn enter_thread(&self) -> Result<Self::ThreadGuard, Self::Error>;

    /// Opens the capture stream of the audio source.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no source or it cannot be opened.
    fn open_capture(&self) -> Result<Self::Capture, Self::Error>;

    /// Opens an output stream on device `id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the device is missing or cannot be opened.
    fn open_render(&self, id: &DeviceId) -> Result<Self::Render, Self::Error>;
}

/// A source of captured audio.
pub trait CaptureStream {
    /// Error returned by stream operations.
    type Error: StdError + Send + Sync + 'static;

    /// Starts capturing.
    ///
    /// # Errors
    ///
    /// Returns an error when the stream cannot start.
    fn start(&mut self) -> Result<(), Self::Error>;

    /// Waits up to `timeout_ms` and returns the next captured block, which may be empty.
    ///
    /// # Errors
    ///
    /// Returns an error when the source fails.
    fn read(&mut self, timeout_ms: u32) -> Result<&[Frame], Self::Error>;
}

/// An output device stream.
pub trait RenderSink {
    /// Error returned by stream operations.
    type Error: StdError + Send + Sync + 'static;

    /// Device buffer size in frames; the most one write accepts.
    fn buffer_frames(&self) -> usize;

    /// Starts playback.
    ///
    /// # Errors
    ///
    /// Returns an error when the stream cannot start.
    fn start(&mut self) -> Result<(), Self::Error>;

    /// Waits up to `timeout_ms` for the device to want more audio; `false` on timeout.
    fn wait(&mut self, timeout_ms: u32) -> bool;

    /// Frames that can be written now.
    ///
    /// # Errors
    ///
    /// Returns an error when the device fails.
    fn writable_frames(&mut self) -> Result<usize, Self::Error>;

    /// Writes `frames` (at most [`Self::writable_frames`]).
    ///
    /// # Errors
    ///
    /// Returns an error when the device fails.
    fn write(&mut self, frames: &[Frame]) -> Result<(), Self::Error>;
}
