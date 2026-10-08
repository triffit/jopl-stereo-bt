use std::sync::Arc;

use stereo_bt_core::{DeviceId, Frame, SampleRate};
use stereo_bt_wasapi::{
    ComGuard, Error, LoopbackCapture, MmcssGuard, RenderStream, STREAM_SAMPLE_RATE,
    default_output_id, is_muted, list_output_devices, set_muted,
};

use crate::{Backend, CaptureStream, Platform, RenderSink};

/// The Windows audio backend: loopback capture of the default output device
/// and shared-mode render streams.
#[derive(Debug, Default, Clone, Copy)]
pub struct WasapiBackend;

/// COM plus, for streaming threads, MMCSS "Pro Audio" priority.
/// Fields drop in order: MMCSS is reverted before COM is released.
pub struct AudioThreadGuard {
    _mmcss: Option<MmcssGuard>,
    _com: ComGuard,
}

impl AudioThreadGuard {
    /// Setup for a real-time streaming thread.
    fn streaming() -> Result<Self, Error> {
        let com = ComGuard::init_mta()?;
        // Without MMCSS the thread still works, only at normal priority.
        let mmcss = MmcssGuard::pro_audio().ok();
        Ok(Self {
            _mmcss: mmcss,
            _com: com,
        })
    }

    /// Setup for the (non-real-time) engine control thread.
    pub(crate) fn control() -> Result<Self, Error> {
        Ok(Self {
            _mmcss: None,
            _com: ComGuard::init_mta()?,
        })
    }
}

impl Backend for WasapiBackend {
    type Error = Error;
    type ThreadGuard = AudioThreadGuard;
    type Capture = LoopbackCapture;
    type Render = RenderStream;

    fn enter_thread(&self) -> Result<AudioThreadGuard, Error> {
        AudioThreadGuard::streaming()
    }

    fn open_capture(&self) -> Result<LoopbackCapture, Error> {
        LoopbackCapture::open_default()
    }

    fn open_render(&self, id: &DeviceId) -> Result<RenderStream, Error> {
        RenderStream::open(id)
    }
}

impl CaptureStream for LoopbackCapture {
    type Error = Error;

    fn start(&mut self) -> Result<(), Error> {
        LoopbackCapture::start(self)
    }

    fn read(&mut self, timeout_ms: u32) -> Result<&[Frame], Error> {
        LoopbackCapture::read(self, timeout_ms)
    }
}

impl RenderSink for RenderStream {
    type Error = Error;

    fn buffer_frames(&self) -> usize {
        RenderStream::buffer_frames(self)
    }

    fn start(&mut self) -> Result<(), Error> {
        RenderStream::start(self)
    }

    fn wait(&mut self, timeout_ms: u32) -> bool {
        RenderStream::wait(self, timeout_ms)
    }

    fn writable_frames(&mut self) -> Result<usize, Error> {
        RenderStream::writable_frames(self)
    }

    fn write(&mut self, frames: &[Frame]) -> Result<(), Error> {
        RenderStream::write(self, frames).map(|_| ())
    }
}

/// Device queries and mute control on Windows. Use on a thread holding COM.
#[derive(Debug, Default)]
pub(crate) struct WasapiPlatform {
    backend: Arc<WasapiBackend>,
}

impl Platform for WasapiPlatform {
    type Backend = WasapiBackend;
    type Error = Error;

    fn backend(&self) -> &Arc<WasapiBackend> {
        &self.backend
    }

    fn sample_rate(&self) -> SampleRate {
        STREAM_SAMPLE_RATE
    }

    fn default_output(&self) -> Result<Option<DeviceId>, Error> {
        default_output_id()
    }

    fn active_outputs(&self) -> Result<Vec<DeviceId>, Error> {
        Ok(list_output_devices()?.into_iter().map(|d| d.id).collect())
    }

    fn is_muted(&self, id: &DeviceId) -> Result<bool, Error> {
        is_muted(id)
    }

    fn set_muted(&self, id: &DeviceId, muted: bool) -> Result<(), Error> {
        set_muted(id, muted)
    }
}
