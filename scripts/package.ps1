$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$target = "x86_64-pc-windows-msvc"
$dist = Join-Path $root "dist"
$stage = Join-Path $dist "package-windows-x64"
$exeName = "opsearch-companion.exe"
$exe = Join-Path $root "target\$target\release\$exeName"
$zipName = "opsearch-companion-windows-x64.zip"
$zip = Join-Path $dist $zipName

New-Item -ItemType Directory -Force -Path $dist | Out-Null
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

cargo test --locked
if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }

# The target-specific Cargo config enables the static MSVC CRT.
cargo build --locked --release --target $target
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

if (-not (Test-Path $exe)) { throw "Expected executable was not produced: $exe" }

Copy-Item $exe (Join-Path $stage $exeName)
Copy-Item (Join-Path $root "README.md") $stage
Copy-Item (Join-Path $root "LICENSE") $stage
Copy-Item (Join-Path $root "assets\fonts\OFL-Onest.txt") $stage
Copy-Item (Join-Path $root "assets\fonts\OFL-Unbounded.txt") $stage
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip
Remove-Item -Recurse -Force $stage

$hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
"$hash  $zipName" | Set-Content -Encoding ascii ("$zip.sha256")

# Retain the standalone executable as a direct-download release asset as well.
$standalone = Join-Path $dist "opsearch-companion-windows-x64.exe"
Copy-Item $exe $standalone -Force
$exeHash = (Get-FileHash -Algorithm SHA256 $standalone).Hash.ToLowerInvariant()
"$exeHash  opsearch-companion-windows-x64.exe" | Set-Content -Encoding ascii ("$standalone.sha256")
Write-Host "Packaged $zipName and standalone x64 executable."
