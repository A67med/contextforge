# Builds the Windows NSIS installer: release CLI → Tauri sidecar → desktop app bundle.
# Output: target\release\bundle\nsis\ContextForge_<version>_x64-setup.exe (+ SHA256SUMS.txt)
$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot\.."
Set-Location $root
cargo build -p contextforge-cli --release --locked
if ($LASTEXITCODE) { throw "CLI build failed" }
New-Item -ItemType Directory -Force apps\desktop\src-tauri\binaries | Out-Null
Copy-Item target\release\contextforge.exe apps\desktop\src-tauri\binaries\contextforge-x86_64-pc-windows-msvc.exe -Force
Set-Location apps\desktop
if (-not (Test-Path node_modules)) { npm ci }
npx tauri build
if ($LASTEXITCODE) { throw "Tauri build failed" }
Set-Location $root
$out = "target\release\bundle\nsis"
Get-ChildItem "$out\*.exe" | ForEach-Object { "{0}  {1}" -f (Get-FileHash $_ -Algorithm SHA256).Hash.ToLower(), $_.Name } | Set-Content "$out\SHA256SUMS.txt"
Get-ChildItem $out | ForEach-Object { "{0}  {1:N1} MB" -f $_.FullName, ($_.Length / 1MB) }
