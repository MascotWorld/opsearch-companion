use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::config::config_path;
use super::types::{CoreError, CoreResult};

/// Command-line flag the autostart entry passes, so a launch at sign-in starts out of the way.
pub const BACKGROUND_FLAG: &str = "--background";

/// Window preferences. Both are off until the user turns them on in settings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Closing or minimising hides the window to the notification area instead.
    #[serde(default)]
    pub tray: bool,
}

impl Settings {
    pub fn load() -> Self {
        settings_path()
            .ok()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> CoreResult<()> {
        let path = settings_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|_| CoreError::Config("could not encode settings".into()))?;
        fs::write(path, bytes)?;
        Ok(())
    }
}

fn settings_path() -> CoreResult<PathBuf> {
    Ok(config_path()?.with_file_name("settings.json"))
}

/// Starting with Windows is kept in the user's own Run key rather than in the settings file,
/// so it stays true to what Windows will actually do.
pub mod autostart {
    use super::*;

    #[cfg(windows)]
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    #[cfg(windows)]
    const VALUE: &str = "OPSearch Companion";

    pub fn supported() -> bool {
        cfg!(windows)
    }

    #[cfg(windows)]
    pub fn enabled() -> bool {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(RUN_KEY)
            .and_then(|key| key.get_value::<String, _>(VALUE))
            .is_ok()
    }

    #[cfg(windows)]
    pub fn set(on: bool) -> CoreResult<()> {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
        if on {
            let exe = std::env::current_exe()?;
            key.set_value(VALUE, &format!("\"{}\" {BACKGROUND_FLAG}", exe.display()))?;
        } else {
            match key.delete_value(VALUE) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(error.into());
                }
                _ => {}
            }
        }
        Ok(())
    }

    #[cfg(not(windows))]
    pub fn enabled() -> bool {
        false
    }

    #[cfg(not(windows))]
    pub fn set(_on: bool) -> CoreResult<()> {
        Err(CoreError::WindowsOnly)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_is_off_by_default_and_old_files_still_load() {
        assert_eq!(Settings::default(), Settings { tray: false });
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(!old.tray);
    }
}
