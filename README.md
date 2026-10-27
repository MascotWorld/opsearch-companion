# OPSearch Companion

**English** · [Русский](README.ru.md)

OPSearch Companion is a small Windows app that tells [OPSearch](https://opsearch.pro) who you just played Counter-Strike 2 with, so the site can fill its comparison table with your lobby in one click.

It is a single portable executable: no installer, and nothing else to install. It does not use Steamworks, does not inject into any process, does not read game memory and does not start or touch the game. It only reads a cache file the Steam client already keeps on disk.

## Get started

1. Download `opsearch-companion-windows-x64.exe` from [Releases](https://github.com/MascotWorld/opsearch-companion/releases) and run it with Steam open and signed in.
2. The app detects the Steam account signed in on this computer. Click **Подключить через Steam** (Connect with Steam).
3. A browser opens on opsearch.pro. Sign in with the same Steam account if asked and click **Подключить** (Connect). The app notices the approval within a couple of seconds and starts sending your lobby.

There is no code to copy. A computer is only linked after the Steam account it reported approves it on the site while signed in, so nobody can attach a computer to someone else's account. A request that is not approved within ten minutes expires. Each Steam account you play on is linked separately. Unlink a computer on the [companion page](https://opsearch.pro/companion).

## What the app does, exactly

This section is the complete list. If the app ever does something not described here, that is a bug.

### What it reads on your computer

| What | Why |
| --- | --- |
| Registry `HKCU\Software\Valve\Steam\ActiveProcess` → `ActiveUser` | Which Steam account is signed in right now. |
| Registry `HKCU\Software\Valve\Steam` → `SteamPath` | Where Steam is installed. |
| `<Steam>\config\coplay_<SteamID>.vdf` | Steam's own "recently played with" list. |
| Environment variable `COMPUTERNAME` | The name shown for this computer on the site, sent once when linking. |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` → `OPSearch Companion` | Only to show whether "Start with Windows" is on. |

Every five seconds the app hashes the bytes of the `coplay_*.vdf` files to notice changes. It parses only the file of the account that is signed in **and** linked; other accounts' files on the same computer are never parsed or sent. From that file it keeps only CS2 (app `730`) entries within two minutes of your most recent game, which is the lobby you were just in.

It reads nothing else: no game files, no screenshots, no chat, no browser data, no other programs.

### What it sends, and where

| Destination | When | What |
| --- | --- | --- |
| `opsearch.pro` | Once, when you click Connect | Your SteamID64, the computer name, the app version. |
| `opsearch.pro` | While waiting for your approval, every 2 s for up to 10 min | The link request ID with its one-time token. |
| `opsearch.pro` | When the lobby changes, and every 30 s while it runs | Your SteamID64, the SteamID64s of the players in your last lobby, the time Steam recorded for each of them, the time the app looked, the app version. |
| `steamcommunity.com`, `*.steamstatic.com` | Once per player shown in the window | A public profile request (`/profiles/<id>?xml=1`) and the avatar picture, to show names and avatars. No account data or token is sent. |
| `api.github.com` | At start and every six hours | An anonymous request for the latest release, to offer updates. |

The VDF file itself is never uploaded. All requests use HTTPS. The device token is only sent to opsearch.pro and is not forwarded through redirects. The server keeps only your latest lobby; the app sends no match history.

### Who sees your lobby

You, when signed in on opsearch.pro with the same Steam account. Players in that lobby who are signed in on opsearch.pro also see it while it is fresh (sent within the last two minutes), even if they have no app of their own. Nobody else does.

### What it stores on your computer

| What | Where |
| --- | --- |
| Device token for each linked account | `%LOCALAPPDATA%\OPSearch\Companion\config.json` |
| Settings (tray) | `%LOCALAPPDATA%\OPSearch\Companion\settings.json` |
| Autostart entry, only if you turn it on | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` → `OPSearch Companion` |

The token in `config.json` lets this computer send lobbies to your OPSearch account. Do not share the file or attach it to bug reports. Deleting it does not unlink the computer; unlink it on the [companion page](https://opsearch.pro/companion), which makes the token useless immediately.

### Settings

The gear in the title bar opens the settings. Both options are off by default.

- **Start with Windows** adds the app to your user's startup list. It then starts in the background: in the tray if the tray is on, otherwise minimised.
- **Minimise to tray** puts an icon next to the clock. Close and minimise then hide the window instead of closing it. Click the icon to bring the window back, or right-click it and choose **Выйти** (Quit) to exit.

**Pause** in the window's footer stops reading and sending until you press **Продолжить** (Resume).

## Updates

The app checks GitHub for a newer release at start and every six hours. When there is one, the footer shows **Обновить до X** (Update to X). It opens the release page; download the new executable and replace the old one. Your link and settings are kept.

## Limitations

The `coplay` cache is an undocumented Steam file and can change; the parser supports the binary KeyValues structure Steam writes today. Steam only updates it after you actually play with someone, so the lobby appears after a match starts, not in the main menu. No claim is made about VAC or Steam's policies.

## Releasing

Versions follow `MAJOR.MINOR.PATCH` and live in `Cargo.toml`. Release assets keep the same names in every release, so `https://github.com/MascotWorld/opsearch-companion/releases/latest/download/opsearch-companion-windows-x64.exe` always serves the newest build.

- **From GitHub:** Actions → **Release** → **Run workflow**, choose `patch`, `minor` or `major`. The workflow bumps `Cargo.toml` and `Cargo.lock`, commits `chore: release vX.Y.Z`, tags it, builds the Windows executable and publishes the release with checksums and generated notes.
- **From a tag:** set the version in `Cargo.toml` yourself, commit, then `git tag vX.Y.Z && git push origin vX.Y.Z`. The workflow refuses a tag that does not match `Cargo.toml`.

## Build from source

Install the Rust toolchain. The repository pins it in `rust-toolchain.toml`; rustup selects it only in this repository. Windows packaging requires the MSVC C++ build tools and Windows SDK, which the GitHub Actions Windows runner has.

```powershell
cargo fmt --all -- --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release --target x86_64-pc-windows-msvc
```

The target-specific Cargo configuration statically links the MSVC runtime. Run `scripts\package.ps1` on Windows to produce the standalone executable, ZIP package and SHA-256 checksum under `dist\`.

## Development notes

- The binary VDF schema was validated against a real Steam `coplay_<owner>.vdf` sample. Test fixtures are synthetic and contain no real account IDs.
- No credentials or real user VDF files belong in the repository.
- The OPSearch client uses HTTPS, bounded timeouts and does not follow redirects.
