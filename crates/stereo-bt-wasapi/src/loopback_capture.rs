use stereo_bt_core::{Frame, pcm};
use wasapi::{
    AudioCaptureClient, AudioClient, DeviceEnumerator, Direction, Handle, StreamMode, WasapiError,
};

use crate::{Error, stream_format::wave_format};

/// Captures everything played on the default output device (WASAPI loopback).
///
/// All buffers are allocated in [`LoopbackCapture::open_default`];
/// [`LoopbackCapture::read`] never allocates.
pub struct LoopbackCapture {
    audio_client: AudioClient,
    capture_client: AudioCaptureClient,
    event: Handle,
    bytes: Vec<u8>,
    frames: Vec<Frame>,
}

impl LoopbackCapture {
    /// Opens a loopback stream on the current default output device.
    ///
    /// Requires a [`crate::ComGuard`] on the calling thread.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when there is no default device or the stream
    /// cannot be initialised.
    pub fn open_default() -> Result<Self, Error> {
        let enumerator = DeviceEnumerator::new()?;
        let device = enumerator.get_default_device(&Direction::Render)?;
        let mut audio_client = device.get_iaudioclient()?;
        let (default_period, _min_period) = audio_client.get_device_period()?;
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: default_period,
        };
        audio_client.initialize_client(&wave_format(), &Direction::Capture, &mode)?;
        let event = audio_client.set_get_eventhandle()?;
        let buffer_frames = audio_client.get_buffer_size()? as usize;
        let capture_client = audio_client.get_audiocaptureclient()?;
        Ok(Self {
            audio_client,
            capture_client,
            event,
            bytes: vec![0; buffer_frames * pcm::BYTES_PER_FRAME],
            frames: vec![Frame::SILENCE; buffer_frames],
        })
    }

    /// Starts capturing.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the stream cannot be started.
    pub fn start(&self) -> Result<(), Error> {
        Ok(self.audio_client.start_stream()?)
    }

    /// Stops capturing.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the stream cannot be stopped.
    pub fn stop(&self) -> Result<(), Error> {
        Ok(self.audio_client.stop_stream()?)
    }

    /// Waits up to `timeout_ms` for audio, then returns the next captured packet.
    ///
    /// Returns an empty slice when nothing arrived; loopback delivers no
    /// packets while nothing is playing. Call repeatedly with a zero timeout
    /// to drain all queued packets.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the device fails, e.g. it was disconnected.
    pub fn read(&mut self, timeout_ms: u32) -> Result<&[Frame], Error> {
        match self.event.wait_for_event(timeout_ms) {
            // On timeout there may still be queued packets, so read anyway.
            Ok(()) | Err(WasapiError::EventTimeout) => {}
            Err(other) => return Err(other.into()),
        }
        let (frame_count, info) = self.capture_client.read_from_device(&mut self.bytes)?;
        let frame_count = frame_count as usize;
        let frames = &mut self.frames[..frame_count];
        if info.flags.silent {
            frames.fill(Frame::SILENCE);
        } else {
            pcm::decode_f32_stereo(&self.bytes[..frame_count * pcm::BYTES_PER_FRAME], frames);
        }
        Ok(frames)
    }
}
