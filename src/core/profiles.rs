use std::io::Read;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::redirect::Policy;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PROFILE_BYTES: u64 = 256 * 1024;
const MAX_AVATAR_BYTES: u64 = 512 * 1024;
const AVATAR_SIZE: u32 = 64;

/// A Steam player's public name and avatar, as their community profile shows them.
#[derive(Clone, Debug, PartialEq)]
pub struct SteamProfile {
    pub steamid64: String,
    pub name: Option<String>,
    /// Square RGBA pixels, `AVATAR_SIZE` on each side.
    pub avatar: Option<Vec<u8>>,
}

impl SteamProfile {
    pub const AVATAR_SIZE: u32 = AVATAR_SIZE;
}

/// Loads public Steam profiles one at a time in the background. Steam's community XML needs
/// no API key; a profile that fails to load comes back without a name or avatar, and the
/// app shows the SteamID instead.
pub struct ProfileLoader {
    requests: Sender<String>,
    loaded: Receiver<SteamProfile>,
}

impl ProfileLoader {
    pub fn spawn() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<String>();
        let (loaded_tx, loaded_rx) = mpsc::channel();
        let _ = thread::Builder::new()
            .name("opsearch-companion-profiles".into())
            .spawn(move || {
                let http = client().ok();
                for steamid64 in request_rx {
                    let profile = match &http {
                        Some(http) => load(http, &steamid64),
                        None => SteamProfile {
                            steamid64,
                            name: None,
                            avatar: None,
                        },
                    };
                    if loaded_tx.send(profile).is_err() {
                        return;
                    }
                }
            });
        Self {
            requests: request_tx,
            loaded: loaded_rx,
        }
    }

    pub fn request(&self, steamid64: &str) {
        if steamid64.len() == 17 && steamid64.bytes().all(|b| b.is_ascii_digit()) {
            let _ = self.requests.send(steamid64.to_owned());
        }
    }

    pub fn poll(&self) -> Vec<SteamProfile> {
        self.loaded.try_iter().collect()
    }
}

fn client() -> reqwest::Result<Client> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(Policy::limited(3))
        .user_agent(concat!("opsearch-companion/", env!("CARGO_PKG_VERSION")))
        .build()
}

fn load(http: &Client, steamid64: &str) -> SteamProfile {
    let xml = fetch(
        http,
        &format!("https://steamcommunity.com/profiles/{steamid64}/?xml=1"),
        MAX_PROFILE_BYTES,
    )
    .and_then(|body| String::from_utf8(body).ok());
    let name = xml.as_deref().and_then(|xml| tag(xml, "steamID"));
    let avatar = xml
        .as_deref()
        .and_then(|xml| tag(xml, "avatarMedium"))
        .filter(|url| steam_cdn(url))
        .and_then(|url| fetch(http, &url, MAX_AVATAR_BYTES))
        .and_then(|bytes| decode_avatar(&bytes));
    SteamProfile {
        steamid64: steamid64.to_owned(),
        name: name.filter(|name| !name.trim().is_empty()),
        avatar,
    }
}

fn fetch(http: &Client, url: &str, limit: u64) -> Option<Vec<u8>> {
    let response = http.get(url).send().ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut body = Vec::new();
    response.take(limit).read_to_end(&mut body).ok()?;
    Some(body)
}

/// The text of the first `<name>` element, with a CDATA wrapper removed.
fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = start + xml[start..].find(&format!("</{name}>"))?;
    let value = xml[start..end].trim();
    let value = value
        .strip_prefix("<![CDATA[")
        .and_then(|v| v.strip_suffix("]]>"))
        .unwrap_or(value);
    Some(unescape(value.trim()))
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Avatars are only fetched from Steam's own image hosts.
fn steam_cdn(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or_default();
    host.ends_with(".steamstatic.com") || host == "steamcdn-a.akamaihd.net"
}

fn decode_avatar(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = image.resize_exact(
        AVATAR_SIZE,
        AVATAR_SIZE,
        image::imageops::FilterType::Triangle,
    );
    Some(image.to_rgba8().into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_name_and_avatar_from_community_xml() {
        let xml = "<profile><steamID64>76561198000000001</steamID64>\
            <steamID><![CDATA[ s1mple &amp; co ]]></steamID>\
            <avatarMedium><![CDATA[https://avatars.steamstatic.com/abc_medium.jpg]]></avatarMedium>\
            </profile>";
        assert_eq!(tag(xml, "steamID").as_deref(), Some("s1mple & co"));
        assert_eq!(
            tag(xml, "avatarMedium").as_deref(),
            Some("https://avatars.steamstatic.com/abc_medium.jpg")
        );
        assert_eq!(tag(xml, "missing"), None);
    }

    #[test]
    fn avatars_come_only_from_steam_hosts() {
        assert!(steam_cdn("https://avatars.steamstatic.com/a.jpg"));
        assert!(steam_cdn("https://avatars.fastly.steamstatic.com/a.jpg"));
        assert!(!steam_cdn("http://avatars.steamstatic.com/a.jpg"));
        assert!(!steam_cdn("https://steamstatic.com.evil.example/a.jpg"));
    }
}
