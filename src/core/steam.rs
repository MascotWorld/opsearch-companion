use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use super::types::{CoreError, CoreResult, Player, Roster};
use super::vdf::{self, Node, Value};

pub(crate) const CS2_APP_ID: u32 = 730;
const COHORT_WINDOW_SECONDS: i64 = 120;
const MAX_UNIX_PLAYTIME: i64 = i32::MAX as i64;
const MAX_VDF_BYTES: u64 = 8 * 1024 * 1024;
const STEAM64_BASE: u64 = 76_561_197_960_265_728;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Account {
    pub steam_id64: String,
    pub steam_root: PathBuf,
}

#[cfg(windows)]
pub(crate) fn active_account() -> CoreResult<Account> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let active = current_user
        .open_subkey_with_flags(r"Software\Valve\Steam\ActiveProcess", KEY_READ)
        .map_err(|_| {
            CoreError::InvalidInput("Steam is not signed in for this Windows user".into())
        })?;
    let account_id: u32 = active.get_value("ActiveUser").map_err(|_| {
        CoreError::InvalidInput("Steam is not signed in for this Windows user".into())
    })?;
    if account_id == 0 {
        return Err(CoreError::InvalidInput(
            "Steam is not signed in for this Windows user".into(),
        ));
    }

    let steam_key = current_user
        .open_subkey_with_flags(r"Software\Valve\Steam", KEY_READ)
        .map_err(|_| {
            CoreError::InvalidInput(
                "Steam install path is missing from the current user's registry".into(),
            )
        })?;
    let root: String = steam_key.get_value("SteamPath").map_err(|_| {
        CoreError::InvalidInput(
            "Steam install path is missing from the current user's registry".into(),
        )
    })?;
    if root.trim().is_empty() {
        return Err(CoreError::InvalidInput(
            "Steam install path is empty".into(),
        ));
    }

    Ok(Account {
        steam_id64: (STEAM64_BASE + u64::from(account_id)).to_string(),
        steam_root: PathBuf::from(root),
    })
}

#[cfg(not(windows))]
pub(crate) fn active_account() -> CoreResult<Account> {
    Err(CoreError::WindowsOnly)
}

#[cfg(test)]
fn steam_id_from_account_id(account_id: u32) -> Option<String> {
    (account_id != 0).then(|| (STEAM64_BASE + u64::from(account_id)).to_string())
}

pub(crate) fn valid_steam_id(id: &str) -> bool {
    let Ok(value) = id.parse::<u64>() else {
        return false;
    };
    (STEAM64_BASE..=STEAM64_BASE + u64::from(u32::MAX)).contains(&value)
}

pub(crate) fn filename_owner(path: &Path) -> Option<String> {
    let filename = path.file_name()?.to_str()?;
    let stem = filename
        .strip_suffix(".vdf")
        .or_else(|| filename.strip_suffix(".VDF"))?;
    let id = stem
        .strip_prefix("coplay_")
        .or_else(|| stem.strip_prefix("Coplay_"))?;
    valid_steam_id(id).then(|| id.to_owned())
}

pub(crate) fn fingerprint_coplay_files(
    config_dir: &Path,
) -> CoreResult<HashMap<PathBuf, [u8; 32]>> {
    let mut fingerprints = HashMap::new();
    let Ok(entries) = fs::read_dir(config_dir) else {
        return Ok(fingerprints);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if filename_owner(&path).is_none() {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_file() {
            continue;
        }
        let Ok(before) = entry.metadata() else {
            continue;
        };
        if before.len() == 0 || before.len() > MAX_VDF_BYTES {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(after) = fs::metadata(&path) else {
            continue;
        };
        if before.len() != after.len()
            || bytes.len() as u64 != after.len()
            || before.modified().ok() != after.modified().ok()
        {
            continue;
        }
        fingerprints.insert(path, Sha256::digest(bytes).into());
    }
    Ok(fingerprints)
}

pub(crate) fn read_roster(path: &Path, owner: &str, now: SystemTime) -> CoreResult<Roster> {
    if !valid_steam_id(owner) {
        return Err(CoreError::InvalidInput("invalid owner SteamID64".into()));
    }
    let metadata = fs::metadata(path)?;
    if metadata.len() == 0 || metadata.len() > MAX_VDF_BYTES {
        return Err(CoreError::InvalidInput(
            "Coplay VDF is empty or exceeds 8 MiB".into(),
        ));
    }
    let bytes = fs::read(path)?;
    let metadata_after = fs::metadata(path)?;
    if metadata.len() != metadata_after.len()
        || bytes.len() as u64 != metadata_after.len()
        || metadata.modified().ok() != metadata_after.modified().ok()
    {
        return Err(CoreError::InvalidInput(
            "Coplay VDF changed while being read".into(),
        ));
    }
    parse_roster(&bytes, owner, now)
}

pub(crate) fn parse_roster(bytes: &[u8], owner: &str, now: SystemTime) -> CoreResult<Roster> {
    if !valid_steam_id(owner) {
        return Err(CoreError::InvalidInput("invalid owner SteamID64".into()));
    }
    let doc = vdf::decode(bytes)
        .map_err(|e| CoreError::InvalidInput(format!("could not parse Steam Coplay VDF: {e}")))?;
    let root = doc
        .get("coplay")
        .and_then(as_object)
        .ok_or_else(|| CoreError::InvalidInput("Steam Coplay root is missing".into()))?;

    #[derive(Clone)]
    struct Row {
        id: String,
        app: u64,
        playtime: i64,
    }
    let mut rows = Vec::with_capacity(root.pairs.len());
    let mut owner_playtime = 0i64;
    for (_, value) in &root.pairs {
        let Some(entry) = as_object(value) else {
            continue;
        };
        let Some(id) = entry
            .get("SteamID")
            .and_then(as_string)
            .and_then(parse_steam_id)
        else {
            continue;
        };
        let Some(app) = entry
            .get("gameid")
            .and_then(as_string)
            .and_then(|v| v.parse::<u64>().ok())
        else {
            continue;
        };
        let Some(playtime) = entry
            .get("Playtime")
            .and_then(as_string)
            .and_then(|v| v.parse::<i64>().ok())
        else {
            continue;
        };
        if !(1..=MAX_UNIX_PLAYTIME).contains(&playtime) {
            continue;
        }
        if id == owner && app == u64::from(CS2_APP_ID) {
            owner_playtime = owner_playtime.max(playtime);
        }
        rows.push(Row { id, app, playtime });
    }
    let current_unix = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    if owner_playtime > current_unix.saturating_add(60) {
        return Err(CoreError::InvalidInput(
            "owner CS2 timestamp is more than one minute in the future".into(),
        ));
    }

    // The lobby is anchored on the newest CS2 entry, not only on the owner's own. Steam writes
    // the people you are playing with while the match is on, but moves the owner's entry
    // forward only once it ends; anchoring on the owner alone kept showing the previous lobby
    // for the whole match.
    let newest_peer = rows
        .iter()
        .filter(|row| {
            row.app == u64::from(CS2_APP_ID)
                && row.id != owner
                && row.playtime <= current_unix.saturating_add(60)
        })
        .map(|row| row.playtime)
        .max()
        .unwrap_or(0);
    let owner_playtime = owner_playtime.max(newest_peer);
    if owner_playtime == 0 {
        return Err(CoreError::InvalidInput(
            "no CS2 entries in Coplay VDF".into(),
        ));
    }

    let mut dedup = HashMap::<String, i64>::new();
    for row in rows {
        if row.app != u64::from(CS2_APP_ID)
            || row.id == owner
            || (row.playtime - owner_playtime).abs() > COHORT_WINDOW_SECONDS
            || row.playtime > current_unix.saturating_add(60)
        {
            continue;
        }
        dedup
            .entry(row.id)
            .and_modify(|at| *at = (*at).max(row.playtime))
            .or_insert(row.playtime);
    }
    let mut players = dedup
        .into_iter()
        .map(|(steamid64, playtime)| Player {
            steamid64,
            playtime,
            app_id: CS2_APP_ID,
        })
        .collect::<Vec<_>>();
    players.sort_by(|a, b| {
        (a.playtime - owner_playtime)
            .abs()
            .cmp(&(b.playtime - owner_playtime).abs())
            .then_with(|| a.steamid64.cmp(&b.steamid64))
    });
    players.truncate(64);

    Ok(Roster {
        owner_steamid64: owner.to_owned(),
        owner_playtime,
        observed_at: now,
        players,
    })
}

fn as_object(value: &Value) -> Option<&Node> {
    match value {
        Value::Object(node) => Some(node),
        Value::String(_) => None,
    }
}
fn as_string(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) => Some(text),
        Value::Object(_) => None,
    }
}
fn parse_steam_id(value: &str) -> Option<String> {
    valid_steam_id(value).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn fixture(owner: u64, peer: u64, old: u64, owner_at: i32) -> Vec<u8> {
        let mut b = Vec::new();
        fn z(b: &mut Vec<u8>, s: &str) {
            b.extend_from_slice(s.as_bytes());
            b.push(0)
        }
        fn row(b: &mut Vec<u8>, index: &str, id: u64, at: i32) {
            b.push(0);
            z(b, index);
            b.push(7);
            z(b, "SteamID");
            b.extend_from_slice(&id.to_le_bytes());
            b.push(7);
            z(b, "gameid");
            b.extend_from_slice(&730u64.to_le_bytes());
            b.push(2);
            z(b, "Playtime");
            b.extend_from_slice(&at.to_le_bytes());
            b.push(8)
        }
        b.push(0);
        z(&mut b, "coplay");
        row(&mut b, "0", owner, owner_at);
        row(&mut b, "1", peer, owner_at - 27);
        row(&mut b, "2", old, owner_at - 136_143);
        b.extend_from_slice(&[8, 8]);
        b
    }

    #[test]
    fn extracts_only_owner_anchored_cs2_cohort_and_keeps_old_timestamp() {
        let owner_at = 1_793_094_741i32;
        let bytes = fixture(
            76_561_198_000_000_000,
            76_561_198_000_000_001,
            76_561_198_000_000_002,
            owner_at,
        );
        let now = UNIX_EPOCH + Duration::from_secs(owner_at as u64);
        let roster = parse_roster(&bytes, "76561198000000000", now).unwrap();
        assert_eq!(roster.owner_playtime, i64::from(owner_at));
        assert_eq!(roster.players.len(), 1);
        assert_eq!(roster.players[0].playtime, i64::from(owner_at - 27));
        assert_eq!(roster.players[0].app_id, 730);
    }

    #[test]
    fn follows_the_match_in_progress_before_steam_moves_the_owner_entry() {
        // Mid-match: the new lobby is already in the file, the owner's entry still has the
        // previous match's time.
        let previous_match = 1_793_090_000i32;
        let now_at = 1_793_094_741i32;
        let mut b = fixture(
            76_561_198_000_000_000,
            76_561_198_000_000_001,
            76_561_198_000_000_002,
            previous_match,
        );
        // Replace the old row's timestamp with one from the match being played.
        let old = 76_561_198_000_000_002u64.to_le_bytes();
        let at = b.windows(8).rposition(|w| w == old).unwrap() + 8 + 1 + "gameid".len() + 1 + 8;
        let at = at + 1 + "Playtime".len() + 1;
        b[at..at + 4].copy_from_slice(&(now_at - 30).to_le_bytes());

        let now = UNIX_EPOCH + Duration::from_secs(now_at as u64);
        let roster = parse_roster(&b, "76561198000000000", now).unwrap();
        assert_eq!(roster.owner_playtime, i64::from(now_at - 30));
        assert_eq!(roster.players.len(), 1);
        assert_eq!(roster.players[0].steamid64, "76561198000000002");
    }

    #[test]
    fn rejects_owner_timestamp_outside_backend_future_tolerance() {
        let now = 1_793_094_741i32;
        let bytes = fixture(
            76_561_198_000_000_000,
            76_561_198_000_000_001,
            76_561_198_000_000_002,
            now + 61,
        );
        let when = UNIX_EPOCH + Duration::from_secs(now as u64);
        assert!(parse_roster(&bytes, "76561198000000000", when).is_err());
    }

    #[test]
    fn account_id_conversion_is_public_steam64() {
        assert_eq!(
            steam_id_from_account_id(1).as_deref(),
            Some("76561197960265729")
        );
        assert!(valid_steam_id("76561197960265729"));
        assert!(!valid_steam_id("123"));
    }

    #[test]
    fn fingerprints_every_coplay_file_and_detects_add_replace_remove() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("coplay_76561198000000000.vdf");
        let second = dir.path().join("coplay_76561198000000001.vdf");
        fs::write(&first, b"first").unwrap();
        let initial = fingerprint_coplay_files(dir.path()).unwrap();
        assert_eq!(initial.len(), 1);

        fs::write(&second, b"second").unwrap();
        let added = fingerprint_coplay_files(dir.path()).unwrap();
        assert_eq!(added.len(), 2);
        assert_ne!(initial, added);

        fs::write(&second, b"replacement").unwrap();
        let replaced = fingerprint_coplay_files(dir.path()).unwrap();
        assert_ne!(added, replaced);

        fs::remove_file(&first).unwrap();
        let removed = fingerprint_coplay_files(dir.path()).unwrap();
        assert_eq!(removed.len(), 1);
        assert_ne!(replaced, removed);
    }
}
