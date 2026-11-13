# Development

## Prerequisites (Windows 10/11 x64)
* Rust stable (1.85+) with the MSVC toolchain and Visual Studio C++ build tools
* Node.js 20+ and npm
* Git (used at runtime for branch/commit/rename awareness)
* WebView2 runtime (preinstalled on Windows 11)

## Layout
```
crates/core          memory engine (SQLite/FTS5, freshness, context, handoff, redaction)
crates/integrations  provider adapters, installers, detection, doctor
crates/mcp           MCP server (rmcp)
crates/cli           `contextforge` binary + protocol and end-to-end tests
apps/desktop         Tauri 2 + React + TypeScript + Tailwind
scripts/             seed-demo.ps1, live-tests/*.ps1, build-installer.ps1
```

## Build and test
```powershell
cargo build -p contextforge-cli                 # the contextforge CLI / hook / MCP binary
cargo test --workspace                          # unit, integration, MCP protocol and E2E tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

cd apps/desktop
npm ci
npm run typecheck; npm run lint; npm test; npm run build
```
`cargo build`/`clippy` of the desktop crate needs the CLI sidecar and the frontend build:
```powershell
New-Item -ItemType Directory -Force apps/desktop/src-tauri/binaries
Copy-Item target/debug/contextforge.exe apps/desktop/src-tauri/binaries/contextforge-x86_64-pc-windows-msvc.exe
```

## Run the desktop app
```powershell
cd apps/desktop
npx tauri dev
```
Use an isolated data directory while developing: `$env:CONTEXTFORGE_HOME = "$env:TEMP\cf-dev"`.
`scripts/seed-demo.ps1 -Data <dir> -Repo <dir>` fills it with a demo project through the real hook and MCP code paths.

## UI end-to-end tests
```powershell
cargo build -p contextforge-cli --release
cd apps/desktop; npx tauri build --no-bundle
pwsh ../../scripts/seed-demo.ps1 -Data $env:TEMP\cf-ui -Repo $env:TEMP\cf-ui-repo
node e2e/run.mjs ..\..\target\release\contextforge-desktop.exe $env:TEMP\cf-ui --screenshots e2e/screenshots
```
The harness launches the real app with WebView2 remote debugging and drives it with Playwright over CDP.

## Live provider tests
`scripts/live-tests/{claude,codex,antigravity}.ps1` run the real agents against a throwaway repository and data
directory without changing your agent configuration. They require the agent to be installed and signed in.

## Installer
`scripts/build-installer.ps1` builds the release CLI, stages it as the Tauri sidecar and produces the NSIS installer in
`target/release/bundle/nsis/`.
