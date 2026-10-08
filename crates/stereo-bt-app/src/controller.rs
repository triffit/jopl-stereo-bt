//! Application logic between the window and the engine: applies user
//! actions to the settings, forwards them to the engine and saves them.
//! Free of UI code so it is fully testable.

use std::time::{Duration, Instant};

use stereo_bt_core::{ChannelMode, Delay, DeviceId, Volume};
use stereo_bt_engine::{Engine, EngineConfig, EngineStatus};

use crate::settings::{DeviceSettings, Settings};
use crate::settings_store::SettingsStore;
use crate::view_model::{self, DeviceRow};

/// Settings are written this long after the last change, so dragging a
/// slider does not write the file on every step.
const SAVE_DELAY: Duration = Duration::from_secs(1);

/// The engine operations the controller uses; abstracted for tests.
pub(crate) trait EngineControl {
    fn start(&self, config: EngineConfig) -> Result<(), String>;
    fn update(&self, config: EngineConfig) -> Result<(), String>;
    fn stop(&self) -> Result<(), String>;
    fn status(&self) -> EngineStatus;
    fn last_error(&self) -> Option<String>;
}

impl EngineControl for Engine {
    fn start(&self, config: EngineConfig) -> Result<(), String> {
        Engine::start(self, config).map_err(|e| e.to_string())
    }

    fn update(&self, config: EngineConfig) -> Result<(), String> {
        Engine::update(self, config).map_err(|e| e.to_string())
    }

    fn stop(&self) -> Result<(), String> {
        Engine::stop(self).map_err(|e| e.to_string())
    }

    fn status(&self) -> EngineStatus {
        Engine::status(self)
    }

    fn last_error(&self) -> Option<String> {
        Engine::last_error(self)
    }
}

/// Everything the window displays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct View {
    pub(crate) running: bool,
    pub(crate) include_default: bool,
    pub(crate) rows: Vec<DeviceRow>,
    pub(crate) status_line: String,
    pub(crate) error: Option<String>,
}

pub(crate) struct Controller<E: EngineControl, S: SettingsStore> {
    engine: E,
    store: S,
    settings: Settings,
    status: EngineStatus,
    running: bool,
    error: Option<String>,
    engine_dirty: bool,
    unsaved_since: Option<Instant>,
}

impl<E: EngineControl, S: SettingsStore> Controller<E, S> {
    pub(crate) fn new(engine: E, store: S) -> Self {
        let (settings, error) = match store.load() {
            Ok(settings) => (settings, None),
            Err(e) => (
                Settings::default(),
                Some(format!(
                    "Saved settings could not be read ({e}); using defaults."
                )),
            ),
        };
        let mut controller = Self {
            status: engine.status(),
            engine,
            store,
            settings,
            running: false,
            error,
            engine_dirty: false,
            unsaved_since: None,
        };
        if controller.settings.learn(&controller.status.devices) {
            controller.unsaved_since = Some(Instant::now());
        }
        controller
    }

    /// Start/Stop button (F5).
    pub(crate) fn toggle_running(&mut self) {
        let result = if self.running {
            self.engine.stop()
        } else {
            self.engine.start(self.settings.engine_config())
        };
        match result {
            Ok(()) => {
                self.running = !self.running;
                self.error = None;
                self.engine_dirty = false;
            }
            Err(e) => self.error = Some(e),
        }
        self.status = self.engine.status();
    }

    pub(crate) fn set_include_default(&mut self, include: bool) {
        self.settings.include_default = include;
        self.changed();
    }

    pub(crate) fn set_enabled(&mut self, id: &DeviceId, enabled: bool) {
        self.edit(id, |d| d.enabled = enabled);
    }

    pub(crate) fn set_channel_mode(&mut self, id: &DeviceId, mode: ChannelMode) {
        self.edit(id, |d| d.channel_mode = mode);
    }

    /// Sets the delay, clamped to the supported range (F3).
    pub(crate) fn set_delay(&mut self, id: &DeviceId, millis: i64) {
        let clamped = u16::try_from(millis.clamp(0, i64::from(Delay::MAX_MILLIS))).unwrap_or(0);
        let delay = Delay::from_millis(clamped).unwrap_or(Delay::ZERO);
        self.edit(id, |d| d.delay = delay);
    }

    /// Changes the delay by `step` milliseconds (fine ±1 ms nudge).
    pub(crate) fn nudge_delay(&mut self, id: &DeviceId, step: i64) {
        if let Some(device) = self.settings.device(id) {
            let current = i64::from(device.delay.as_millis());
            self.set_delay(id, current + step);
        }
    }

    /// Sets the volume, clamped to 0..=100 % (F4).
    pub(crate) fn set_volume(&mut self, id: &DeviceId, percent: i64) {
        let clamped = u8::try_from(percent.clamp(0, 100)).unwrap_or(100);
        let volume = Volume::from_percent(clamped).unwrap_or(Volume::FULL);
        self.edit(id, |d| d.volume = volume);
    }

    /// Periodic work: refresh status, learn new devices, push pending
    /// changes to the engine and save settings once edits settle.
    pub(crate) fn tick(&mut self, now: Instant) {
        self.status = self.engine.status();
        if self.settings.learn(&self.status.devices) {
            self.unsaved_since.get_or_insert(now);
        }
        if self.engine_dirty {
            self.engine_dirty = false;
            if let Err(e) = self.engine.update(self.settings.engine_config()) {
                self.error = Some(e);
            }
        }
        if self
            .unsaved_since
            .is_some_and(|since| now.duration_since(since) >= SAVE_DELAY)
        {
            self.save();
        }
    }

    /// Saves pending changes immediately (on exit).
    pub(crate) fn save(&mut self) {
        self.unsaved_since = None;
        if let Err(e) = self.store.save(&self.settings) {
            self.error = Some(format!("Settings could not be saved: {e}"));
        }
    }

    pub(crate) fn view(&self) -> View {
        View {
            running: self.running,
            include_default: self.settings.include_default,
            rows: view_model::device_rows(&self.settings, &self.status),
            status_line: view_model::status_line(&self.settings, &self.status),
            error: self.error.clone().or_else(|| self.engine.last_error()),
        }
    }

    fn edit(&mut self, id: &DeviceId, apply: impl FnOnce(&mut DeviceSettings)) {
        if let Some(device) = self.settings.device_mut(id) {
            apply(device);
            self.changed();
        }
    }

    fn changed(&mut self) {
        self.engine_dirty = true;
        self.unsaved_since.get_or_insert_with(Instant::now);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io;
    use std::rc::Rc;

    use stereo_bt_core::DeviceInfo;

    use super::*;

    #[derive(Default)]
    struct EngineLog {
        started: Vec<EngineConfig>,
        updated: Vec<EngineConfig>,
        stopped: usize,
        fail: bool,
        status: EngineStatus,
    }

    #[derive(Clone, Default)]
    struct FakeEngine(Rc<RefCell<EngineLog>>);

    impl EngineControl for FakeEngine {
        fn start(&self, config: EngineConfig) -> Result<(), String> {
            let mut log = self.0.borrow_mut();
            if log.fail {
                return Err("no audio".into());
            }
            log.started.push(config);
            Ok(())
        }

        fn update(&self, config: EngineConfig) -> Result<(), String> {
            self.0.borrow_mut().updated.push(config);
            Ok(())
        }

        fn stop(&self) -> Result<(), String> {
            self.0.borrow_mut().stopped += 1;
            Ok(())
        }

        fn status(&self) -> EngineStatus {
            self.0.borrow().status.clone()
        }

        fn last_error(&self) -> Option<String> {
            None
        }
    }

    #[derive(Clone, Default)]
    struct FakeStore {
        saved: Rc<RefCell<Vec<Settings>>>,
        corrupt: bool,
    }

    impl SettingsStore for FakeStore {
        fn load(&self) -> io::Result<Settings> {
            if self.corrupt {
                Err(io::Error::other("bad toml"))
            } else {
                Ok(Settings::default())
            }
        }

        fn save(&self, settings: &Settings) -> io::Result<()> {
            self.saved.borrow_mut().push(settings.clone());
            Ok(())
        }
    }

    fn id(raw: &str) -> DeviceId {
        DeviceId::new(raw).unwrap()
    }

    fn controller() -> (Controller<FakeEngine, FakeStore>, FakeEngine, FakeStore) {
        let engine = FakeEngine::default();
        engine.0.borrow_mut().status.devices = vec![
            DeviceInfo {
                id: id("A"),
                name: "A".into(),
                is_default: true,
            },
            DeviceInfo {
                id: id("B"),
                name: "B".into(),
                is_default: false,
            },
        ];
        let store = FakeStore::default();
        (
            Controller::new(engine.clone(), store.clone()),
            engine,
            store,
        )
    }

    #[test]
    fn f5_start_sends_enabled_devices_and_stop_stops() {
        let (mut c, engine, _) = controller();
        c.set_enabled(&id("B"), true);

        c.toggle_running();
        assert!(c.view().running);
        c.toggle_running();

        let log = engine.0.borrow();
        assert_eq!(log.started[0].outputs[0].id, id("B"));
        assert_eq!(log.stopped, 1);
        assert!(!c.view().running);
    }

    #[test]
    fn failed_start_shows_the_error_and_stays_stopped() {
        let (mut c, engine, _) = controller();
        engine.0.borrow_mut().fail = true;

        c.toggle_running();

        assert!(!c.view().running);
        assert_eq!(c.view().error.as_deref(), Some("no audio"));
    }

    #[test]
    fn edits_reach_the_engine_on_the_next_tick_coalesced() {
        let (mut c, engine, _) = controller();
        c.set_enabled(&id("B"), true);
        c.toggle_running();

        c.set_delay(&id("B"), 100);
        c.set_delay(&id("B"), 120);
        c.tick(Instant::now());

        let log = engine.0.borrow();
        assert_eq!(log.updated.len(), 1);
        assert_eq!(log.updated[0].outputs[0].delay.as_millis(), 120);
    }

    #[test]
    fn f3_delay_is_clamped_and_nudged_in_one_ms_steps() {
        let (mut c, _, _) = controller();

        c.set_delay(&id("B"), 9_999);
        assert_eq!(c.view().rows[1].delay_ms, 500);
        c.nudge_delay(&id("B"), 1);
        assert_eq!(c.view().rows[1].delay_ms, 500);
        c.nudge_delay(&id("B"), -1);
        assert_eq!(c.view().rows[1].delay_ms, 499);
        c.set_delay(&id("B"), -5);
        assert_eq!(c.view().rows[1].delay_ms, 0);
    }

    #[test]
    fn f4_volume_is_clamped() {
        let (mut c, _, _) = controller();

        c.set_volume(&id("B"), 250);

        assert_eq!(c.view().rows[1].volume_percent, 100);
    }

    #[test]
    fn f8_settings_are_saved_once_edits_settle() {
        let (mut c, _, store) = controller();
        let start = Instant::now();
        c.save();
        store.saved.borrow_mut().clear();

        c.set_channel_mode(&id("B"), ChannelMode::LeftOnly);
        c.tick(start);
        assert!(store.saved.borrow().is_empty());
        c.tick(start + SAVE_DELAY + Duration::from_millis(1));

        let saved = store.saved.borrow();
        assert_eq!(saved.len(), 1);
        assert_eq!(
            saved[0].device(&id("B")).unwrap().channel_mode,
            ChannelMode::LeftOnly
        );
    }

    #[test]
    fn corrupt_settings_fall_back_to_defaults_with_a_message() {
        let store = FakeStore {
            corrupt: true,
            ..FakeStore::default()
        };

        let c = Controller::new(FakeEngine::default(), store);

        assert!(c.view().error.unwrap().contains("using defaults"));
    }
}
