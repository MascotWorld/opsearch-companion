# OPSearch Companion

OPSearch Companion is a small Windows app that reads Steam's local recent-player cache for Counter-Strike 2 and sends the current recent-player roster to your OPSearch account. It runs as a console program, checks every five seconds, and sends a heartbeat at least every 30 seconds.

The companion supports Windows x64 and Windows ARM64. It does not use Steamworks, inject into a process, read game memory, or start a game.

## Pair your Steam account

1. Sign in to [OPSearch Companion](https://opsearch.pro/companion) with Steam.
2. Choose **Create pairing code**. The code expires after ten minutes and can be used once.
3. Download the correct Windows x64 or ARM64 ZIP from [Releases](https://github.com/MascotWorld/opsearch-companion/releases) and extract it. The `run.cmd` launcher keeps the console open if the companion exits.
4. Enter the code in this command:

   ```powershell
   .\opsearch-companion.exe --pair ABCD1234EFGH
   ```

The companion reads the active Steam account from the current Windows user's Steam registry settings and binds the pairing to that account. Pair each Steam account you want to use separately. Credentials are stored per account, each bound to the server that issued them. The credential file is saved under `%LOCALAPPDATA%\OPSearch\Companion\config.json`; keep access to your Windows account protected. The token is a device credential for your OPSearch account.

## Run

Keep Steam signed in, then start `run.cmd` from the extracted folder to keep the console visible. Or run the executable from PowerShell:

```powershell
.\opsearch-companion.exe
```

Every five seconds, the app scans `config\coplay_*.vdf` for file additions, replacements, and changes. It publishes only the roster from the file matching the currently signed-in and paired Steam account; it never combines accounts. It pauses uploads while Steam is signed out and switches to the corresponding pairing after an account change. It selects the CS2 (`730`) records within two minutes of the owner's most recent `Playtime` timestamp. This avoids mixing older matches retained in Steam's cache. Original Steam timestamps are sent unchanged; old rosters remain visible as stale in OPSearch until Steam updates them.

Useful options:

```powershell
# Inspect the active Steam account and VDF locally; never uploads
.\opsearch-companion.exe --diagnose

# Inspect once locally and exit; never uploads
.\opsearch-companion.exe --once

# Use an explicit config file
.\opsearch-companion.exe --config .\config.json
```

For parser development, `--coplay-file PATH --owner-steamid64 ID --diagnose` inspects one file without requiring Windows Steam registry access or a pairing. This option is read-only and never uploads. `--server` selects the HTTPS service origin; plain HTTP is accepted only for localhost testing. Each pairing stores its own server origin, and redirects are rejected so a bearer token is never forwarded to another host.

## Data and privacy

The app reads Steam's `ActiveProcess` and Steam install-path registry values. Every five seconds it reads and hashes the bytes of each `config\coplay_*.vdf` file to notice additions, replacements, and changes. It parses and uploads a roster only from the file whose owner matches the currently signed-in, paired Steam account; it does not parse or upload the other accounts' roster contents. Pairing also sends your computer's device name. Roster updates send the paired account's SteamID64, peer SteamID64 values, CS2 app ID, original `Playtime` timestamps, an observation timestamp, and companion version. No VDF file is uploaded. The backend replaces the account's latest roster; the companion does not send historical match records as separate events.

The local paired-device file contains bearer tokens. Do not share it or include it in bug reports. It is excluded from Git by default. To unlink a device, revoke it from the OPSearch Companion page; deleting a local config file alone does not revoke the server-side token.

The Steam cache is an undocumented client file format and can change. The parser currently supports the observed binary KeyValues1 `coplay` structure. `--diagnose` reports parsing/account issues without uploading data. No claim is made about VAC status or Steam's policy decisions.

## Build from source

Requires Go 1.26 or newer.

```powershell
go test ./...
$env:GOOS = "windows"
$env:GOARCH = "amd64"
go build -trimpath -o dist\opsearch-companion-windows-amd64.exe .\cmd\opsearch-companion
$env:GOARCH = "arm64"
go build -trimpath -o dist\opsearch-companion-windows-arm64.exe .\cmd\opsearch-companion
```

`scripts\package.ps1` builds both Windows executables, creates ZIP packages, and writes SHA-256 checksum files.

## Development notes

- The binary VDF schema was validated against a real Steam `coplay_<owner>.vdf` sample. Public test fixtures are synthetic and contain no real account IDs.
- No credentials or real user VDF files belong in the repository.
- The network client uses HTTPS by default, has bounded request timeouts, and does not follow redirects.
