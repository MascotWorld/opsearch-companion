mod api;
mod config;
mod profiles;
mod settings;
mod steam;
mod types;
mod update;
mod vdf;
mod worker;

pub use api::DEFAULT_SERVER;
pub use profiles::{ProfileLoader, SteamProfile};
pub use settings::{BACKGROUND_FLAG, Settings, autostart};
pub use types::{Command, CoreError, Event, Player, Roster, Status};
pub use update::{CURRENT_VERSION, RELEASES_PAGE, UpdateChecker, UpdateInfo};
pub use worker::CoreHandle;

/// Opens a page in the default browser without blocking the window.
pub fn open_in_browser(url: String) {
    let _ = std::thread::Builder::new()
        .name("opsearch-companion-open".into())
        .spawn(move || {
            let _ = webbrowser::open(&url);
        });
}
