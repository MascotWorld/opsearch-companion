use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::Url;
use reqwest::blocking::{Client, Response};
use reqwest::redirect::Policy;
use serde::Deserialize;
use serde::Serialize;

use super::steam::valid_steam_id;
use super::types::{CoreError, CoreResult, Roster};

pub const DEFAULT_SERVER: &str = "https://opsearch.pro";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: u64 = 32 * 1024;
const MAX_PLAYERS: usize = 64;

pub(crate) fn validate_server(raw: &str) -> CoreResult<String> {
    let url = Url::parse(raw).map_err(|_| CoreError::InvalidInput("invalid service URL".into()))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(CoreError::InvalidInput(
            "service URL must be an origin without credentials, path, query, or fragment".into(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| CoreError::InvalidInput("service URL must include a host".into()))?;
    let is_loopback =
        host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1";
    if url.scheme() != "https" && !(url.scheme() == "http" && is_loopback) {
        return Err(CoreError::InvalidInput(
            "service URL must use HTTPS (HTTP is allowed only for localhost testing)".into(),
        ));
    }
    Ok(url.origin().ascii_serialization())
}

pub(crate) struct ApiClient {
    http: Client,
}

impl ApiClient {
    pub fn new() -> CoreResult<Self> {
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(Policy::none())
            .build()
            .map_err(|_| CoreError::Network)?;
        Ok(Self { http })
    }

    /// Asks the server to link this computer to the Steam account read locally. The returned
    /// token stays inert until the account's owner approves the request in the browser.
    pub fn request_link(
        &self,
        server: &str,
        steam_id64: &str,
        device_name: &str,
    ) -> CoreResult<LinkRequest> {
        let origin = validate_server(server)?;
        if !valid_steam_id(steam_id64) {
            return Err(CoreError::InvalidInput("Steam account is invalid".into()));
        }
        let request = LinkRequestBody {
            steamid64: steam_id64,
            device_name,
            client_version: env!("CARGO_PKG_VERSION"),
        };
        let response = self
            .http
            .post(format!("{origin}/api/cs2/companion/links"))
            .json(&request)
            .send()
            .map_err(|_| CoreError::Network)?;
        let body = successful_body(response)?;
        let result: LinkRequest = serde_json::from_slice(&body).map_err(|_| {
            CoreError::InvalidInput("server returned an invalid link response".into())
        })?;
        if result.token.is_empty() || !valid_link_id(&result.link_id) {
            return Err(CoreError::InvalidInput(
                "server returned an invalid link response".into(),
            ));
        }
        Ok(result)
    }

    pub fn link_status(&self, server: &str, link_id: &str, token: &str) -> CoreResult<LinkStatus> {
        let origin = validate_server(server)?;
        if !valid_link_id(link_id) {
            return Err(CoreError::InvalidInput("link id is invalid".into()));
        }
        let response = self
            .http
            .get(format!("{origin}/api/cs2/companion/links/{link_id}/status"))
            .bearer_auth(token)
            .send()
            .map_err(|_| CoreError::Network)?;
        let body = successful_body(response)?;
        serde_json::from_slice(&body)
            .map_err(|_| CoreError::InvalidInput("server returned an invalid link status".into()))
    }

    pub fn send_roster(
        &self,
        server: &str,
        token: &str,
        roster: &Roster,
        observed_at: DateTime<Utc>,
    ) -> CoreResult<()> {
        let origin = validate_server(server)?;
        if token.is_empty() {
            return Err(CoreError::InvalidInput(
                "paired device credential is missing".into(),
            ));
        }
        validate_roster(roster)?;
        let players = roster
            .players
            .iter()
            .map(|player| SnapshotPlayer {
                steamid64: &player.steamid64,
                playtime: player.playtime,
                app_id: player.app_id,
            })
            .collect::<Vec<_>>();
        let request = SnapshotRequest {
            source_steamid64: &roster.owner_steamid64,
            local_steamid64: &roster.owner_steamid64,
            observed_at: observed_at.to_rfc3339_opts(SecondsFormat::Micros, true),
            owner_playtime: roster.owner_playtime,
            players,
            client_version: env!("CARGO_PKG_VERSION"),
        };
        let response = self
            .http
            .post(format!("{origin}/api/cs2/companion/snapshots"))
            .bearer_auth(token)
            .json(&request)
            .send()
            .map_err(|_| CoreError::Network)?;
        let _ = successful_body(response)?;
        Ok(())
    }
}

#[derive(Serialize)]
struct LinkRequestBody<'a> {
    steamid64: &'a str,
    device_name: &'a str,
    client_version: &'a str,
}

#[derive(Deserialize)]
pub(crate) struct LinkRequest {
    pub link_id: String,
    pub token: String,
}

#[derive(Deserialize)]
pub(crate) struct LinkStatus {
    pub status: String,
    pub device_id: Option<String>,
}

/// Link ids are server UUIDs; anything else must not reach a URL path.
fn valid_link_id(id: &str) -> bool {
    id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

#[derive(Serialize)]
struct SnapshotRequest<'a> {
    source_steamid64: &'a str,
    local_steamid64: &'a str,
    observed_at: String,
    owner_playtime: i64,
    players: Vec<SnapshotPlayer<'a>>,
    client_version: &'a str,
}

#[derive(Serialize)]
struct SnapshotPlayer<'a> {
    steamid64: &'a str,
    playtime: i64,
    app_id: u32,
}

fn successful_body(response: Response) -> CoreResult<Vec<u8>> {
    let status = response.status();
    if !status.is_success() {
        return Err(CoreError::HttpStatus(status.as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES)
    {
        return Err(CoreError::InvalidInput(
            "server response exceeded the size limit".into(),
        ));
    }
    use std::io::Read;
    let mut body = Vec::new();
    response
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|_| CoreError::Network)?;
    if body.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(CoreError::InvalidInput(
            "server response exceeded the size limit".into(),
        ));
    }
    Ok(body)
}

fn validate_roster(roster: &Roster) -> CoreResult<()> {
    if !valid_steam_id(&roster.owner_steamid64)
        || !(1..=i32::MAX as i64).contains(&roster.owner_playtime)
        || roster.players.len() > MAX_PLAYERS
    {
        return Err(CoreError::InvalidInput(
            "roster does not satisfy the service limits".into(),
        ));
    }
    if roster.players.iter().any(|player| {
        !valid_steam_id(&player.steamid64)
            || player.steamid64 == roster.owner_steamid64
            || player.app_id != 730
            || !(1..=i32::MAX as i64).contains(&player.playtime)
    }) {
        return Err(CoreError::InvalidInput(
            "roster contains invalid player data".into(),
        ));
    }
    Ok(())
}

pub(crate) fn monotonic_observed_at(
    now: DateTime<Utc>,
    previous: Option<DateTime<Utc>>,
) -> DateTime<Utc> {
    previous.map_or(now, |last| {
        if now > last {
            now
        } else {
            last + chrono::Duration::microseconds(1)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;
    use reqwest::blocking::Client;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn validates_origins_without_credentials_or_redirect_targets() {
        assert_eq!(
            validate_server("https://opsearch.pro/").unwrap(),
            "https://opsearch.pro"
        );
        assert_eq!(
            validate_server("http://127.0.0.1:4000").unwrap(),
            "http://127.0.0.1:4000"
        );
        for bad in [
            "http://example.test",
            "https://u:p@example.test",
            "https://example.test/path",
            "https://example.test/?key=x",
            "https://example.test/#frag",
        ] {
            assert!(validate_server(bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn bearer_auth_is_not_forwarded_through_redirects() {
        let second = TcpListener::bind("127.0.0.1:0").unwrap();
        let second_addr = second.local_addr().unwrap();
        second.set_nonblocking(true).unwrap();
        let second_thread = thread::spawn(move || {
            for _ in 0..100 {
                match second.accept() {
                    Ok((mut stream, _)) => {
                        let mut request = [0u8; 4096];
                        let count = stream.read(&mut request).unwrap();
                        let request = String::from_utf8_lossy(&request[..count]);
                        return request.to_ascii_lowercase().contains("authorization:");
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => return true,
                }
            }
            false
        });
        let first = TcpListener::bind("127.0.0.1:0").unwrap();
        let first_addr = first.local_addr().unwrap();
        let first_thread = thread::spawn(move || {
            let (mut stream, _) = first.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{second_addr}/leak\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        let client = Client::builder().redirect(Policy::none()).build().unwrap();
        let response = client
            .post(format!("http://{first_addr}/api"))
            .bearer_auth("no-secret-forward")
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::FOUND);
        first_thread.join().unwrap();
        assert!(
            !second_thread.join().unwrap(),
            "request was forwarded to redirect target"
        );
    }

    #[test]
    fn observed_time_is_strictly_monotonic() {
        let now = Utc::now();
        let next = monotonic_observed_at(now, Some(now));
        assert!(next > now);
    }
}
