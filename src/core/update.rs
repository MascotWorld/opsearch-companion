use std::io::Read;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use serde::Deserialize;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const RELEASES_PAGE: &str = "https://github.com/MascotWorld/opsearch-companion/releases";
const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/MascotWorld/opsearch-companion/releases/latest";
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: u64 = 512 * 1024;

/// A published release that is newer than the running build.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateInfo {
    pub version: String,
    pub url: String,
}

/// Asks GitHub for the latest published release in the background: once at startup and then
/// every few hours, since the app is meant to stay open. Failures are silent; the check is a
/// convenience, and the app keeps working without it.
pub struct UpdateChecker {
    found: Receiver<UpdateInfo>,
}

impl UpdateChecker {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        let _ = thread::Builder::new()
            .name("opsearch-companion-update".into())
            .spawn(move || {
                let Ok(http) = client() else { return };
                loop {
                    if let Some(update) = latest_release(&http)
                        .and_then(|release| newer_release(CURRENT_VERSION, release))
                        && tx.send(update).is_err()
                    {
                        return;
                    }
                    thread::sleep(CHECK_INTERVAL);
                }
            });
        Self { found: rx }
    }

    pub fn poll(&self) -> Option<UpdateInfo> {
        self.found.try_iter().last()
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

fn client() -> reqwest::Result<Client> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        // No credentials are sent, so following GitHub's own redirects (a renamed repository)
        // is harmless; the opened page is still checked below.
        .redirect(Policy::limited(3))
        .user_agent(concat!("opsearch-companion/", env!("CARGO_PKG_VERSION")))
        .build()
}

fn latest_release(http: &Client) -> Option<Release> {
    let response = http
        .get(LATEST_RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut body = Vec::new();
    response
        .take(MAX_RESPONSE_BYTES)
        .read_to_end(&mut body)
        .ok()?;
    serde_json::from_slice(&body).ok()
}

fn newer_release(current: &str, release: Release) -> Option<UpdateInfo> {
    if release.draft || release.prerelease {
        return None;
    }
    let latest = parse_version(&release.tag_name)?;
    if latest <= parse_version(current)? {
        return None;
    }
    // The button opens whatever URL this is, so only the project's own release pages qualify.
    let url = if release.html_url.starts_with(RELEASES_PAGE) {
        release.html_url
    } else {
        RELEASES_PAGE.to_owned()
    };
    Some(UpdateInfo {
        version: format!("{}.{}.{}", latest.0, latest.1, latest.2),
        url,
    })
}

fn parse_version(raw: &str) -> Option<(u64, u64, u64)> {
    let core = raw.trim().trim_start_matches(['v', 'V']);
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.into(),
            html_url: format!("{RELEASES_PAGE}/tag/{tag}"),
            draft: false,
            prerelease: false,
        }
    }

    #[test]
    fn parses_tags_with_and_without_prefix() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.10.0"), Some((0, 10, 0)));
        assert_eq!(parse_version("v2.0.0-rc.1"), Some((2, 0, 0)));
        assert_eq!(parse_version("v1.2"), None);
        assert_eq!(parse_version("v1.2.3.4"), None);
        assert_eq!(parse_version("latest"), None);
    }

    #[test]
    fn reports_only_strictly_newer_releases() {
        assert_eq!(
            newer_release("0.2.0", release("v0.10.0")).map(|u| u.version),
            Some("0.10.0".into())
        );
        assert!(newer_release("0.2.0", release("v0.2.0")).is_none());
        assert!(newer_release("0.3.0", release("v0.2.9")).is_none());
    }

    #[test]
    fn ignores_prereleases_and_foreign_links() {
        let mut pre = release("v9.0.0");
        pre.prerelease = true;
        assert!(newer_release("0.2.0", pre).is_none());

        let mut foreign = release("v9.0.0");
        foreign.html_url = "https://example.com/download".into();
        assert_eq!(newer_release("0.2.0", foreign).unwrap().url, RELEASES_PAGE);
    }
}
