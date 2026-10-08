//! Test doubles for the backend, platform and mute journal.

use std::collections::HashSet;
use std::fmt;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use stereo_bt_core::{DeviceId, Frame, SampleRate};

use crate::{Backend, CaptureStream, MuteJournal, Platform, RenderSink};

#[derive(Debug)]
pub(crate) struct FakeError;

impl fmt::Display for FakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("fake error")
    }
}

impl std::error::Error for FakeError {}

/// Captures silence and discards rendered audio.
pub(crate) struct SilentBackend;

pub(crate) struct SilentCapture([Frame; 480]);

pub(crate) struct DiscardRender;

impl Backend for SilentBackend {
    type Error = FakeError;
    type ThreadGuard = ();
    type Capture = SilentCapture;
    type Render = DiscardRender;

    fn enter_thread(&self) -> Result<(), FakeError> {
        Ok(())
    }

    fn open_capture(&self) -> Result<SilentCapture, FakeError> {
        Ok(SilentCapture([Frame::SILENCE; 480]))
    }

    fn open_render(&self, _id: &DeviceId) -> Result<DiscardRender, FakeError> {
        Ok(DiscardRender)
    }
}

impl CaptureStream for SilentCapture {
    type Error = FakeError;

    fn start(&mut self) -> Result<(), FakeError> {
        Ok(())
    }

    fn read(&mut self, _timeout_ms: u32) -> Result<&[Frame], FakeError> {
        thread::sleep(Duration::from_millis(5));
        Ok(&self.0)
    }
}

impl RenderSink for DiscardRender {
    type Error = FakeError;

    fn buffer_frames(&self) -> usize {
        480
    }

    fn start(&mut self) -> Result<(), FakeError> {
        Ok(())
    }

    fn wait(&mut self, _timeout_ms: u32) -> bool {
        thread::sleep(Duration::from_millis(5));
        true
    }

    fn writable_frames(&mut self) -> Result<usize, FakeError> {
        Ok(480)
    }

    fn write(&mut self, _frames: &[Frame]) -> Result<(), FakeError> {
        Ok(())
    }
}

/// The simulated Windows device world, shared with the test.
#[derive(Debug, Default)]
pub(crate) struct World {
    pub(crate) default: Option<DeviceId>,
    pub(crate) active: Vec<DeviceId>,
    pub(crate) muted: HashSet<DeviceId>,
}

/// A world shared between the test and the platform (possibly on another thread).
pub(crate) type SharedWorld = Arc<Mutex<World>>;

/// Locks a shared fake, ignoring poisoning from an earlier failed assertion.
pub(crate) fn lock<T>(shared: &Mutex<T>) -> MutexGuard<'_, T> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) struct FakePlatform {
    backend: Arc<SilentBackend>,
    world: SharedWorld,
}

impl FakePlatform {
    pub(crate) fn new(world: &SharedWorld) -> Self {
        Self {
            backend: Arc::new(SilentBackend),
            world: Arc::clone(world),
        }
    }
}

impl Platform for FakePlatform {
    type Backend = SilentBackend;
    type Error = FakeError;

    fn backend(&self) -> &Arc<SilentBackend> {
        &self.backend
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::HZ_48000
    }

    fn default_output(&self) -> Result<Option<DeviceId>, FakeError> {
        Ok(lock(&self.world).default.clone())
    }

    fn active_outputs(&self) -> Result<Vec<DeviceId>, FakeError> {
        Ok(lock(&self.world).active.clone())
    }

    fn is_muted(&self, id: &DeviceId) -> Result<bool, FakeError> {
        Ok(lock(&self.world).muted.contains(id))
    }

    fn set_muted(&self, id: &DeviceId, muted: bool) -> Result<(), FakeError> {
        let mut world = lock(&self.world);
        if !world.active.contains(id) {
            return Err(FakeError);
        }
        if muted {
            world.muted.insert(id.clone());
        } else {
            world.muted.remove(id);
        }
        Ok(())
    }
}

/// In-memory journal, shared with the test.
pub(crate) struct FakeJournal(pub(crate) Arc<Mutex<Vec<DeviceId>>>);

impl MuteJournal for FakeJournal {
    fn load(&self) -> io::Result<Vec<DeviceId>> {
        Ok(lock(&self.0).clone())
    }

    fn save(&self, muted: &[DeviceId]) -> io::Result<()> {
        *lock(&self.0) = muted.to_vec();
        Ok(())
    }
}
