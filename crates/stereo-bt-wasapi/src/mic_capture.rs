use wasapi::{
    AudioCaptureClient, AudioClient, AudioClientProperties, Device, DeviceEnumerator, Direction,
    Handle, StreamMode, StreamOption, WasapiError,
};

use crate::{Error, stream_format::wave_format};

/// Microphones at or below this native rate are Bluetooth hands-free (call)
/// microphones: unusable for measurement, and opening one switches the
/// headset or speaker into low-quality call mode.
const MIN_MIC_RATE: u32 = 32_000;

/// A microphone recording for latency measurement, in RAW mode so Windows
/// noise suppression and gain control cannot mute the test signal.
///
/// Buffers are allocated in [`MicCapture::open`]; reading never allocates.
pub struct MicCapture {
    name: String,
    audio_client: AudioClient,
    capture_client: AudioCaptureClient,
    event: Handle,
    bytes: Vec<u8>,
    samples: Vec<f32>,
}

impl MicCapture {
    /// Opens the default microphone, or another one when the default is a
    /// Bluetooth hands-free microphone.
    ///
    /// Requires a [`crate::ComGuard`] on the calling thread.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoMicrophone`] when no suitable microphone exists, or
    /// another [`Error`] when it cannot be opened.
    pub fn open() -> Result<Self, Error> {
        let device = suitable_microphone()?.ok_or(Error::NoMicrophone)?;
        let name = device.get_friendlyname()?;
        let mut audio_client = device.get_iaudioclient()?;
        // RAW bypasses voice processing; without it the laptop microphone
        // was measured gating test chirps to silence.
        audio_client.set_properties(AudioClientProperties::new().set_option(StreamOption::Raw))?;
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
            name,
            audio_client,
            capture_client,
            event,
            bytes: vec![0; buffer_frames * 8],
            samples: vec![0.0; buffer_frames],
        })
    }

    /// The microphone's name, e.g. "Microphone Array (Realtek(R) Audio)".
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Starts recording.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the stream cannot start.
    pub fn start(&self) -> Result<(), Error> {
        Ok(self.audio_client.start_stream()?)
    }

    /// Waits up to `timeout_ms` and returns the next packet as mono samples
    /// (empty when none arrived).
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the device fails.
    pub fn read(&mut self, timeout_ms: u32) -> Result<&[f32], Error> {
        match self.event.wait_for_event(timeout_ms) {
            Ok(()) | Err(WasapiError::EventTimeout) => {}
            Err(other) => return Err(other.into()),
        }
        let (frames, info) = self.capture_client.read_from_device(&mut self.bytes)?;
        let frames = frames as usize;
        let samples = &mut self.samples[..frames];
        if info.flags.silent {
            samples.fill(0.0);
        } else {
            for (sample, bytes) in samples.iter_mut().zip(self.bytes.as_chunks::<8>().0) {
                let [l0, l1, l2, l3, r0, r1, r2, r3] = *bytes;
                let left = f32::from_le_bytes([l0, l1, l2, l3]);
                let right = f32::from_le_bytes([r0, r1, r2, r3]);
                *sample = f32::midpoint(left, right);
            }
        }
        Ok(samples)
    }

    /// Frames recorded but not yet read; used to date each packet precisely.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the device fails.
    pub fn queued_frames(&self) -> Result<usize, Error> {
        Ok(self.audio_client.get_current_padding()? as usize)
    }
}

fn suitable_microphone() -> Result<Option<Device>, Error> {
    let enumerator = DeviceEnumerator::new()?;
    let usable = |device: &Device| {
        device
            .get_device_format()
            .is_ok_and(|format| format.get_samplespersec() >= MIN_MIC_RATE)
    };
    if let Ok(default) = enumerator.get_default_device(&Direction::Capture)
        && usable(&default)
    {
        return Ok(Some(default));
    }
    for device in &enumerator.get_device_collection(&Direction::Capture)? {
        let device = device?;
        if usable(&device) {
            return Ok(Some(device));
        }
    }
    Ok(None)
}
