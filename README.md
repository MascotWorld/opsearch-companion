# OPSearch Companion

OPSearch Companion is a portable Windows desktop app that reads Steam's local recent-player cache for Counter-Strike 2 and sends the current recent-player roster to your OPSearch account. It runs in the background and checks for updates every five seconds.

The Windows x64 download is a single executable. It does not require Rust, Go, WebView2, or the Visual C++ Redistributable to be installed. There is no installer. The app does not use Steamworks, inject into a process, read game memory, or start a game.

## Get started

1. Sign in at [OPSearch Companion](https://opsearch.pro/companion) with Steam and choose **Create pairing code**. The code remains valid until it is used or you create a replacement code, and can be used once.
2. Download `opsearch-companion-windows-x64.exe` from [Releases](https://github.com/MascotWorld/opsearch-companion/releases).
3. Open the executable, paste the 12-character pairing code into the app, and click **Подключить** (Connect). Monitoring starts automatically after pairing.
4. Keep Steam signed in. The app watches for roster updates and reports its connection status in the window.

The companion reads the active Steam account from the current Windows user's Steam registry settings and binds the pairing to that account. Pair each Steam account you want to use separately. Credentials are stored per account and bound to the server that issued them. The credential file is saved under `%LOCALAPPDATA%\OPSearch\Companion\config.json`; keep access to your Windows account protected. The token is a device credential for your OPSearch account.

## What it reads and sends

Every five seconds, the app reads and hashes the bytes of files matching Steam's `config\coplay_*.vdf` pattern to notice additions, replacements, and changes. It parses and uploads a roster only from the file whose owner matches the currently signed-in and paired Steam account; it does not parse or upload the other accounts' roster contents. It pauses uploads while Steam is signed out and switches to the corresponding pairing after an account change.

For the active account, the app selects CS2 (`730`) records within two minutes of the owner's most recent `Playtime` timestamp. Original Steam timestamps are sent unchanged. Old rosters remain visible as stale in OPSearch until Steam updates them. Pairing also sends your computer's device name. Roster updates send the paired account's SteamID64, peer SteamID64 values, CS2 app ID, original `Playtime` timestamps, an observation timestamp, and companion version. No VDF file is uploaded. The backend replaces the account's latest roster; the companion does not send historical match records as separate events.

The local paired-device file contains bearer tokens. Do not share it or include it in bug reports. It is excluded from Git by default. To unlink a device, revoke it from the OPSearch Companion page; deleting a local config file alone does not revoke the server-side token.

The Steam cache is an undocumented client file format and can change. The parser currently supports the observed binary KeyValues1 `coplay` structure. No claim is made about VAC status or Steam's policy decisions.

## Updates

The app checks GitHub for a newer published release at startup and then every six hours. When one exists it shows a banner with a **Скачать** (Download) button that opens the release page; download the new executable and replace the old one. The check sends no account data: it is an anonymous request to the GitHub API for the latest release.

## Releasing

Versions follow `MAJOR.MINOR.PATCH` and live in `Cargo.toml`. Release assets keep the same names in every release, so `https://github.com/MascotWorld/opsearch-companion/releases/latest/download/opsearch-companion-windows-x64.exe` always serves the newest build.

- **From GitHub:** Actions → **Release** → **Run workflow**, choose `patch`, `minor` or `major`. The workflow bumps `Cargo.toml` and `Cargo.lock`, commits `chore: release vX.Y.Z`, tags it, builds the Windows executable and publishes the release with checksums and generated notes.
- **From a tag:** set the version in `Cargo.toml` yourself, commit, then `git tag vX.Y.Z && git push origin vX.Y.Z`. The workflow refuses a tag that does not match `Cargo.toml`.

## Build from source

Install the Rust toolchain for development. The repository pins the project toolchain in `rust-toolchain.toml`; rustup selects it only in this repository. Windows packaging requires the MSVC C++ build tools and Windows SDK, which are available on the GitHub Actions Windows runner.

```powershell
cargo fmt --all -- --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release --target x86_64-pc-windows-msvc
```

The target-specific Cargo configuration statically links the MSVC runtime. Run `scripts\package.ps1` on Windows to produce the standalone executable, ZIP package, and SHA-256 checksum under `dist\`.

## Development notes

- The binary VDF schema was validated against a real Steam `coplay_<owner>.vdf` sample. Public test fixtures are synthetic and contain no real account IDs.
- No credentials or real user VDF files belong in the repository.
- The network client uses HTTPS by default, has bounded request timeouts, and does not follow redirects.
