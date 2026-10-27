use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::steam::valid_steam_id;
use super::types::{CoreError, CoreResult};

#[derive(Default, Serialize, Deserialize)]
struct Config {
    #[serde(default)]
    devices: BTreeMap<String, DeviceCredential>,
}

#[derive(Serialize, Deserialize)]
struct DeviceCredential {
    device_id: String,
    token: String,
    server: String,
}

pub(crate) struct Device {
    pub token: String,
    pub server: String,
}

pub(crate) struct ConfigStore {
    path: PathBuf,
    config: Config,
}

impl ConfigStore {
    pub fn load_default() -> CoreResult<Self> {
        Self::load(config_path()?)
    }

    pub fn load(path: PathBuf) -> CoreResult<Self> {
        let config = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                CoreError::Config("could not read paired-device configuration".into())
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self { path, config })
    }

    pub fn device(&self, steam_id64: &str) -> Option<Device> {
        self.config.devices.get(steam_id64).map(|cred| Device {
            token: cred.token.clone(),
            server: cred.server.clone(),
        })
    }

    pub fn store_device(
        &mut self,
        steam_id64: &str,
        device_id: String,
        token: String,
        server: String,
    ) -> CoreResult<()> {
        if !valid_steam_id(steam_id64) || token.is_empty() || device_id.is_empty() {
            return Err(CoreError::Config(
                "server returned invalid device credentials".into(),
            ));
        }
        self.config.devices.insert(
            steam_id64.to_owned(),
            DeviceCredential {
                device_id,
                token,
                server,
            },
        );
        self.save()
    }

    fn save(&self) -> CoreResult<()> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| CoreError::Config("invalid configuration path".into()))?;
        fs::create_dir_all(parent)?;
        secure_directory(parent)?;
        let bytes = serde_json::to_vec_pretty(&self.config).map_err(|_| {
            CoreError::Config("could not encode paired-device configuration".into())
        })?;
        let temp = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        secure_file(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        if let Err(error) = fs::rename(&temp, &self.path) {
            let _ = fs::remove_file(&temp);
            return Err(error.into());
        }
        Ok(())
    }
}

pub(crate) fn config_path() -> CoreResult<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| CoreError::Config("Windows LOCALAPPDATA is unavailable".into()))?;

    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or_else(|| CoreError::Config("user configuration directory is unavailable".into()))?;

    Ok(base.join("OPSearch").join("Companion").join("config.json"))
}

#[cfg(unix)]
fn secure_directory(path: &Path) -> CoreResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
#[cfg(not(unix))]
fn secure_directory(_path: &Path) -> CoreResult<()> {
    Ok(())
}

#[cfg(unix)]
fn secure_file(path: &Path) -> CoreResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}
#[cfg(not(unix))]
fn secure_file(_path: &Path) -> CoreResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_remain_bound_to_their_own_server() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut store = ConfigStore::load(path.clone()).unwrap();
        store
            .store_device(
                "76561198000000000",
                "device-a".into(),
                "secret-a".into(),
                "https://a.example".into(),
            )
            .unwrap();
        store
            .store_device(
                "76561198000000001",
                "device-b".into(),
                "secret-b".into(),
                "https://b.example".into(),
            )
            .unwrap();
        let store = ConfigStore::load(path).unwrap();
        let a = store.device("76561198000000000").unwrap();
        let b = store.device("76561198000000001").unwrap();
        assert_eq!(
            (a.token.as_str(), a.server.as_str()),
            ("secret-a", "https://a.example")
        );
        assert_eq!(
            (b.token.as_str(), b.server.as_str()),
            ("secret-b", "https://b.example")
        );
    }
}
