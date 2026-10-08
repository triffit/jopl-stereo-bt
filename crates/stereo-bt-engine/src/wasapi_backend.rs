use stereo_bt_core::{DeviceId, Frame};
use stereo_bt_wasapi::{ComGuard, Error, LoopbackCapture, RenderStream};

use crate::{Backend, CaptureStream, RenderSink};

/// The Windows audio backend: loopback capture of the default output device
/// and shared-mode render streams.
#[derive(Debug, Default, Clone, Copy)]
pub struct WasapiBackend;

impl Backend for WasapiBackend {
    type Error = Error;
    type ThreadGuard = ComGuard;
    type Capture = LoopbackCapture;
    type Render = RenderStream;

    fn enter_thread(&self) -> Result<ComGuard, Error> {
        ComGuard::init_mta()
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
