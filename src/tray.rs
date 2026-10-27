//! The notification-area icon. While it exists, closing or minimising the window only hides it;
//! the icon brings it back and its menu quits for real.

use eframe::egui;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set when the user picks "Выйти", so the next close request is not turned into a hide.
pub static QUITTING: AtomicBool = AtomicBool::new(false);

/// The app's window, reachable from the tray's own thread. A hidden eframe window runs no
/// frames, so viewport commands sent to it would wait forever; on Windows the window is
/// shown and hidden through Win32 directly instead.
#[derive(Clone)]
pub struct Window {
    ctx: egui::Context,
    #[cfg(windows)]
    hwnd: Option<isize>,
}

impl Window {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        #[cfg(windows)]
        let hwnd = {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            match cc.window_handle().map(|handle| handle.as_raw()) {
                Ok(RawWindowHandle::Win32(handle)) => Some(handle.hwnd.get()),
                _ => None,
            }
        };
        Self {
            ctx: cc.egui_ctx.clone(),
            #[cfg(windows)]
            hwnd,
        }
    }

    pub fn show(&self) {
        #[cfg(windows)]
        if let Some(hwnd) = self.hwnd {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow,
            };
            // SAFETY: the handle belongs to this process's own window, which lives as long as
            // the app; showing a window that is already gone is a harmless no-op.
            unsafe {
                let hwnd = hwnd as windows_sys::Win32::Foundation::HWND;
                ShowWindow(hwnd, SW_SHOW);
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
            self.ctx.request_repaint();
            return;
        }
        self.ctx
            .send_viewport_cmd(egui::ViewportCommand::Visible(true));
        self.ctx
            .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.ctx.request_repaint();
    }

    pub fn hide(&self) {
        #[cfg(windows)]
        if let Some(hwnd) = self.hwnd {
            use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};
            // SAFETY: as in `show`.
            unsafe {
                ShowWindow(hwnd as windows_sys::Win32::Foundation::HWND, SW_HIDE);
            }
            return;
        }
        self.ctx
            .send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }

    pub fn quit(&self) {
        QUITTING.store(true, Ordering::SeqCst);
        self.show();
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

#[cfg(any(windows, target_os = "macos"))]
pub use native::Tray;

#[cfg(any(windows, target_os = "macos"))]
mod native {
    use super::*;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    pub struct Tray {
        _icon: TrayIcon,
    }

    impl Tray {
        /// Must run on the thread that owns the window's event loop.
        pub fn new(window: &Window, icon: &Arc<egui::IconData>) -> Option<Self> {
            let open = MenuItem::new("Открыть", true, None);
            let quit = MenuItem::new("Выйти", true, None);
            let menu = Menu::new();
            menu.append_items(&[&open, &PredefinedMenuItem::separator(), &quit])
                .ok()?;
            let icon = Icon::from_rgba(icon.rgba.clone(), icon.width, icon.height).ok()?;
            let tray = TrayIconBuilder::new()
                .with_tooltip("OPSearch Companion")
                .with_icon(icon)
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .build()
                .ok()?;

            let clicks = window.clone();
            TrayIconEvent::set_event_handler(Some(move |event| match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
                | TrayIconEvent::DoubleClick { .. } => clicks.show(),
                _ => {}
            }));
            let (open, quit) = (open.id().clone(), quit.id().clone());
            let menus = window.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if event.id == open {
                    menus.show();
                } else if event.id == quit {
                    menus.quit();
                }
            }));
            Some(Self { _icon: tray })
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            TrayIconEvent::set_event_handler(None::<fn(TrayIconEvent)>);
            MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
        }
    }
}

/// Platforms without a notification area keep the plain window.
#[cfg(not(any(windows, target_os = "macos")))]
pub struct Tray;

#[cfg(not(any(windows, target_os = "macos")))]
impl Tray {
    pub fn new(_window: &Window, _icon: &Arc<egui::IconData>) -> Option<Self> {
        None
    }
}
