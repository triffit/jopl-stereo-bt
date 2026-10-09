//! Application logic between the window and the engine: applies user
//! actions to the settings, forwards them to the engine and saves them.
//! Free of UI code so it is fully testable.

use std::time::{Duration, Instant};

use stereo_bt_core::{ChannelMode, Delay, DeviceId, Volume};
use stereo_bt_engine::{Engine, EngineConfig, EngineStatus};

use crate::assistant::{Assistant, Phase, Step, TAPS_WANTED};
use crate::settings::{DeviceSettings, Settings};
use crate::settings_store::SettingsStore;
use crate::view_model::{self, DeviceRow, StatusKind};

/// Settings are written this long after the last change, so dragging a
/// slider does not write the file on every step.
const SAVE_DELAY: Duration = Duration::from_secs(1);

/// The engine operations the controller uses; abstracted for tests.
pub(crate) trait EngineControl {
    fn start(&self, config: EngineConfig) -> Result<(), String>;
    fn update(&self, config: EngineConfig) -> Result<(), String>;
    fn stop(&self) -> Result<(), String>;
    fn make_default(&self, id: &DeviceId) -> Result<(), String>;
    fn set_click_track(&self, on: bool) -> Result<(), String>;
    fn isolate(&self, device: Option<DeviceId>) -> Result<(), String>;
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

    fn make_default(&self, id: &DeviceId) -> Result<(), String> {
        Engine::make_default(self, id.clone()).map_err(|e| e.to_string())
    }

    fn set_click_track(&self, on: bool) -> Result<(), String> {
        Engine::set_click_track(self, on).map_err(|e| e.to_string())
    }

    fn isolate(&self, device: Option<DeviceId>) -> Result<(), String> {
        Engine::isolate(self, device).map_err(|e| e.to_string())
    }

    fn status(&self) -> EngineStatus {
        Engine::status(self)
    }

    fn last_error(&self) -> Option<String> {
        Engine::last_error(self)
    }
}

/// One line of the assistant's results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResultLine {
    pub(crate) name: String,
    pub(crate) text: String,
    pub(crate) warning: bool,
}

/// What the sync assistant shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AssistantView {
    pub(crate) phase: Phase,
    /// 1-based step and number of steps, while measuring.
    pub(crate) step: usize,
    pub(crate) steps: usize,
    pub(crate) device: String,
    pub(crate) is_source: bool,
    pub(crate) taps: usize,
    pub(crate) taps_wanted: usize,
    pub(crate) can_next: bool,
    pub(crate) results: Vec<ResultLine>,
}

/// Everything the window displays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct View {
    pub(crate) running: bool,
    pub(crate) include_default: bool,
    pub(crate) follow_system_volume: bool,
    /// The followed Windows volume in percent, while streaming and following.
    pub(crate) system_volume_percent: Option<u8>,
    pub(crate) rows: Vec<DeviceRow>,
    pub(crate) status_line: String,
    pub(crate) status_kind: StatusKind,
    pub(crate) error: Option<String>,
    pub(crate) assistant: Option<AssistantView>,
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
    assistant: Option<Assistant>,
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
            assistant: None,
        };
        if controller.settings.learn(&controller.status.devices) {
            controller.unsaved_since = Some(Instant::now());
        }
        controller
    }

    /// Start/Stop button (F5).
    pub(crate) fn toggle_running(&mut self) {
        if self.running {
            self.assistant_close();
        }
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

    pub(crate) fn is_running(&self) -> bool {
        self.running
    }

    pub(crate) fn set_include_default(&mut self, include: bool) {
        self.settings.include_default = include;
        self.changed();
    }

    /// Makes `id` the Windows default device (the audio source).
    pub(crate) fn make_default(&mut self, id: &DeviceId) {
        match self.engine.make_default(id) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("Could not make it the default device: {e}")),
        }
        self.status = self.engine.status();
    }

    /// Makes every device follow the Windows volume (or not).
    pub(crate) fn set_follow_system_volume(&mut self, follow: bool) {
        self.settings.follow_system_volume = follow;
        self.changed();
    }

    /// Checks or unchecks a device. For the default (source) device this
    /// chooses between playing natively and muting it while streaming.
    pub(crate) fn set_enabled(&mut self, id: &DeviceId, enabled: bool) {
        let is_source = self
            .status
            .devices
            .iter()
            .any(|d| d.is_default && &d.id == id);
        if is_source {
            self.set_include_default(enabled);
        } else {
            self.edit(id, |d| d.enabled = enabled);
        }
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
            follow_system_volume: self.settings.follow_system_volume,
            system_volume_percent: self.status.system_volume_percent,
            rows: view_model::device_rows(&self.settings, &self.status),
            status_line: view_model::status_line(&self.settings, &self.status),
            status_kind: view_model::status_kind(&self.status),
            error: self.error.clone().or_else(|| self.engine.last_error()),
            assistant: self.assistant.as_ref().map(|a| self.assistant_view(a)),
        }
    }

    /// Opens the sync assistant: clicks start on the default device, which
    /// is measured first, alone.
    pub(crate) fn start_assistant(&mut self, now: Instant) {
        if !self.running {
            self.error = Some("Start streaming first, then run the sync assistant.".into());
            return;
        }
        let steps = self.assistant_steps();
        if steps.len() < 2 {
            self.error = Some(
                "The sync assistant needs at least one playing device besides the default.".into(),
            );
            return;
        }
        let first = steps[0].id.clone();
        let result = self
            .engine
            .set_click_track(true)
            .and_then(|()| self.engine.isolate(Some(first)));
        match result {
            Ok(()) => {
                self.error = None;
                self.assistant = Some(Assistant::new(steps, now));
            }
            Err(e) => {
                self.error = Some(format!("The sync assistant could not start: {e}"));
                self.assistant_close();
            }
        }
    }

    /// The user heard a click.
    pub(crate) fn assistant_tap(&mut self, now: Instant) {
        if let Some(assistant) = self.assistant.as_mut() {
            assistant.tap(now);
        }
    }

    pub(crate) fn assistant_restart_step(&mut self) {
        if let Some(assistant) = self.assistant.as_mut() {
            assistant.restart_step();
        }
    }

    /// Finishes the current device and isolates the next one (or all again
    /// when every device is measured).
    pub(crate) fn assistant_next(&mut self) {
        let clicks = self.engine.status().click_times;
        let Some(assistant) = self.assistant.as_mut() else {
            return;
        };
        let next = assistant.advance(&clicks).map(|step| step.id.clone());
        if let Err(e) = self.engine.isolate(next) {
            self.error = Some(e);
        }
    }

    /// Uses the suggested delays, then lets all devices click together for
    /// fine-tuning by ear.
    pub(crate) fn assistant_apply(&mut self) {
        let Some(assistant) = self.assistant.as_ref() else {
            return;
        };
        let settings = &self.settings;
        let suggestions = assistant.suggestions(|id| {
            settings
                .device(id)
                .map_or(stereo_bt_core::Delay::ZERO, |d| d.delay)
        });
        for line in suggestions {
            if let Some(suggestion) = line.suggestion {
                self.edit(&line.id, |d| d.delay = suggestion.delay);
            }
        }
        if let Some(assistant) = self.assistant.as_mut() {
            assistant.start_fine_tune();
        }
    }

    /// Closes the assistant: clicks stop and every device plays again.
    pub(crate) fn assistant_close(&mut self) {
        if self.assistant.take().is_none() {
            return;
        }
        let result = self
            .engine
            .set_click_track(false)
            .and(self.engine.isolate(None));
        if let Err(e) = result {
            self.error = Some(e);
        }
    }

    /// The default device first (the reference), then every playing device.
    fn assistant_steps(&self) -> Vec<Step> {
        let name = |id: &DeviceId| {
            self.settings
                .device(id)
                .map_or_else(|| id.to_string(), |d| d.name.clone())
        };
        let source = self.status.source.iter().map(|id| Step {
            id: id.clone(),
            name: name(id),
            is_source: true,
        });
        let outputs = self
            .status
            .outputs
            .iter()
            .filter(|o| o.state == stereo_bt_engine::StreamState::Running)
            .map(|o| Step {
                id: o.id.clone(),
                name: name(&o.id),
                is_source: false,
            });
        source.chain(outputs).collect()
    }

    fn assistant_view(&self, assistant: &Assistant) -> AssistantView {
        let (step, steps) = assistant.progress();
        let current = assistant.current_step();
        let results = assistant
            .suggestions(|id| {
                self.settings
                    .device(id)
                    .map_or(stereo_bt_core::Delay::ZERO, |d| d.delay)
            })
            .into_iter()
            .map(|line| {
                let (text, warning) = match line.suggestion {
                    Some(s) if s.too_late => (
                        "Plays later than the default device. Make the slowest device the default."
                            .to_owned(),
                        true,
                    ),
                    Some(s) => (format!("Delay {} ms", s.delay.as_millis()), false),
                    None => ("Not measured; tap along with the clicks.".to_owned(), true),
                };
                ResultLine {
                    name: line.name,
                    text,
                    warning,
                }
            })
            .collect();
        AssistantView {
            phase: assistant.phase(),
            step,
            steps,
            device: current.map(|s| s.name.clone()).unwrap_or_default(),
            is_source: current.is_some_and(|s| s.is_source),
            taps: assistant.taps(),
            taps_wanted: TAPS_WANTED,
            can_next: assistant.can_advance(),
            results,
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
        made_default: Vec<DeviceId>,
        click_track: bool,
        isolated: Vec<Option<DeviceId>>,
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

        fn make_default(&self, id: &DeviceId) -> Result<(), String> {
            let mut log = self.0.borrow_mut();
            if log.fail {
                return Err("refused".into());
            }
            log.made_default.push(id.clone());
            Ok(())
        }

        fn set_click_track(&self, on: bool) -> Result<(), String> {
            self.0.borrow_mut().click_track = on;
            Ok(())
        }

        fn isolate(&self, device: Option<DeviceId>) -> Result<(), String> {
            self.0.borrow_mut().isolated.push(device);
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
    fn f3_delay_is_clamped_to_the_supported_range() {
        let (mut c, _, _) = controller();

        c.set_delay(&id("B"), 9_999);
        assert_eq!(c.view().rows[1].delay_ms, 500);
        c.set_delay(&id("B"), 499);
        assert_eq!(c.view().rows[1].delay_ms, 499);
        c.set_delay(&id("B"), -5);
        assert_eq!(c.view().rows[1].delay_ms, 0);
    }

    #[test]
    fn checking_the_source_device_keeps_it_playing_natively() {
        let (mut c, _, _) = controller();

        c.set_enabled(&id("A"), true);

        assert!(c.view().include_default);
        assert!(c.view().rows[0].checked);
    }

    #[test]
    fn following_the_system_volume_is_sent_to_the_engine() {
        let (mut c, engine, _) = controller();
        c.set_enabled(&id("B"), true);
        c.toggle_running();

        c.set_follow_system_volume(false);
        c.tick(Instant::now());

        assert!(!c.view().follow_system_volume);
        assert!(!engine.0.borrow().updated[0].follow_system_volume);
    }

    #[test]
    fn make_default_asks_the_engine() {
        let (mut c, engine, _) = controller();

        c.make_default(&id("B"));

        assert_eq!(engine.0.borrow().made_default, vec![id("B")]);
        assert_eq!(c.view().error, None);
    }

    #[test]
    fn refused_make_default_shows_an_error() {
        let (mut c, engine, _) = controller();
        engine.0.borrow_mut().fail = true;

        c.make_default(&id("B"));

        assert!(c.view().error.unwrap().contains("refused"));
    }

    /// Streaming from A to B, with clicks every second from `epoch`.
    fn streaming(epoch: Instant) -> (Controller<FakeEngine, FakeStore>, FakeEngine) {
        let (mut c, engine, _) = controller();
        c.set_enabled(&id("B"), true);
        c.toggle_running();
        {
            let mut log = engine.0.borrow_mut();
            log.status.source = Some(id("A"));
            log.status.outputs = vec![stereo_bt_engine::OutputStatus {
                id: id("B"),
                state: stereo_bt_engine::StreamState::Running,
                underruns: 0,
                overruns: 0,
                trims: 0,
                error: None,
            }];
            log.status.click_times = (0..60).map(|n| epoch + Duration::from_secs(n)).collect();
        }
        c.tick(epoch);
        (c, engine)
    }

    fn tap_along(c: &mut Controller<FakeEngine, FakeStore>, epoch: Instant, from: u64, lag: u64) {
        for n in from..from + 8 {
            c.assistant_tap(epoch + Duration::from_millis(n * 1_000 + lag));
        }
    }

    #[test]
    fn sync_assistant_needs_streaming() {
        let (mut c, engine, _) = controller();

        c.start_assistant(Instant::now());

        assert!(c.view().assistant.is_none());
        assert!(c.view().error.unwrap().contains("Start streaming"));
        assert!(!engine.0.borrow().click_track);
    }

    #[test]
    fn sync_assistant_measures_the_default_device_first_alone() {
        let epoch = Instant::now();
        let (mut c, engine) = streaming(epoch);

        c.start_assistant(epoch);

        let view = c.view().assistant.unwrap();
        assert_eq!(view.phase, Phase::Measuring);
        assert_eq!((view.step, view.steps), (1, 2));
        assert!(view.is_source);
        let log = engine.0.borrow();
        assert!(log.click_track);
        assert_eq!(log.isolated.last(), Some(&Some(id("A"))));
    }

    #[test]
    fn sync_assistant_suggests_and_applies_delays_then_fine_tunes() {
        let epoch = Instant::now();
        let (mut c, engine) = streaming(epoch);
        c.start_assistant(epoch);
        tap_along(&mut c, epoch, 1, 260);
        c.assistant_next();
        assert_eq!(engine.0.borrow().isolated.last(), Some(&Some(id("B"))));
        tap_along(&mut c, epoch, 10, 90);
        c.assistant_next();

        let view = c.view().assistant.unwrap();
        assert_eq!(view.phase, Phase::Results);
        assert_eq!(view.results[0].text, "Delay 170 ms");
        assert_eq!(engine.0.borrow().isolated.last(), Some(&None));

        c.assistant_apply();

        assert_eq!(c.view().assistant.unwrap().phase, Phase::FineTune);
        assert_eq!(c.view().rows[1].delay_ms, 170);
    }

    #[test]
    fn closing_the_assistant_stops_clicks_and_isolation() {
        let epoch = Instant::now();
        let (mut c, engine) = streaming(epoch);
        c.start_assistant(epoch);

        c.assistant_close();

        assert!(c.view().assistant.is_none());
        let log = engine.0.borrow();
        assert!(!log.click_track);
        assert_eq!(log.isolated.last(), Some(&None));
    }

    #[test]
    fn stopping_closes_the_assistant() {
        let epoch = Instant::now();
        let (mut c, engine) = streaming(epoch);
        c.start_assistant(epoch);

        c.toggle_running();

        assert!(c.view().assistant.is_none());
        assert!(!engine.0.borrow().click_track);
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
