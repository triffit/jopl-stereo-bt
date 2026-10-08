use stereo_bt_core::{DeviceId, Frame, pcm};
use wasapi::{AudioClient, AudioRenderClient, DeviceEnumerator, Direction, Handle, StreamMode};

use crate::{Error, stream_format::wave_format};

/// An event-driven shared-mode output stream on one specific device.
///
/// All buffers are allocated in [`RenderStream::open`]; writing never allocates.
pub struct RenderStream {
    audio_client: AudioClient,
    render_client: AudioRenderClient,
    event: Handle,
    bytes: Vec<u8>,
}

impl RenderStream {
    /// Opens an output stream on the device with identifier `id`.
    ///
    /// Requires a [`crate::ComGuard`] on the calling thread.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the device does not exist, is not active, or
    /// the stream cannot be initialised.
    pub fn open(id: &DeviceId) -> Result<Self, Error> {
        let enumerator = DeviceEnumerator::new()?;
        let device = enumerator.get_device(id.as_str())?;
        let mut audio_client = device.get_iaudioclient()?;
        let (default_period, _min_period) = audio_client.get_device_period()?;
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: default_period,
        };
        audio_client.initialize_client(&wave_format(), &Direction::Render, &mode)?;
        let event = audio_client.set_get_eventhandle()?;
        let buffer_frames = audio_client.get_buffer_size()? as usize;
        let render_client = audio_client.get_audiorenderclient()?;
        Ok(Self {
            audio_client,
            render_client,
            event,
            bytes: vec![0; buffer_frames * pcm::BYTES_PER_FRAME],
        })
    }

    /// Size of the device buffer in frames; the most [`Self::write`] accepts.
    #[must_use]
    pub fn buffer_frames(&self) -> usize {
        self.bytes.len() / pcm::BYTES_PER_FRAME
    }

    /// Starts playback.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the stream cannot be started.
    pub fn start(&self) -> Result<(), Error> {
        Ok(self.audio_client.start_stream()?)
    }

    /// Stops playback.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the stream cannot be stopped.
    pub fn stop(&self) -> Result<(), Error> {
        Ok(self.audio_client.stop_stream()?)
    }

    /// Waits up to `timeout_ms` for the device to request more audio.
    /// Returns `false` on timeout.
    #[must_use]
    pub fn wait(&self, timeout_ms: u32) -> bool {
        self.event.wait_for_event(timeout_ms).is_ok()
    }

    /// Number of frames that can be written right now.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the device fails, e.g. it was disconnected.
    pub fn writable_frames(&self) -> Result<usize, Error> {
        Ok(self.audio_client.get_available_space_in_frames()? as usize)
    }

    /// Writes up to [`Self::writable_frames`] frames and returns how many were written.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the device fails or more frames are passed than
    /// there is space for.
    pub fn write(&mut self, frames: &[Frame]) -> Result<usize, Error> {
        let count = pcm::encode_f32_stereo(frames, &mut self.bytes);
        self.render_client.write_to_device(
            count,
            &self.bytes[..count * pcm::BYTES_PER_FRAME],
            None,
        )?;
        Ok(count)
    }
}
