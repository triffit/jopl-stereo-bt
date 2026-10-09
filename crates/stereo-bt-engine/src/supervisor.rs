use std::io;
use std::time::{Duration, Instant};

use stereo_bt_core::{DeviceId, DeviceInfo, MutedApp, SystemVolume};

use crate::click_track::{ClickTrack, TestSignal};
use crate::{
    Backend, EngineConfig, OutputConfig, OutputStatus, Platform, RestoreJournal, Session,
    StreamState,
};

/// How often the default device's volume mixer is checked for muted apps.
const MUTED_APP_CHECK: Duration = Duration::from_secs(1);
/// How often battery levels are read; they change slowly.
const BATTERY_CHECK: Duration = Duration::from_secs(60);

/// Volumes at or above this slider position count as full.
/// A Windows volume scalar at or above this counts as full.
const FULL_VOLUME: f32 = 0.999;

/// Time between clicks of the assisted-sync click track.
pub(crate) const CLICK_PERIOD_MILLIS: u32 = 1_000;
/// Time between chirps when measuring with a microphone.
pub(crate) const CHIRP_PERIOD_MILLIS: u32 = 700;

/// How long a failed device or capture stream waits before it is retried.
/// Bluetooth devices often need a moment after a dropout or profile switch.
pub(crate) const RETRY_DELAY: Duration = Duration::from_secs(3);

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
    /// Why the capture stream failed, if it did.
    pub capture_error: Option<String>,
    /// Per-output health of the current session.
    pub outputs: Vec<OutputStatus>,
    /// Sessions started so far (each device-set or source change starts one).
    pub session_starts: u64,
    /// Active output devices, refreshed on every change (also while stopped).
    pub devices: Vec<DeviceInfo>,
    /// The followed Windows volume in percent, while following it.
    pub system_volume_percent: Option<u8>,
    /// Whether the assisted-sync click track is playing.
    pub click_track: bool,
    /// When recent clicks left the default device, oldest first.
    pub click_times: Vec<Instant>,
    /// The only audible device while assisted sync isolates one.
    pub isolated: Option<DeviceId>,
    /// Apps playing on the default device while muted for it in the Windows
    /// volume mixer: their audio reaches no device at all.
    pub muted_apps: Vec<MutedApp>,
    /// Battery level in percent of devices that report one (also while stopped).
    pub batteries: Vec<(DeviceId, u8)>,
}

struct ActiveSession<B: Backend> {
    session: Session<B>,
    source: DeviceId,
    /// When each currently failed output was first seen failing.
    failed_since: Vec<(DeviceId, Instant)>,
    /// When the capture stream was first seen failing.
    capture_failed_since: Option<Instant>,
}

impl<B: Backend> ActiveSession<B> {
    fn new(session: Session<B>, source: DeviceId) -> Self {
        Self {
            session,
            source,
            failed_since: Vec::new(),
            capture_failed_since: None,
        }
    }
}

/// Keeps the running session, the source device and its mute in line with the
/// user's configuration and the devices actually present (PRODUCT.md F5, F6).
///
/// Every change funnels into one `reconcile` step that derives the desired
/// state and applies only the differences, so device events, configuration
/// changes and start/stop all behave consistently. Adding or removing a
/// device never disturbs the others; only a new source restarts everything.
/// [`Supervisor::maintain`] retries failed streams. Single-threaded: owned by
/// the engine thread.
pub struct Supervisor<P: Platform, J: RestoreJournal> {
    platform: P,
    journal: J,
    config: EngineConfig,
    running: bool,
    active: Option<ActiveSession<P::Backend>>,
    muted_by_us: Vec<DeviceId>,
    /// Target devices we raised to full volume, with their original volume.
    boosted: Vec<(DeviceId, f32)>,
    session_starts: u64,
    devices: Vec<DeviceInfo>,
    system_volume: Option<SystemVolume>,
    test_signal: Option<TestSignal>,
    click_track: Option<ClickTrack>,
    isolated: Option<DeviceId>,
    muted_apps: Vec<MutedApp>,
    muted_apps_checked: Option<Instant>,
    batteries: Vec<(DeviceId, u8)>,
    batteries_checked: Option<Instant>,
}

impl<P: Platform, J: RestoreJournal> Supervisor<P, J> {
    /// Creates a stopped supervisor and restores any mutes a previous run
    /// left behind (e.g. after a crash).
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] when the journal cannot be read.
    pub fn new(platform: P, journal: J) -> Result<Self, SupervisorError> {
        let muted_by_us = journal.load_mutes()?;
        let boosted = journal.load_volumes()?;
        let mut supervisor = Self {
            platform,
            journal,
            config: EngineConfig::default(),
            running: false,
            active: None,
            muted_by_us,
            boosted,
            session_starts: 0,
            devices: Vec::new(),
            system_volume: None,
            test_signal: None,
            click_track: None,
            isolated: None,
            muted_apps: Vec::new(),
            muted_apps_checked: None,
            batteries: Vec::new(),
            batteries_checked: None,
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
        self.test_signal = None;
        self.isolated = None;
        self.reconcile()
    }

    /// Starts or stops the assisted-sync click track on the default device.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] when the click thread cannot be started.
    pub fn set_click_track(&mut self, on: bool) -> Result<(), SupervisorError> {
        self.set_test_signal(on.then_some(TestSignal::Click))
    }

    /// Plays `signal` on the default device (`None`: stop).
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] when the test-signal thread cannot be started.
    pub fn set_test_signal(&mut self, signal: Option<TestSignal>) -> Result<(), SupervisorError> {
        self.test_signal = signal;
        self.reconcile_click_track()
    }

    /// Leaves only `device` audible for assisted sync (`None`: all again).
    /// Isolating another device than the default also mutes the default
    /// device for the duration (restored like any other mute).
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] when devices cannot be queried or controlled.
    pub fn isolate(&mut self, device: Option<DeviceId>) -> Result<(), SupervisorError> {
        self.isolated = device;
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

    /// Unmutes process `pid` on the default device in the volume mixer.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] when the device cannot be changed.
    pub fn unmute_app(&mut self, pid: u32) -> Result<(), SupervisorError> {
        if let Some(default) = self.default_device() {
            self.platform
                .unmute_app(&default, pid)
                .map_err(platform_error)?;
        }
        self.refresh_muted_apps();
        Ok(())
    }

    fn default_device(&self) -> Option<DeviceId> {
        self.devices
            .iter()
            .find(|d| d.is_default)
            .map(|d| d.id.clone())
    }

    /// Re-reads battery levels; a failed query keeps the last known levels.
    fn refresh_batteries(&mut self) {
        let known: Vec<DeviceId> = self.devices.iter().map(|d| d.id.clone()).collect();
        if let Ok(levels) = self.platform.battery_levels(&known) {
            self.batteries = levels;
        }
    }

    /// Re-reads which apps are muted on the default device (also while stopped).
    fn refresh_muted_apps(&mut self) {
        self.muted_apps = self
            .default_device()
            // A failed query (e.g. the device is switching) shows no warning.
            .and_then(|id| self.platform.muted_apps(&id).ok())
            .unwrap_or_default();
    }

    /// Plays a short double beep on `id`, if it is playing (F18).
    pub fn ping(&self, id: &DeviceId) {
        if let Some(active) = &self.active {
            let _ = active.session.ping(id);
        }
    }

    /// Makes `id` the Windows default device (the new audio source) and
    /// switches streaming over to it.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] when Windows refuses or devices cannot be
    /// queried or controlled.
    pub fn make_default(&mut self, id: &DeviceId) -> Result<(), SupervisorError> {
        self.platform
            .set_default_output(id)
            .map_err(platform_error)?;
        // Windows also sends a device event, but switching now avoids the delay.
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

    /// Periodic upkeep (call every ~100 ms): restarts outputs that failed
    /// while their device is present, and the whole session when capture
    /// failed, each after [`RETRY_DELAY`].
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] when devices cannot be queried or controlled.
    pub fn maintain(&mut self, now: Instant) -> Result<(), SupervisorError> {
        self.follow_system_volume();
        if self
            .muted_apps_checked
            .is_none_or(|at| now.duration_since(at) >= MUTED_APP_CHECK)
        {
            self.muted_apps_checked = Some(now);
            self.refresh_muted_apps();
        }
        if self
            .batteries_checked
            .is_none_or(|at| now.duration_since(at) >= BATTERY_CHECK)
        {
            self.batteries_checked = Some(now);
            self.refresh_batteries();
        }
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        if active.session.capture_state() == StreamState::Failed {
            let since = *active.capture_failed_since.get_or_insert(now);
            if now.duration_since(since) >= RETRY_DELAY {
                self.active = None;
                return self.reconcile();
            }
            return Ok(());
        }

        let failed: Vec<DeviceId> = active
            .session
            .output_status()
            .into_iter()
            .filter(|o| o.state == StreamState::Failed)
            .map(|o| o.id)
            .collect();
        active.failed_since.retain(|(id, _)| failed.contains(id));
        for id in failed {
            let Some(&(_, since)) = active.failed_since.iter().find(|(known, _)| known == &id)
            else {
                active.failed_since.push((id, now));
                continue;
            };
            if now.duration_since(since) < RETRY_DELAY {
                continue;
            }
            active.failed_since.retain(|(known, _)| known != &id);
            if let Some(config) = self.config.outputs.iter().find(|o| o.id == id) {
                active.session.add_output(config).map_err(spawn_error)?;
            }
        }
        Ok(())
    }

    /// Applies the default device's Windows volume to every output, or unity
    /// gain when not following it.
    fn follow_system_volume(&mut self) {
        let Some(active) = self.active.as_mut() else {
            self.system_volume = None;
            return;
        };
        if !self.config.follow_system_volume {
            self.system_volume = None;
            active.session.set_master_gain(1.0);
            return;
        }
        // A failed read (e.g. the device is switching) keeps the last gain.
        let Ok(mut volume) = self.platform.system_volume(&active.source) else {
            return;
        };
        if self.muted_by_us.contains(&active.source) {
            // Our own mute silences only the source, not the followers.
            volume = volume.unmuted();
        }
        self.system_volume = Some(volume);
        active.session.set_master_gain(volume.amplitude());
    }

    /// A snapshot for the UI.
    #[must_use]
    pub fn status(&self) -> EngineStatus {
        EngineStatus {
            running: self.running,
            source: self.active.as_ref().map(|a| a.source.clone()),
            muted: self.muted_by_us.clone(),
            capture: self.active.as_ref().map(|a| a.session.capture_state()),
            capture_error: self.active.as_ref().and_then(|a| a.session.capture_error()),
            outputs: self
                .active
                .as_ref()
                .map(|a| a.session.output_status())
                .unwrap_or_default(),
            session_starts: self.session_starts,
            devices: self.devices.clone(),
            system_volume_percent: self.system_volume.map(SystemVolume::percent),
            click_track: self.click_track.is_some(),
            click_times: self
                .click_track
                .as_ref()
                .map(ClickTrack::clicks)
                .unwrap_or_default(),
            isolated: self.isolated.clone(),
            muted_apps: self.muted_apps.clone(),
            batteries: self.batteries.clone(),
        }
    }

    fn reconcile(&mut self) -> Result<(), SupervisorError> {
        // Fail safe: when devices cannot be queried, stop streaming and
        // restore mutes, then report the error.
        let query = self.platform.outputs().map_err(platform_error);
        if let Ok(devices) = &query {
            self.devices.clone_from(devices);
        }
        let source = match &query {
            Ok(devices) if self.running => {
                devices.iter().find(|d| d.is_default).map(|d| d.id.clone())
            }
            _ => None,
        };
        let isolating_other = self
            .isolated
            .as_ref()
            .is_some_and(|isolated| Some(isolated) != source.as_ref());
        let mute_target = source
            .clone()
            .filter(|_| !self.config.include_default || isolating_other);
        self.restore_mutes_except(mute_target.as_ref())?;
        if let Some(id) = &mute_target {
            self.mute(id)?;
        }
        self.reconcile_session(source)?;
        if let Some(active) = self.active.as_mut() {
            active.session.isolate(self.isolated.as_ref());
        }
        self.reconcile_click_track()?;
        self.reconcile_volumes()?;
        query.map(|_| ())
    }

    /// While streaming and following the system volume, every target device
    /// plays at full Windows volume, so the system volume (applied by us) is
    /// the one control for all devices. Original volumes are journaled first
    /// and restored when no longer needed; a device that cannot be restored
    /// right now (disconnected) is retried on the next reconcile.
    fn reconcile_volumes(&mut self) -> Result<(), SupervisorError> {
        let wanted: Vec<DeviceId> = match &self.active {
            Some(active) if self.running && self.config.follow_system_volume => {
                active.session.output_ids()
            }
            _ => Vec::new(),
        };
        let before = self.boosted.len();
        let platform = &self.platform;
        self.boosted.retain(|(id, original)| {
            wanted.contains(id) || platform.set_volume(id, *original).is_err()
        });
        if self.boosted.len() != before {
            self.journal.save_volumes(&self.boosted)?;
        }
        for id in wanted {
            if self.boosted.iter().any(|(boosted, _)| boosted == &id) {
                continue;
            }
            // A device that cannot be queried right now is tried again later.
            let Ok(volume) = self.platform.system_volume(&id) else {
                continue;
            };
            if volume.scalar() >= FULL_VOLUME {
                continue;
            }
            // Record first: a crash between the two steps only causes a harmless restore.
            self.boosted.push((id.clone(), volume.scalar()));
            self.journal.save_volumes(&self.boosted)?;
            self.platform.set_volume(&id, 1.0).map_err(platform_error)?;
        }
        Ok(())
    }

    /// Keeps the click track playing on the current source while wanted.
    fn reconcile_click_track(&mut self) -> Result<(), SupervisorError> {
        let wanted = self
            .active
            .as_ref()
            .zip(self.test_signal)
            .map(|(a, signal)| (a.source.clone(), signal));
        match wanted {
            None => self.click_track = None,
            Some((source, signal)) => {
                let current = self
                    .click_track
                    .as_ref()
                    .map(|t| (t.device().clone(), t.signal()));
                if current != Some((source.clone(), signal)) {
                    self.click_track = None;
                    let period = match signal {
                        TestSignal::Click => CLICK_PERIOD_MILLIS,
                        TestSignal::Chirp => CHIRP_PERIOD_MILLIS,
                    };
                    let track = ClickTrack::start(
                        self.platform.backend(),
                        source,
                        self.platform.sample_rate(),
                        signal,
                        period,
                    )
                    .map_err(spawn_error)?;
                    self.click_track = Some(track);
                }
            }
        }
        Ok(())
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
            self.journal.save_mutes(&self.muted_by_us)?;
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
        self.journal.save_mutes(&self.muted_by_us)?;
        self.platform.set_muted(id, true).map_err(platform_error)
    }

    fn reconcile_session(&mut self, source: Option<DeviceId>) -> Result<(), SupervisorError> {
        let Some(source) = source else {
            self.active = None;
            return Ok(());
        };
        let present = |id: &DeviceId| self.devices.iter().any(|d| &d.id == id);
        let desired: Vec<OutputConfig> = self
            .config
            .outputs
            .iter()
            .filter(|o| o.id != source && present(&o.id))
            .cloned()
            .collect();
        if desired.is_empty() {
            self.active = None;
            return Ok(());
        }

        let Some(active) = self.active.as_mut().filter(|a| a.source == source) else {
            // New source: release the old streams before opening devices again.
            self.active = None;
            let mut session = Session::start(
                self.platform.backend(),
                self.platform.sample_rate(),
                &desired,
            )
            .map_err(spawn_error)?;
            session.set_space(self.config.space);
            self.session_starts += 1;
            self.active = Some(ActiveSession::new(session, source));
            return Ok(());
        };

        // Same source: change only the devices that differ, so the others
        // keep playing undisturbed.
        for id in active.session.output_ids() {
            if !desired.iter().any(|o| o.id == id) {
                active.session.remove_output(&id);
            }
        }
        let current = active.session.output_ids();
        for config in desired.iter().filter(|o| !current.contains(&o.id)) {
            active.session.add_output(config).map_err(spawn_error)?;
        }
        active.session.set_space(self.config.space);
        Ok(())
    }
}

impl<P: Platform, J: RestoreJournal> Drop for Supervisor<P, J> {
    fn drop(&mut self) {
        self.running = false;
        // Best effort: whatever cannot be restored now stays in the journal
        // and is restored on the next start.
        let _ = self.reconcile();
    }
}

fn apply_live<B: Backend>(session: &Session<B>, output: &OutputConfig) {
    // Devices not in the session are added by `reconcile`.
    let _ = session.set_delay(&output.id, output.delay);
    let _ = session.set_volume(&output.id, output.volume);
    let _ = session.set_channel_mode(&output.id, output.channel_mode);
    let _ = session.set_bass_cut(&output.id, output.bass_cut);
    let _ = session.set_virtual_bass(&output.id, output.virtual_bass);
    let _ = session.set_position(&output.id, output.position);
}

fn platform_error(error: impl std::error::Error + Send + Sync + 'static) -> SupervisorError {
    SupervisorError::Platform(Box::new(error))
}

fn spawn_error(error: io::Error) -> SupervisorError {
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
        let supervisor = Supervisor::new(
            FakePlatform::new(world),
            FakeJournal(Arc::clone(&journal), Arc::default()),
        )
        .unwrap();
        (supervisor, journal)
    }

    fn config(outputs: &[&str], include_default: bool) -> EngineConfig {
        EngineConfig {
            outputs: outputs
                .iter()
                .map(|raw| OutputConfig::new(id(raw)))
                .collect(),
            include_default,
            follow_system_volume: true,
            ..EngineConfig::default()
        }
    }

    fn master_gain(s: &TestSupervisor) -> f32 {
        s.active.as_ref().unwrap().session.master_gain()
    }

    #[test]
    fn make_default_switches_the_source_and_streams_to_the_old_default() {
        let w = world("A", &["A", "B"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["A", "B"], true)).unwrap();
        assert_eq!(s.status().source, Some(id("A")));

        s.make_default(&id("B")).unwrap();

        let status = s.status();
        assert_eq!(lock(&w).default, Some(id("B")));
        assert_eq!(status.source, Some(id("B")));
        assert_eq!(output_ids(&status), vec![id("A")]);
    }

    #[test]
    fn make_default_of_an_unknown_device_is_an_error() {
        let w = world("A", &["A", "B"]);
        let (mut s, _) = supervisor(&w, &[]);

        assert!(s.make_default(&id("Z")).is_err());
        assert_eq!(lock(&w).default, Some(id("A")));
    }

    #[test]
    fn click_track_plays_on_the_source_and_follows_it() {
        let w = world("A", &["A", "B"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["A", "B"], true)).unwrap();

        s.set_click_track(true).unwrap();
        assert!(s.status().click_track);
        assert_eq!(
            s.click_track.as_ref().map(ClickTrack::device),
            Some(&id("A"))
        );

        s.make_default(&id("B")).unwrap();
        assert_eq!(
            s.click_track.as_ref().map(ClickTrack::device),
            Some(&id("B"))
        );

        s.set_click_track(false).unwrap();
        assert!(!s.status().click_track);
    }

    #[test]
    fn isolating_another_device_mutes_the_source_until_released() {
        let w = world("A", &["A", "B", "C"]);
        let (mut s, journal) = supervisor(&w, &[]);
        s.start(config(&["B", "C"], true)).unwrap();

        s.isolate(Some(id("B"))).unwrap();
        assert!(lock(&w).muted.contains(&id("A")));
        assert_eq!(*lock(&journal), vec![id("A")]);
        assert_eq!(s.status().isolated, Some(id("B")));

        s.isolate(None).unwrap();
        assert!(lock(&w).muted.is_empty());
    }

    #[test]
    fn isolating_the_source_silences_the_outputs_but_keeps_it_playing() {
        let w = world("A", &["A", "B"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B"], true)).unwrap();

        s.isolate(Some(id("A"))).unwrap();

        assert!(lock(&w).muted.is_empty());
        let session = &s.active.as_ref().unwrap().session;
        assert!(session.isolated_for_test(&id("B")));
    }

    #[test]
    fn stop_ends_click_track_and_isolation() {
        let w = world("A", &["A", "B"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B"], true)).unwrap();
        s.set_click_track(true).unwrap();
        s.isolate(Some(id("B"))).unwrap();

        s.stop().unwrap();

        assert!(!s.status().click_track);
        assert_eq!(s.status().isolated, None);
        assert!(lock(&w).muted.is_empty());
    }

    fn supervisor_with_volumes(
        world: &SharedWorld,
        volumes: &[(&str, f32)],
    ) -> (TestSupervisor, crate::test_fakes::VolumeRecord) {
        let record: crate::test_fakes::VolumeRecord = Arc::new(Mutex::new(
            volumes.iter().map(|&(raw, v)| (id(raw), v)).collect(),
        ));
        let supervisor = Supervisor::new(
            FakePlatform::new(world),
            FakeJournal(Arc::default(), Arc::clone(&record)),
        )
        .unwrap();
        (supervisor, record)
    }

    fn scalar(w: &SharedWorld, raw: &str) -> f32 {
        lock(w).volumes.get(&id(raw)).map_or(1.0, |v| v.scalar())
    }

    #[test]
    fn following_raises_target_volumes_to_full_and_stop_restores_them() {
        let w = world("A", &["A", "B", "C"]);
        lock(&w)
            .volumes
            .insert(id("B"), SystemVolume::new(-35.0, 0.1, false));
        lock(&w)
            .volumes
            .insert(id("A"), SystemVolume::new(-3.0, 0.85, false));
        let (mut s, record) = supervisor_with_volumes(&w, &[]);

        s.start(config(&["B", "C"], true)).unwrap();

        assert!((scalar(&w, "B") - 1.0).abs() < f32::EPSILON);
        assert!(
            (scalar(&w, "A") - 0.85).abs() < f32::EPSILON,
            "source untouched"
        );
        assert_eq!(*lock(&record), vec![(id("B"), 0.1)], "C was already full");

        s.stop().unwrap();

        assert!((scalar(&w, "B") - 0.1).abs() < f32::EPSILON);
        assert!(lock(&record).is_empty());
    }

    #[test]
    fn not_following_leaves_device_volumes_alone() {
        let w = world("A", &["A", "B"]);
        lock(&w)
            .volumes
            .insert(id("B"), SystemVolume::new(-35.0, 0.1, false));
        let (mut s, _) = supervisor_with_volumes(&w, &[]);
        let mut cfg = config(&["B"], true);
        cfg.follow_system_volume = false;

        s.start(cfg).unwrap();

        assert!((scalar(&w, "B") - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn switching_following_off_restores_volumes() {
        let w = world("A", &["A", "B"]);
        lock(&w)
            .volumes
            .insert(id("B"), SystemVolume::new(-35.0, 0.1, false));
        let (mut s, _) = supervisor_with_volumes(&w, &[]);
        s.start(config(&["B"], true)).unwrap();
        let mut cfg = config(&["B"], true);
        cfg.follow_system_volume = false;

        s.update(cfg).unwrap();

        assert!((scalar(&w, "B") - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn startup_restores_volumes_left_by_a_crash() {
        let w = world("A", &["A", "B"]);

        let (_s, record) = supervisor_with_volumes(&w, &[("B", 0.44)]);

        assert!((scalar(&w, "B") - 0.44).abs() < f32::EPSILON);
        assert!(lock(&record).is_empty());
    }

    #[test]
    fn volume_of_a_disconnected_device_is_restored_when_it_returns() {
        let w = world("A", &["A"]);
        let (mut s, record) = supervisor_with_volumes(&w, &[("B", 0.44)]);
        assert_eq!(lock(&record).len(), 1);

        lock(&w).active.push(id("B"));
        s.devices_changed().unwrap();

        assert!((scalar(&w, "B") - 0.44).abs() < f32::EPSILON);
        assert!(lock(&record).is_empty());
    }

    fn chrome() -> MutedApp {
        MutedApp {
            pid: 42,
            name: "chrome".into(),
        }
    }

    #[test]
    fn app_muted_on_the_default_device_is_reported_even_while_stopped() {
        let w = world("A", &["A", "B"]);
        lock(&w).muted_apps.push((id("A"), chrome()));
        lock(&w).muted_apps.push((
            id("B"),
            MutedApp {
                pid: 7,
                name: "other".into(),
            },
        ));
        let (mut s, _) = supervisor(&w, &[]);

        s.maintain(Instant::now()).unwrap();

        assert_eq!(s.status().muted_apps, vec![chrome()]);
    }

    #[test]
    fn f17_battery_levels_are_read_every_minute_also_while_stopped() {
        let w = world("A", &["A", "B"]);
        lock(&w).batteries.push((id("B"), 40));
        lock(&w).batteries.push((id("Z"), 90));
        let (mut s, _) = supervisor(&w, &[]);
        let start = Instant::now();

        s.maintain(start).unwrap();
        assert_eq!(
            s.status().batteries,
            vec![(id("B"), 40)],
            "unknown Z left out"
        );

        lock(&w).batteries[0].1 = 39;
        s.maintain(start + Duration::from_secs(30)).unwrap();
        assert_eq!(s.status().batteries, vec![(id("B"), 40)], "not re-read yet");

        s.maintain(start + BATTERY_CHECK).unwrap();
        assert_eq!(s.status().batteries, vec![(id("B"), 39)]);
    }

    #[test]
    fn unmuting_an_app_clears_the_warning() {
        let w = world("A", &["A", "B"]);
        lock(&w).muted_apps.push((id("A"), chrome()));
        let (mut s, _) = supervisor(&w, &[]);
        s.maintain(Instant::now()).unwrap();

        s.unmute_app(42).unwrap();

        assert!(lock(&w).muted_apps.is_empty());
        assert!(s.status().muted_apps.is_empty());
    }

    #[test]
    fn muted_apps_are_checked_about_once_a_second() {
        let w = world("A", &["A", "B"]);
        let (mut s, _) = supervisor(&w, &[]);
        let now = Instant::now();
        s.maintain(now).unwrap();

        lock(&w).muted_apps.push((id("A"), chrome()));
        s.maintain(now + Duration::from_millis(500)).unwrap();
        assert!(s.status().muted_apps.is_empty());
        s.maintain(now + MUTED_APP_CHECK).unwrap();

        assert_eq!(s.status().muted_apps, vec![chrome()]);
    }

    #[test]
    fn outputs_follow_the_system_volume_of_the_default_device() {
        let w = world("A", &["A", "B"]);
        lock(&w)
            .volumes
            .insert(id("A"), SystemVolume::new(-6.020_6, 0.6, false));
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B"], true)).unwrap();

        s.maintain(Instant::now()).unwrap();

        assert!((master_gain(&s) - 0.5).abs() < 1e-3);
        assert_eq!(s.status().system_volume_percent, Some(60));
    }

    #[test]
    fn muting_the_system_volume_silences_the_followers() {
        let w = world("A", &["A", "B"]);
        lock(&w)
            .volumes
            .insert(id("A"), SystemVolume::new(-6.0, 0.6, true));
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B"], true)).unwrap();

        s.maintain(Instant::now()).unwrap();

        assert!(master_gain(&s).abs() < f32::EPSILON);
    }

    #[test]
    fn our_own_mute_of_the_source_does_not_silence_the_followers() {
        let w = world("A", &["A", "B"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B"], false)).unwrap();
        assert!(lock(&w).muted.contains(&id("A")));

        s.maintain(Instant::now()).unwrap();

        assert!((master_gain(&s) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn not_following_keeps_unity_gain() {
        let w = world("A", &["A", "B"]);
        lock(&w)
            .volumes
            .insert(id("A"), SystemVolume::new(-20.0, 0.2, false));
        let (mut s, _) = supervisor(&w, &[]);
        let mut cfg = config(&["B"], true);
        cfg.follow_system_volume = false;
        s.start(cfg).unwrap();

        s.maintain(Instant::now()).unwrap();

        assert!((master_gain(&s) - 1.0).abs() < f32::EPSILON);
        assert_eq!(s.status().system_volume_percent, None);
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
    fn f16_space_changes_live_without_restart() {
        let w = world("A", &["A", "B"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B"], false)).unwrap();
        let mut changed = config(&["B"], false);
        changed.space.reverb = stereo_bt_core::Percent::new(30).unwrap();

        s.update(changed.clone()).unwrap();

        assert_eq!(s.status().session_starts, 1);
        assert_eq!(s.active.as_ref().unwrap().session.space(), changed.space);
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
    fn status_lists_devices_even_while_stopped() {
        let w = world("A", &["A", "B"]);
        let (s, _) = supervisor(&w, &[]);

        let devices = s.status().devices;

        assert_eq!(devices.len(), 2);
        assert!(devices.iter().any(|d| d.id == id("A") && d.is_default));
    }

    #[test]
    fn device_list_follows_hot_plug_while_stopped() {
        let w = world("A", &["A"]);
        let (mut s, _) = supervisor(&w, &[]);

        lock(&w).active.push(id("B"));
        s.devices_changed().unwrap();

        assert_eq!(s.status().devices.len(), 2);
    }

    fn wait_for(s: &TestSupervisor, what: &str, condition: impl Fn(&EngineStatus) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition(&s.status()) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn state_of(status: &EngineStatus, raw: &str) -> Option<StreamState> {
        status
            .outputs
            .iter()
            .find(|o| o.id == id(raw))
            .map(|o| o.state)
    }

    #[test]
    fn enabling_a_device_adds_it_without_reopening_the_others() {
        let w = world("A", &["A", "B", "C"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B"], false)).unwrap();
        wait_for(&s, "B running", |st| {
            state_of(st, "B") == Some(StreamState::Running)
        });

        s.update(config(&["B", "C"], false)).unwrap();

        wait_for(&s, "C running", |st| {
            state_of(st, "C") == Some(StreamState::Running)
        });
        assert_eq!(s.status().session_starts, 1);
        assert_eq!(
            lock(&w)
                .render_opens
                .iter()
                .filter(|o| *o == &id("B"))
                .count(),
            1
        );
    }

    #[test]
    fn f6_failed_device_reports_its_error_and_is_retried_after_backoff() {
        let w = world("A", &["A", "B", "C"]);
        lock(&w).failing.insert(id("C"));
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B", "C"], false)).unwrap();
        wait_for(&s, "C failed", |st| {
            state_of(st, "C") == Some(StreamState::Failed)
        });
        let error = s.status().outputs[1].error.clone();
        assert_eq!(error.as_deref(), Some("fake error"));

        lock(&w).failing.clear();
        let now = Instant::now();
        s.maintain(now).unwrap();
        s.maintain(now + RETRY_DELAY / 2).unwrap();
        assert_eq!(state_of(&s.status(), "C"), Some(StreamState::Failed));
        s.maintain(now + RETRY_DELAY).unwrap();

        wait_for(&s, "C running again", |st| {
            state_of(st, "C") == Some(StreamState::Running)
        });
        assert_eq!(state_of(&s.status(), "B"), Some(StreamState::Running));
        assert_eq!(s.status().session_starts, 1);
    }

    #[test]
    fn failed_capture_restarts_the_session_after_backoff() {
        let w = world("A", &["A", "B"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B"], false)).unwrap();
        lock(&w).capture_failing = true;
        wait_for(&s, "capture failed", |st| {
            st.capture == Some(StreamState::Failed)
        });
        assert_eq!(s.status().capture_error.as_deref(), Some("fake error"));

        lock(&w).capture_failing = false;
        let now = Instant::now();
        s.maintain(now).unwrap();
        s.maintain(now + RETRY_DELAY).unwrap();

        assert_eq!(s.status().session_starts, 2);
        wait_for(&s, "capture running", |st| {
            st.capture == Some(StreamState::Running)
        });
    }

    #[test]
    fn session_streams_after_start() {
        let w = world("A", &["A", "B"]);
        let (mut s, _) = supervisor(&w, &[]);
        s.start(config(&["B"], false)).unwrap();

        // Capture and render threads start independently; wait for both.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let status = s.status();
            if status.capture == Some(StreamState::Running)
                && status.outputs[0].state == StreamState::Running
            {
                break;
            }
            assert!(Instant::now() < deadline, "session never started");
            thread::sleep(Duration::from_millis(10));
        }
    }
}
