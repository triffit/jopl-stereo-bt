//! System tray icon (PRODUCT.md F7): keeps streaming reachable while the
//! window is hidden. Events are polled from the UI timer.

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::icon;

/// Something the user asked for through the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrayAction {
    Show,
    ToggleRunning,
    Quit,
}

pub(crate) struct Tray {
    icon: TrayIcon,
    toggle: MenuItem,
    show_id: MenuId,
    toggle_id: MenuId,
    quit_id: MenuId,
    running: bool,
}

impl Tray {
    /// Adds the tray icon. Must run on the UI thread.
    pub(crate) fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let show = MenuItem::new("Show JOPL-StereoBT", true, None);
        let toggle = MenuItem::new("Play on multiple devices", true, None);
        let quit = MenuItem::new("Quit", true, None);
        let menu = Menu::new();
        menu.append_items(&[&show, &toggle, &PredefinedMenuItem::separator(), &quit])?;
        let icon = TrayIconBuilder::new()
            .with_icon(Icon::from_rgba(icon::rgba(32), 32, 32)?)
            .with_tooltip("JOPL-StereoBT · stopped")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()?;
        Ok(Self {
            icon,
            show_id: show.id().clone(),
            toggle_id: toggle.id().clone(),
            quit_id: quit.id().clone(),
            toggle,
            running: false,
        })
    }

    /// Actions requested since the last poll.
    pub(crate) fn poll(&self) -> Vec<TrayAction> {
        let mut actions: Vec<TrayAction> = MenuEvent::receiver()
            .try_iter()
            .filter_map(|event| self.menu_action(event.id()))
            .collect();
        actions.extend(TrayIconEvent::receiver().try_iter().filter_map(|event| {
            matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick { .. }
            )
            .then_some(TrayAction::Show)
        }));
        actions
    }

    /// Reflects the streaming state in the menu and tooltip.
    pub(crate) fn set_running(&mut self, running: bool) {
        if running == self.running {
            return;
        }
        self.running = running;
        self.toggle.set_text(if running {
            "Stop playing"
        } else {
            "Play on multiple devices"
        });
        let tooltip = if running {
            "JOPL-StereoBT · streaming"
        } else {
            "JOPL-StereoBT · stopped"
        };
        // A tooltip that fails to update is cosmetic only.
        let _ = self.icon.set_tooltip(Some(tooltip));
    }

    fn menu_action(&self, id: &MenuId) -> Option<TrayAction> {
        if id == &self.show_id {
            Some(TrayAction::Show)
        } else if id == &self.toggle_id {
            Some(TrayAction::ToggleRunning)
        } else if id == &self.quit_id {
            Some(TrayAction::Quit)
        } else {
            None
        }
    }
}
