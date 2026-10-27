use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use super::api::{self, ApiClient};
use super::config::ConfigStore;
use super::steam;
use super::types::{Command, CoreError, CoreResult, Event, Roster, Status};

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const RETRY_START: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(60);
const LINK_POLL_INTERVAL: Duration = Duration::from_secs(2);
const LINK_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// A link request waiting for the owner to approve it in the browser.
struct PendingLink {
    origin: String,
    steam_id64: String,
    link_id: String,
    token: String,
    deadline: Instant,
    next_check: Instant,
}

pub struct CoreHandle {
    commands: Sender<Command>,
    events: Receiver<Event>,
}

impl CoreHandle {
    pub fn new() -> CoreResult<Self> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        thread::Builder::new()
            .name("opsearch-companion-core".into())
            .spawn(move || worker_loop(command_rx, event_tx))
            .map_err(CoreError::Io)?;
        Ok(Self {
            commands: command_tx,
            events: event_rx,
        })
    }

    pub fn send(&self, command: Command) -> CoreResult<()> {
        self.commands
            .send(command)
            .map_err(|_| CoreError::WorkerStopped)
    }

    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

impl Drop for CoreHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

struct MonitorState {
    running: bool,
    owner: Option<String>,
    last_roster_hash: Option<[u8; 32]>,
    last_upload: Option<Instant>,
    pending_hash: Option<[u8; 32]>,
    retry_at: Option<Instant>,
    retry_delay: Duration,
    last_observed_at: Option<DateTime<Utc>>,
    last_fingerprints: Option<std::collections::HashMap<std::path::PathBuf, [u8; 32]>>,
    cached_roster: Option<Roster>,
    parse_pending: bool,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            running: false,
            owner: None,
            last_roster_hash: None,
            last_upload: None,
            pending_hash: None,
            retry_at: None,
            retry_delay: RETRY_START,
            last_observed_at: None,
            last_fingerprints: None,
            cached_roster: None,
            parse_pending: true,
        }
    }
}

fn worker_loop(commands: Receiver<Command>, events: Sender<Event>) {
    let _ = events.send(Event::Status(Status::Idle));
    let Ok(api_client) = ApiClient::new() else {
        emit_error(&events, CoreError::Network);
        return;
    };
    let mut config = match ConfigStore::load_default() {
        Ok(config) => Some(config),
        Err(error) => {
            emit_error(&events, error);
            None
        }
    };
    let mut state = MonitorState::default();
    let mut next_poll = Instant::now();
    let mut pending_link: Option<PendingLink> = None;

    loop {
        let timeout = next_poll
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(250));
        match commands.recv_timeout(timeout) {
            Ok(Command::Shutdown) => break,
            Ok(Command::Start) => {
                state.running = true;
                next_poll = Instant::now();
            }
            Ok(Command::Stop) => {
                state = MonitorState::default();
                let _ = events.send(Event::Status(Status::Stopped));
            }
            Ok(Command::Link { server }) => match request_link(&api_client, &server) {
                Ok(link) => {
                    let _ = events.send(Event::Status(Status::Linking));
                    let _ = events.send(Event::OpenUrl(format!(
                        "{}/companion/link/{}",
                        link.origin, link.link_id
                    )));
                    pending_link = Some(link);
                }
                Err(error) => emit_error(&events, error),
            },
            Ok(Command::CancelLink) => {
                pending_link = None;
                next_poll = Instant::now();
            }
            Ok(Command::Inspect { path, owner }) => {
                let result = owner
                    .or_else(|| steam::filename_owner(&path))
                    .ok_or_else(|| {
                        CoreError::InvalidInput(
                            "owner SteamID64 is required for this VDF file".into(),
                        )
                    })
                    .and_then(|owner| steam::read_roster(&path, &owner, SystemTime::now()));
                match result {
                    Ok(roster) => {
                        let _ = events.send(Event::Roster(roster));
                    }
                    Err(error) => emit_error(&events, error),
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        if let Some(link) = pending_link.as_mut()
            && Instant::now() >= link.next_check
        {
            link.next_check = Instant::now() + LINK_POLL_INTERVAL;
            match check_link(&api_client, &mut config, link) {
                Ok(true) => {
                    let steam_id64 = link.steam_id64.clone();
                    pending_link = None;
                    let _ = events.send(Event::PairComplete { steam_id64 });
                    next_poll = Instant::now();
                }
                Ok(false) => {}
                Err(error) => {
                    pending_link = None;
                    emit_error(&events, error);
                    next_poll = Instant::now();
                }
            }
        }

        if state.running && pending_link.is_none() && Instant::now() >= next_poll {
            poll_once(&api_client, &mut config, &events, &mut state);
            next_poll = Instant::now() + POLL_INTERVAL;
        }
    }
    let _ = events.send(Event::Status(Status::Stopped));
}

fn request_link(api: &ApiClient, server: &str) -> CoreResult<PendingLink> {
    let origin = api::validate_server(server)?;
    let account = steam::active_account()?;
    let device_name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "Windows PC".into());
    let response = api.request_link(&origin, &account.steam_id64, &device_name)?;
    Ok(PendingLink {
        origin,
        steam_id64: account.steam_id64,
        link_id: response.link_id,
        token: response.token,
        deadline: Instant::now() + LINK_TIMEOUT,
        next_check: Instant::now() + LINK_POLL_INTERVAL,
    })
}

/// Returns true once the owner has approved the link and the device is stored.
fn check_link(
    api: &ApiClient,
    store: &mut Option<ConfigStore>,
    link: &PendingLink,
) -> CoreResult<bool> {
    if Instant::now() >= link.deadline {
        return Err(CoreError::LinkExpired);
    }
    let status = match api.link_status(&link.origin, &link.link_id, &link.token) {
        Ok(status) => status,
        // A dropped connection while the user is in the browser is not fatal; try again.
        Err(CoreError::Network) => return Ok(false),
        Err(error) => return Err(error),
    };
    match status.status.as_str() {
        "pending" => Ok(false),
        "expired" => Err(CoreError::LinkExpired),
        "approved" => {
            let device_id = status.device_id.ok_or_else(|| {
                CoreError::InvalidInput("server approved the link without a device".into())
            })?;
            let config = if let Some(config) = store.as_mut() {
                config
            } else {
                *store = Some(ConfigStore::load_default()?);
                store.as_mut().expect("config was just initialized")
            };
            config.store_device(
                &link.steam_id64,
                device_id,
                link.token.clone(),
                link.origin.clone(),
            )?;
            Ok(true)
        }
        _ => Err(CoreError::InvalidInput("unknown link status".into())),
    }
}

fn poll_once(
    api: &ApiClient,
    store: &mut Option<ConfigStore>,
    events: &Sender<Event>,
    state: &mut MonitorState,
) {
    let account = match steam::active_account() {
        Ok(account) => account,
        Err(_) => {
            if state.owner.take().is_some() {
                reset_account_state(state);
            }
            let _ = events.send(Event::Status(Status::SignedOut));
            return;
        }
    };
    if state.owner.as_deref() != Some(&account.steam_id64) {
        state.owner = Some(account.steam_id64.clone());
        reset_account_state(state);
    }

    let config = match store {
        Some(config) => config,
        None => match ConfigStore::load_default() {
            Ok(config) => {
                *store = Some(config);
                store.as_mut().expect("config was just initialized")
            }
            Err(error) => {
                emit_error(events, error);
                return;
            }
        },
    };
    let Some(device) = config.device(&account.steam_id64) else {
        let _ = events.send(Event::Status(Status::NeedsPairing {
            steam_id64: account.steam_id64,
        }));
        return;
    };

    let config_dir = account.steam_root.join("config");
    let file_set_changed = match steam::fingerprint_coplay_files(&config_dir) {
        Ok(fingerprints) => {
            let changed = state.last_fingerprints.as_ref() != Some(&fingerprints);
            state.last_fingerprints = Some(fingerprints);
            state.parse_pending |= changed;
            changed
        }
        Err(error) => {
            emit_error(events, error);
            false
        }
    };
    if file_set_changed || state.cached_roster.is_none() || state.parse_pending {
        let path = active_coplay_path(&config_dir, &account.steam_id64);
        match steam::read_roster(&path, &account.steam_id64, SystemTime::now()) {
            Ok(roster) => {
                state.cached_roster = Some(roster);
                state.parse_pending = false;
            }
            Err(error) => {
                state.cached_roster = None;
                state.parse_pending = true;
                let _ = events.send(Event::Status(Status::Watching {
                    steam_id64: account.steam_id64,
                }));
                emit_error(events, error);
                return;
            }
        }
    }
    let Some(roster) = state.cached_roster.clone() else {
        return;
    };
    let hash = roster_hash(&roster);
    let content_changed = state.last_roster_hash != Some(hash);
    if content_changed && state.pending_hash != Some(hash) {
        state.pending_hash = Some(hash);
        state.retry_delay = RETRY_START;
        state.retry_at = Some(Instant::now());
    }
    let heartbeat = state
        .last_upload
        .is_none_or(|sent| sent.elapsed() >= HEARTBEAT_INTERVAL);
    let retry_ready = state.retry_at.is_none_or(|at| Instant::now() >= at);
    let should_send = (content_changed || heartbeat || state.pending_hash.is_some()) && retry_ready;

    if should_send {
        // Recheck Steam identity directly before sending; a fast account switch must
        // never put one profile's local roster on another profile's pairing.
        let still_active = matches!(steam::active_account(), Ok(current) if current.steam_id64 == account.steam_id64);
        if !still_active {
            reset_account_state(state);
            state.owner = None;
            let _ = events.send(Event::Status(Status::SignedOut));
            return;
        }
        let now = Utc::now();
        let observed = api::monotonic_observed_at(now, state.last_observed_at);
        match api.send_roster(&device.server, &device.token, &roster, observed) {
            Ok(()) => {
                state.last_roster_hash = Some(hash);
                state.last_upload = Some(Instant::now());
                state.pending_hash = None;
                state.retry_at = None;
                state.retry_delay = RETRY_START;
                state.last_observed_at = Some(observed);
                let _ = events.send(Event::Roster(roster));
            }
            Err(error) => {
                state.retry_at = Some(Instant::now() + state.retry_delay);
                state.retry_delay = (state.retry_delay * 2).min(RETRY_MAX);
                emit_error(events, error);
            }
        }
    }
    let _ = events.send(Event::Status(Status::Watching {
        steam_id64: account.steam_id64,
    }));
}

fn active_coplay_path(config_dir: &std::path::Path, owner: &str) -> std::path::PathBuf {
    let exact = config_dir.join(format!("coplay_{owner}.vdf"));
    if exact.exists() {
        return exact;
    }
    std::fs::read_dir(config_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| steam::filename_owner(path).as_deref() == Some(owner))
        .unwrap_or(exact)
}

fn reset_account_state(state: &mut MonitorState) {
    state.last_roster_hash = None;
    state.last_upload = None;
    state.pending_hash = None;
    state.retry_at = None;
    state.retry_delay = RETRY_START;
    state.last_observed_at = None;
    state.last_fingerprints = None;
    state.cached_roster = None;
    state.parse_pending = true;
}

fn roster_hash(roster: &Roster) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(roster.owner_steamid64.as_bytes());
    digest.update(roster.owner_playtime.to_le_bytes());
    for player in &roster.players {
        digest.update(player.steamid64.as_bytes());
        digest.update(player.playtime.to_le_bytes());
        digest.update(player.app_id.to_le_bytes());
    }
    digest.finalize().into()
}

fn emit_error(events: &Sender<Event>, error: CoreError) {
    let _ = events.send(Event::Error(error.to_string()));
}
