#Requires -Version 5.1
# Corgigram release build — Windows (native, run on Windows)
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$Version = (Select-String -Path Cargo.toml -Pattern '^version' | Select-Object -First 1).Line -replace '.*"(.*)".*','$1'
$Out = Join-Path $Root "dist\corgigram-$Version-windows-x86_64"
New-Item -ItemType Directory -Force -Path $Out | Out-Null

Write-Host "==> Corgigram release $Version (Windows x86_64)"
Write-Host "    Output: $Out"
Write-Host

Write-Host "==> cargo test"
cargo test
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "==> CLI release"
cargo build --release -p corgigram
Copy-Item "target\release\corgigram.exe" "$Out\corgigram.exe" -Force

Write-Host "==> Desktop release (Tauri)"
Write-Host "    Requires: Visual Studio Build Tools, WebView2"
cargo build --release -p corgigram-desktop
Copy-Item "target\release\corgigram-desktop.exe" "$Out\corgigram-desktop.exe" -Force

if (Get-Command cargo-tauri -ErrorAction SilentlyContinue) {
    Write-Host "==> Tauri bundle (MSI)"
    Set-Location apps\desktop
    cargo tauri build --ci
    Set-Location $Root
    $Bundle = "target\release\bundle\nsis"
    if (Test-Path $Bundle) {
        Copy-Item -Recurse $Bundle "$Out\installer" -Force
    }
}

Copy-Item "docs\release-test.md" "$Out\TESTING.md"
@"
Corgigram $Version — Windows x86_64

  corgigram.exe          CLI
  corgigram-desktop.exe  GUI

Data: %LOCALAPPDATA%\corgigram\
Firebase is preconfigured.

See TESTING.md for Linux + Windows test plan.
"@ | Set-Content "$Out\README.txt" -Encoding UTF8

Write-Host
Write-Host "Done."
Get-ChildItem $Out
