use std::any::Any;
use std::io;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use stereo_bt_core::{DeviceEvent, DeviceId};

use crate::engine_loop::{self, Command, Shared};
use crate::{EngineConfig, EngineStatus, MuteJournal, Platform, Supervisor, SupervisorError};

/// What the engine-thread setup returns: values to keep alive for the
/// thread's lifetime (dropped after the supervisor), and the supervisor.
pub(crate) type Setup<P, J> = Result<(Box<dyn Any>, Supervisor<P, J>), SupervisorError>;

/// How long a request waits for the engine thread before giving up.
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// Errors from controlling the [`Engine`].
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The request reached the engine but failed.
    #[error(transparent)]
    Supervisor(#[from] SupervisorError),
    /// The engine thread could not be started.
    #[error("could not start the audio engine thread: {0}")]
    Spawn(#[from] io::Error),
    /// The engine thread has exited.
    #[error("the audio engine has stopped")]
    Stopped,
    /// The engine thread did not answer in time.
    #[error("the audio engine did not respond in time")]
    Timeout,
}

/// The audio engine running on its own thread; the UI's single entry point.
///
/// All methods are cheap and non-blocking except the request/response ones
/// (`start`, `update`, `stop`), which wait for the engine thread to apply them.
/// Dropping the engine stops streaming and restores muted devices.
pub struct Engine {
    commands: Sender<Command>,
    shared: Arc<Mutex<Shared>>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Starts the Windows engine. Mutes this app applies are recorded in
    /// `journal` and restored on the next start after a crash.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the engine thread or the Windows audio
    /// system cannot be initialised.
    #[cfg(windows)]
    pub fn spawn(journal: crate::FileMuteJournal) -> Result<Self, EngineError> {
        use crate::wasapi_backend::{AudioThreadGuard, WasapiPlatform};
        use stereo_bt_wasapi::DeviceWatcher;

        Self::spawn_with(move |device_events| {
            let guard = AudioThreadGuard::control().map_err(boxed)?;
            let watcher = DeviceWatcher::start(device_events).map_err(boxed)?;
            let supervisor = Supervisor::new(WasapiPlatform::default(), journal)?;
            // Dropped after the supervisor: watcher first, then COM.
            let keep_alive: Box<dyn Any> = Box::new((guard, watcher));
            Ok((keep_alive, supervisor))
        })
    }

    /// Starts an engine whose platform is built on the engine thread by `setup`,
    /// which receives the sender for device events.
    pub(crate) fn spawn_with<P, J, F>(setup: F) -> Result<Self, EngineError>
    where
        P: Platform,
        J: MuteJournal,
        F: FnOnce(&Sender<DeviceEvent>) -> Setup<P, J> + Send + 'static,
    {
        let (commands, command_rx) = mpsc::channel();
        let (ready, ready_rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let thread_shared = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("engine".into())
            .spawn(move || {
                let (event_tx, event_rx) = mpsc::channel();
                match setup(&event_tx) {
                    Ok((keep_alive, supervisor)) => {
                        let _ = ready.send(Ok(()));
                        engine_loop::run(supervisor, &command_rx, &event_rx, &thread_shared);
                        drop(keep_alive);
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                    }
                }
            })?;
        let engine = Self {
            commands,
            shared,
            thread: Some(thread),
        };
        match ready_rx.recv_timeout(REPLY_TIMEOUT) {
            Ok(Ok(())) => Ok(engine),
            Ok(Err(error)) => Err(error.into()),
            Err(RecvTimeoutError::Timeout) => Err(EngineError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(EngineError::Stopped),
        }
    }

    /// Starts streaming with `config`.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the configuration cannot be applied.
    pub fn start(&self, config: EngineConfig) -> Result<(), EngineError> {
        self.request(|reply| Command::Start(config, reply))
    }

    /// Applies a changed configuration while streaming (or stores it while stopped).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the configuration cannot be applied.
    pub fn update(&self, config: EngineConfig) -> Result<(), EngineError> {
        self.request(|reply| Command::Update(config, reply))
    }

    /// Stops streaming and restores muted devices.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the engine does not respond.
    pub fn stop(&self) -> Result<(), EngineError> {
        self.request(Command::Stop)
    }

    /// Makes `id` the Windows default output device (the audio source).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when Windows refuses or the engine does not respond.
    pub fn make_default(&self, id: DeviceId) -> Result<(), EngineError> {
        self.request(|reply| Command::MakeDefault(id, reply))
    }

    /// Starts or stops the assisted-sync click track on the default device.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the click track cannot be started.
    pub fn set_click_track(&self, on: bool) -> Result<(), EngineError> {
        self.request(|reply| Command::ClickTrack(on, reply))
    }

    /// Leaves only `device` audible for assisted sync (`None`: all again).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when devices cannot be controlled.
    pub fn isolate(&self, device: Option<DeviceId>) -> Result<(), EngineError> {
        self.request(|reply| Command::Isolate(device, reply))
    }

    /// The latest status snapshot (refreshed about every 100 ms).
    #[must_use]
    pub fn status(&self) -> EngineStatus {
        self.lock_shared().status.clone()
    }

    /// The latest error from reacting to a device change, if any.
    #[must_use]
    pub fn last_error(&self) -> Option<String> {
        self.lock_shared().last_error.clone()
    }

    fn request(
        &self,
        make: impl FnOnce(Sender<Result<(), SupervisorError>>) -> Command,
    ) -> Result<(), EngineError> {
        let (reply, reply_rx) = mpsc::channel();
        self.commands
            .send(make(reply))
            .map_err(|_| EngineError::Stopped)?;
        match reply_rx.recv_timeout(REPLY_TIMEOUT) {
            Ok(result) => Ok(result?),
            Err(RecvTimeoutError::Timeout) => Err(EngineError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(EngineError::Stopped),
        }
    }

    fn lock_shared(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // The thread may already have exited; then there is nothing to stop.
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(windows)]
fn boxed(error: stereo_bt_wasapi::Error) -> SupervisorError {
    SupervisorError::Platform(Box::new(error))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use stereo_bt_core::DeviceId;

    use super::*;
    use crate::OutputConfig;
    use crate::test_fakes::{FakeJournal, FakePlatform, SharedWorld, World, lock};

    fn id(raw: &str) -> DeviceId {
        DeviceId::new(raw).unwrap()
    }

    fn engine(world: &SharedWorld) -> Engine {
        let world = Arc::clone(world);
        Engine::spawn_with(move |_events| {
            let journal = FakeJournal(Arc::new(Mutex::new(Vec::new())));
            let supervisor = Supervisor::new(FakePlatform::new(&world), journal)?;
            Ok((Box::new(()), supervisor))
        })
        .unwrap()
    }

    fn world() -> SharedWorld {
        Arc::new(Mutex::new(World {
            default: Some(id("A")),
            active: vec![id("A"), id("B")],
            ..World::default()
        }))
    }

    fn config() -> EngineConfig {
        EngineConfig {
            outputs: vec![OutputConfig::new(id("B"))],
            include_default: false,
            follow_system_volume: true,
        }
    }

    #[test]
    fn start_and_stop_round_trip_through_the_engine_thread() {
        let w = world();
        let engine = engine(&w);

        engine.start(config()).unwrap();
        assert!(lock(&w).muted.contains(&id("A")));
        engine.stop().unwrap();

        assert!(lock(&w).muted.is_empty());
    }

    #[test]
    fn failing_setup_is_reported_by_spawn() {
        let result = Engine::spawn_with(|_events| -> Setup<FakePlatform, FakeJournal> {
            Err(SupervisorError::Journal(io::Error::other("unreadable")))
        });

        assert!(matches!(result, Err(EngineError::Supervisor(_))));
    }

    #[test]
    fn make_default_round_trips_through_the_engine_thread() {
        let w = world();
        let engine = engine(&w);

        engine.make_default(id("B")).unwrap();

        assert_eq!(lock(&w).default, Some(id("B")));
    }

    #[test]
    fn dropping_the_engine_restores_mutes() {
        let w = world();
        let engine = engine(&w);
        engine.start(config()).unwrap();

        drop(engine);

        assert!(lock(&w).muted.is_empty());
    }
}
