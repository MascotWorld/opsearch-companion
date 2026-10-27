$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$dist = Join-Path $root "dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null
go test ./...
foreach ($arch in @("amd64", "arm64")) {
    $exe = "opsearch-companion-windows-$arch.exe"
    $env:GOOS = "windows"
    $env:GOARCH = $arch
    go build -buildvcs=false -trimpath -ldflags "-s -w" -o (Join-Path $dist $exe) ./cmd/opsearch-companion
    $stage = Join-Path $dist "package-$arch"
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    Copy-Item (Join-Path $dist $exe) (Join-Path $stage "opsearch-companion.exe")
    Copy-Item (Join-Path $root "README.md") $stage
    Copy-Item (Join-Path $root "LICENSE") $stage
    Copy-Item (Join-Path $root "run.cmd") $stage
    Compress-Archive -Path (Join-Path $stage "*") -DestinationPath (Join-Path $dist "opsearch-companion-windows-$arch.zip") -Force
    Remove-Item -Recurse -Force $stage
    $zip = Join-Path $dist "opsearch-companion-windows-$arch.zip"
    (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower() + "  " + (Split-Path -Leaf $zip) | Set-Content -Encoding ascii ($zip + ".sha256")
}
Remove-Item Env:GOOS
Remove-Item Env:GOARCH
