use std::io;

use stereo_bt_core::DeviceId;

use crate::{
    EngineConfig, MuteJournal, OutputConfig, OutputStatus, Platform, Session, StreamState,
};

/// Errors from supervising the routing.
#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    /// A device query or control failed.
    #[error("audio device error: {0}")]
    Platform(Box<dyn std::error::Error + Send + Sync>),
    /// The mute journal could not be read or written.
    #[error("mute journal error: {0}")]
    Journal(#[from] io::Error),
}

/// Snapshot of the engine for the UI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineStatus {
    /// Whether the user has started streaming.
    pub running: bool,
    /// The capture source (default device) of the current session.
    pub source: Option<DeviceId>,
    /// Devices this app has muted.
    pub muted: Vec<DeviceId>,
    /// Capture stream state, when a session runs.
    pub capture: Option<StreamState>,
    /// Per-output health of the current session.
    pub outputs: Vec<OutputStatus>,
    /// Sessions started so far (each device-set or source change starts one).
    pub session_starts: u64,
}

struct ActiveSession {
    session: Session,
    source: DeviceId,
    outputs: Vec<DeviceId>,
}

/// Keeps the running session, the source device and its mute in line with the
/// user's configuration and the devices actually present (PRODUCT.md F5, F6).
///
/// Every change funnels into one `reconcile` step that derives the desired
/// state and applies only the differences, so device events, configuration
/// changes and start/stop all behave consistently. Single-threaded: owned by
/// the engine thread.
pub struct Supervisor<P: Platform, J: MuteJournal> {
    platform: P,
    journal: J,
    config: EngineConfig,
    running: bool,
    active: Option<ActiveSession>,
    muted_by_us: Vec<DeviceId>,
    session_starts: u64,
}

impl<P: Platform, J: MuteJournal> Supervisor<P, J> {
    /// Creates a stopped supervisor and restores any mutes a previous run
    /// left behind (e.g. after a crash).
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] when the journal cannot be read.
    pub fn new(platform: P, journal: J) -> Result<Self, SupervisorError> {
        let muted_by_us = journal.load()?;
        let mut supervisor = Self {
            platform,
            journal,
            config: EngineConfig::default(),
            running: false,
            active: None,
            muted_by_us,
            session_starts: 0,
        };
        supervisor.reconcile()?;
        Ok(supervisor)
    }

    /// Starts streaming with `config`.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] when devices cannot be queried or controlled.
    pub fn start(&mut self, config: EngineConfig) -> Result<(), SupervisorError> {
        self.config = config;
        self.running = true;
        self.reconcile()
    }

    /// Stops streaming and restores muted devices.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] when the journal cannot be written.
    pub fn stop(&mut self) -> Result<(), SupervisorError> {
        self.running = false;
        self.reconcile()
    }

    /// Applies a changed configuration. Delay, volume and channel mode change
    /// live; adding or removing devices restarts the session.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] when devices cannot be queried or controlled.
    pub fn update(&mut self, config: EngineConfig) -> Result<(), SupervisorError> {
        if let Some(active) = &self.active {
            for output in &config.outputs {
                apply_live(&active.session, output);
            }
        }
        self.config = config;
        self.reconcile()
    }

    /// Re-evaluates after devices were added, removed or the default changed.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] when devices cannot be queried or controlled.
    pub fn devices_changed(&mut self) -> Result<(), SupervisorError> {
        self.reconcile()
    }

    /// A snapshot for the UI.
    #[must_use]
    pub fn status(&self) -> EngineStatus {
        EngineStatus {
            running: self.running,
            source: self.active.as_ref().map(|a| a.source.clone()),
            muted: self.muted_by_us.clone(),
            capture: self.active.as_ref().map(|a| a.session.capture_state()),
            outputs: self
                .active
                .as_ref()
                .map(|a| a.session.output_status())
                .unwrap_or_default(),
            session_starts: self.session_starts,
        }
    }

    fn reconcile(&mut self) -> Result<(), SupervisorError> {
        let source = if self.running {
            self.platform.default_output().map_err(platform_error)?
        } else {
            None
        };
        let mute_target = source.clone().filter(|_| !self.config.include_default);
        self.restore_mutes_except(mute_target.as_ref())?;
        if let Some(id) = &mute_target {
            self.mute(id)?;
        }
        self.reconcile_session(source)
    }

    /// Unmutes every device we muted except `keep`. A device that cannot be
    /// unmuted right now (e.g. disconnected) stays recorded and is retried on
    /// the next reconcile.
    fn restore_mutes_except(&mut self, keep: Option<&DeviceId>) -> Result<(), SupervisorError> {
        let before = self.muted_by_us.len();
        let platform = &self.platform;
        self.muted_by_us
            .retain(|id| Some(id) == keep || platform.set_muted(id, false).is_err());
        if self.muted_by_us.len() != before {
            self.journal.save(&self.muted_by_us)?;
        }
        Ok(())
    }

    fn mute(&mut self, id: &DeviceId) -> Result<(), SupervisorError> {
        if self.muted_by_us.contains(id) || self.platform.is_muted(id).map_err(platform_error)? {
            // Already muted by us, or by the user: then it is not ours to restore.
            return Ok(());
        }
        // Record first: a crash between the two steps only causes a harmless unmute.
        self.muted_by_us.push(id.clone());
        self.journal.save(&self.muted_by_us)?;
        self.platform.set_muted(id, true).map_err(platform_error)
    }

    fn reconcile_session(&mut self, source: Option<DeviceId>) -> Result<(), SupervisorError> {
        let Some(source) = source else {
            self.active = None;
            return Ok(());
        };
        let present = self.platform.active_outputs().map_err(platform_error)?;
        let desired: Vec<OutputConfig> = self
            .config
            .outputs
            .iter()
            .filter(|o| o.id != source && present.contains(&o.id))
            .cloned()
            .collect();
        if desired.is_empty() {
            self.active = None;
            return Ok(());
        }

        let restart = self.active.as_ref().is_none_or(|active| {
            let added = desired.iter().any(|o| !active.outputs.contains(&o.id));
            // A device that disappeared keeps the session running for the others;
            // only a device the user deselected forces a restart.
            let deselected = active
                .outputs
                .iter()
                .any(|id| present.contains(id) && !desired.iter().any(|o| &o.id == id));
            active.source != source || added || deselected
        });
        if restart {
            // Release the old streams before opening devices again.
            self.active = None;
            let session = Session::start(
                self.platform.backend(),
                self.platform.sample_rate(),
                &desired,
            )
            .map_err(|error| SupervisorError::Platform(Box::new(error)))?;
            self.session_starts += 1;
            self.active = Some(ActiveSession {
                session,
                source,
                outputs: desired.into_iter().map(|o| o.id).collect(),
            });
        }
        Ok(())
    }
}

impl<P: Platform, J: MuteJournal> Drop for Supervisor<P, J> {
    fn drop(&mut self) {
        self.running = false;
        // Best effort: whatever cannot be restored now stays in the journal
        // and is restored on the next start.
        let _ = self.reconcile();
    }
}

fn apply_live(session: &Session, output: &OutputConfig) {
    // Devices not in the session are handled by the restart in `reconcile`.
    let _ = session.set_delay(&output.id, output.delay);
    let _ = session.set_volume(&output.id, output.volume);
    let _ = session.set_channel_mode(&output.id, output.channel_mode);
}

fn platform_error(error: impl std::error::Error + Send + Sync + 'static) -> SupervisorError {
    SupervisorError::Platform(Box::new(error))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use stereo_bt_core::Delay;

    use super::*;
    use crate::test_fakes::{FakeJournal, FakePlatform, SharedWorld, World, lock};

    type TestSupervisor = Supervisor<FakePlatform, FakeJournal>;

    fn id(raw: &str) -> DeviceId {
        DeviceId::new(raw).unwrap()
    }

    fn world(default: &str, active: &[&str]) -> SharedWorld {
        Arc::new(Mutex::new(World {
            default: Some(id(default)),
            active: active.iter().map(|raw| id(raw)).collect(),
            ..World::default()
        }))
    }

    fn supervisor(
        world: &SharedWorld,
        journal: &[&str],
    ) -> (TestSupervisor, Arc<Mutex<Vec<DeviceId>>>) {
        let journal = Arc::new(Mutex::new(journal.iter().map(|raw| id(raw)).collect()));
        let supervisor =
            Supervisor::new(FakePlatform::new(world), FakeJournal(Arc::clone(&journal))).unwrap();
        (supervisor, journal)
    }

    fn config(outputs: &[&str], include_default: bool) -> EngineConfig {
        EngineConfig {
            outputs: outputs
                .iter()
                .map(|raw| OutputConfig::new(id(raw)))
                .collect(),
            include_default,
        }
    }

    fn output_ids(status: &EngineStatus) -> Vec<DeviceId> {
        status.outputs.iter().map(|o| o.id.clone()).collect()
    }

    #[test]
    fn f5_start_mutes_default_and_routes_every_other_selected_device() {
        let w = world("A", &["A", "B", "C"]);
        let (mut s, journal) = supervisor(&w, &[]);

        s.start(config(&["B", "A", "C"], false)).unwrap();

        let status = s.status();
        assert_eq!(output_ids(&status), vec![id("B"), id("C")]);
        assert_eq!(status.source, Some(id("A")));
        assert!(lock(&w).muted.contains(&id("A")));
        assert_eq!(*lock(&journal), vec![id("A")]);
    }

    #[test]
    fn include_default_leaves_the_default_device_playing() {
        let w = world("A", &["A", "B"]);
        let (mut s, journal) = supervisor(&w, &[]);

        s.start(config(&["B"], true)).unwrap();

        assert!(lock(&w).muted.is_empty());
        assert!(lock(&journal).is_empty());
        assert_eq!(output_ids(&s.status()), vec![id("B")]);
    }

    #[test]
    fn stop_restores_mute_and_clears_the_journal() {
        let w = world("A", &["A", "B"]);
        let (mut s, journal) = supervisor(&w, &[]);
        s.start(config(&["B"], false)).unwrap();

        s.stop().unwrap();

        assert!(lock(&w).muted.is_empty());
        assert!(lock(&journal).is_empty());
        assert!(s.status().outputs.is_empty());
    }

    #[test]
    fn device_the_user_had_muted_is_left_muted_after_stop() {
        let w = world("A", &["A", "B"]);
        lock(&w).muted.insert(id("A"));
        let (mut s, journal) = supervisor(&w, &[]);
        s.start(config(&["B"], false)).unwrap();

        s.stop().unwrap();

        assert!(lock(&w).muted.contains(&id("A")));
        assert!(lock(&journal).is_empty());
    }

    #[test]
    fn startup_restores_mutes_left_by_a_crash() {
        let w = world("A", &["A", "B"]);
        lock(&w).muted.insert(id("A"));

        let (_s, journal) = supervisor(&w, &["A"]);

        assert!(lock(&w).muted.is_empty());
        assert!(lock(&journal).is_empty());
    }

    #[test]
    fn mute_of_a_disconnected_device_is_restored_when_it_returns() {
        let w = world("A", &["A", "B"]);
        let (mut s, journal) = supervisor(&w, &[]);
        s.start(config(&["B"], false)).unwrap();
        {
            let mut world = lock(&w);
            world.active.retain(|d| d != &id("A"));
            world.default = Some(id("B"));
        }
        s.stop().unwrap();
        assert_eq!(*lock(&journal), vec![id("A")]);

        lock(&w).active.push(id("A"));
        s.devices_changed().unwrap();

        assert!(!lock(&w).muted.contains(&id("A")));
        assert!(lock(&journal).is_empty());
    }

    #[test]
    fn f6_default_change_moves_source_and_mute() {
        let w = world("A", &["A", "B", "C"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["A", "B", "C"], false)).unwrap();

        lock(&w).default = Some(id("B"));
        s.devices_changed().unwrap();

        let status = s.status();
        assert_eq!(status.source, Some(id("B")));
        assert_eq!(output_ids(&status), vec![id("A"), id("C")]);
        assert_eq!(status.muted, vec![id("B")]);
        assert!(!lock(&w).muted.contains(&id("A")));
        assert_eq!(status.session_starts, 2);
    }

    #[test]
    fn f6_removed_device_does_not_restart_the_others() {
        let w = world("A", &["A", "B", "C"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B", "C"], false)).unwrap();

        lock(&w).active.retain(|d| d != &id("C"));
        s.devices_changed().unwrap();

        assert_eq!(s.status().session_starts, 1);
    }

    #[test]
    fn f6_reconnected_device_rejoins() {
        let w = world("A", &["A", "B"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B", "C"], false)).unwrap();
        assert_eq!(output_ids(&s.status()), vec![id("B")]);

        lock(&w).active.push(id("C"));
        s.devices_changed().unwrap();

        assert_eq!(output_ids(&s.status()), vec![id("B"), id("C")]);
    }

    #[test]
    fn deselecting_a_device_restarts_without_it() {
        let w = world("A", &["A", "B", "C"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B", "C"], false)).unwrap();

        s.update(config(&["B"], false)).unwrap();

        assert_eq!(output_ids(&s.status()), vec![id("B")]);
    }

    #[test]
    fn live_setting_change_does_not_restart() {
        let w = world("A", &["A", "B"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B"], false)).unwrap();
        let mut changed = config(&["B"], false);
        changed.outputs[0].delay = Delay::from_millis(150).unwrap();

        s.update(changed).unwrap();

        assert_eq!(s.status().session_starts, 1);
    }

    #[test]
    fn toggling_include_default_unmutes_without_restart() {
        let w = world("A", &["A", "B"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B"], false)).unwrap();

        s.update(config(&["B"], true)).unwrap();

        assert!(lock(&w).muted.is_empty());
        assert_eq!(s.status().session_starts, 1);
    }

    #[test]
    fn no_default_device_means_no_session() {
        let w = world("A", &["B"]);
        lock(&w).default = None;
        let (mut s, _) = supervisor(&w, &[]);

        s.start(config(&["B"], false)).unwrap();

        assert!(s.status().outputs.is_empty());
        assert!(s.status().running);
    }

    #[test]
    fn dropping_a_running_supervisor_restores_mutes() {
        let w = world("A", &["A", "B"]);
        let (mut s, journal) = supervisor(&w, &[]);
        s.start(config(&["B"], false)).unwrap();

        drop(s);

        assert!(lock(&w).muted.is_empty());
        assert!(lock(&journal).is_empty());
    }

    #[test]
    fn session_streams_after_start() {
        let w = world("A", &["A", "B"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B"], false)).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while s.status().outputs[0].state != StreamState::Running {
            assert!(Instant::now() < deadline, "output never started");
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(s.status().capture, Some(StreamState::Running));
    }
}
