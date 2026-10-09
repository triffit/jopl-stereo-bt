use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use stereo_bt_core::{DeviceEvent, DeviceId};

use crate::{EngineConfig, EngineStatus, MuteJournal, Platform, Supervisor, SupervisorError};

/// Device events arrive in bursts (state change, then default change);
/// wait this long after the last one before reacting, so Windows settles.
pub(crate) const SETTLE: Duration = Duration::from_millis(300);

/// How often the status snapshot refreshes while idle.
const TICK: Duration = Duration::from_millis(100);

/// A request from the UI to the engine thread.
pub(crate) enum Command {
    Start(EngineConfig, Sender<Result<(), SupervisorError>>),
    Update(EngineConfig, Sender<Result<(), SupervisorError>>),
    Stop(Sender<Result<(), SupervisorError>>),
    MakeDefault(DeviceId, Sender<Result<(), SupervisorError>>),
    Shutdown,
}

/// What the engine thread publishes for the UI.
#[derive(Debug, Clone, Default)]
pub(crate) struct Shared {
    pub(crate) status: EngineStatus,
    /// The latest error from reacting to a device change, for display.
    pub(crate) last_error: Option<String>,
}

/// Runs the supervisor until [`Command::Shutdown`] or the command channel closes.
pub(crate) fn run<P: Platform, J: MuteJournal>(
    mut supervisor: Supervisor<P, J>,
    commands: &Receiver<Command>,
    device_events: &Receiver<DeviceEvent>,
    shared: &Arc<Mutex<Shared>>,
) {
    let mut last_device_event: Option<Instant> = None;
    loop {
        match commands.recv_timeout(TICK) {
            Ok(Command::Start(config, reply)) => {
                // The UI may have stopped waiting; a lost reply is harmless.
                let _ = reply.send(supervisor.start(config));
            }
            Ok(Command::Update(config, reply)) => {
                let _ = reply.send(supervisor.update(config));
            }
            Ok(Command::Stop(reply)) => {
                let _ = reply.send(supervisor.stop());
            }
            Ok(Command::MakeDefault(id, reply)) => {
                let _ = reply.send(supervisor.make_default(&id));
            }
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }

        if device_events.try_iter().count() > 0 {
            last_device_event = Some(Instant::now());
        }
        let mut error = supervisor
            .maintain(Instant::now())
            .err()
            .map(|e| e.to_string());
        if last_device_event.is_some_and(|at| at.elapsed() >= SETTLE) {
            last_device_event = None;
            if let Err(e) = supervisor.devices_changed() {
                error = Some(e.to_string());
            }
        }

        let mut shared = shared.lock().unwrap_or_else(PoisonError::into_inner);
        shared.status = supervisor.status();
        if error.is_some() {
            shared.last_error = error;
        }
    }
    // Dropping the supervisor stops streaming and restores mutes.
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;

    use stereo_bt_core::DeviceId;

    use super::*;
    use crate::OutputConfig;
    use crate::test_fakes::{FakeJournal, FakePlatform, SharedWorld, World, lock};

    fn id(raw: &str) -> DeviceId {
        DeviceId::new(raw).unwrap()
    }

    type Reply = Sender<Result<(), SupervisorError>>;

    struct Harness {
        world: SharedWorld,
        commands: Sender<Command>,
        events: Sender<DeviceEvent>,
        shared: Arc<Mutex<Shared>>,
        thread: thread::JoinHandle<()>,
    }

    impl Harness {
        fn spawn() -> Self {
            let world: SharedWorld = Arc::new(Mutex::new(World {
                default: Some(id("A")),
                active: vec![id("A"), id("B"), id("C")],
                ..World::default()
            }));
            let journal = FakeJournal(Arc::new(Mutex::new(Vec::new())));
            let supervisor = Supervisor::new(FakePlatform::new(&world), journal).unwrap();
            let (commands, command_rx) = mpsc::channel();
            let (events, event_rx) = mpsc::channel();
            let shared = Arc::new(Mutex::new(Shared::default()));
            let thread_shared = Arc::clone(&shared);
            let thread =
                thread::spawn(move || run(supervisor, &command_rx, &event_rx, &thread_shared));
            Self {
                world,
                commands,
                events,
                shared,
                thread,
            }
        }

        fn request(&self, make: impl FnOnce(Reply) -> Command) {
            let (reply, reply_rx) = mpsc::channel();
            self.commands.send(make(reply)).unwrap();
            reply_rx.recv().unwrap().unwrap();
        }

        fn wait_for(&self, condition: impl Fn(&Shared) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !condition(&lock(&self.shared)) {
                assert!(Instant::now() < deadline, "condition not reached");
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn shutdown(self) {
            self.commands.send(Command::Shutdown).unwrap();
            self.thread.join().unwrap();
        }
    }

    fn config() -> EngineConfig {
        EngineConfig {
            outputs: vec![OutputConfig::new(id("B")), OutputConfig::new(id("C"))],
            include_default: false,
            follow_system_volume: true,
        }
    }

    #[test]
    fn start_publishes_running_status() {
        let h = Harness::spawn();

        h.request(|reply| Command::Start(config(), reply));

        h.wait_for(|s| s.status.running && s.status.outputs.len() == 2);
        h.shutdown();
    }

    #[test]
    fn f6_device_event_is_applied_after_settling() {
        let h = Harness::spawn();
        h.request(|reply| Command::Start(config(), reply));
        lock(&h.world).active.retain(|d| d != &id("C"));
        h.request(Command::Stop);
        h.request(|reply| Command::Start(config(), reply));
        lock(&h.world).active.push(id("C"));

        let sent = Instant::now();
        h.events.send(DeviceEvent::Activated(id("C"))).unwrap();

        h.wait_for(|s| s.status.outputs.len() == 2);
        assert!(sent.elapsed() >= SETTLE);
        h.shutdown();
    }

    #[test]
    fn shutdown_restores_mutes() {
        let h = Harness::spawn();
        h.request(|reply| Command::Start(config(), reply));
        assert!(lock(&h.world).muted.contains(&id("A")));
        let world = Arc::clone(&h.world);

        h.shutdown();

        assert!(lock(&world).muted.is_empty());
    }
}
