# Live test: Antigravity CLI ↔ ContextForge (workspace-level .agents/ config in a throwaway repo;
# the global ~/.gemini/config is not modified). Usage: pwsh scripts/live-tests/antigravity.ps1 [-Exe path]
param([string]$Exe = "")
$ErrorActionPreference = "Stop"
if (-not $Exe) { $Exe = (Resolve-Path "$PSScriptRoot\..\..\target\debug\contextforge.exe").Path }
$work = Join-Path ([IO.Path]::GetTempPath()) "cf-live-agy-$(Get-Random)"
$data = Join-Path $work "data"; $repo = Join-Path $work "repo"
New-Item -ItemType Directory -Force $data, $repo | Out-Null
git -C $repo init -q -b main; git -C $repo config user.email live@example.invalid; git -C $repo config user.name live
Set-Content "$repo\search.py" "def query(): pass`n"; git -C $repo add -A; git -C $repo commit -q -m init 2>$null
$env:CONTEXTFORGE_HOME = $data
& $Exe project add $repo --name LiveAntigravity | Out-Null
& $Exe memory add --kind decision --title "Search uses SQLite FTS5 (marker HERON-9)" --content "No external search service." --project $repo | Out-Null
# Install workspace-scoped integration through ContextForge's own installer.
& $Exe integrations install antigravity --project $repo --yes | Out-Null
# The MCP server needs the isolated data dir: add env to the generated workspace config.
$mcpPath = Join-Path $repo ".agents\mcp_config.json"; $m = Get-Content $mcpPath -Raw | ConvertFrom-Json
$m.mcpServers.contextforge | Add-Member -NotePropertyName env -NotePropertyValue @{ CONTEXTFORGE_HOME = $data } -Force
$m | ConvertTo-Json -Depth 10 | Set-Content $mcpPath
"--- generated .agents/hooks.json"; Get-Content (Join-Path $repo ".agents\hooks.json")
$settings = '{"permissions":{"allow":["mcp(contextforge/*)"]}}'
$prompt = "First, without calling any tool, state the marker from the ContextForge project memory in your context. Then use the contextforge MCP server to call record_task with title 'Tune FTS ranking (agy live)' and status 'pending', and report the memory_id it returns."
Push-Location $repo
$out = agy -p $prompt --settings $settings --print-timeout 240s 2>&1
Pop-Location
"--- agy output (tail)"; $out | Select-Object -Last 20
"--- sessions"; & $Exe sessions
"--- tasks"; & $Exe --json memory list --project $repo --kind task | ConvertFrom-Json | ForEach-Object { $_.hits } | ForEach-Object { "$($_.memory.id) $($_.memory.title) source=$($_.memory.source_agent) client=$($_.memory.metadata.mcp_client)" }
"--- hook log"; Get-ChildItem (Join-Path $data "logs") | Get-Content | Select-String "hook" | Select-Object -Last 12
"RESULT context-injection: " + $(if (($out -join "`n") -match "HERON-9") { "PASS" } else { "FAIL" })
"work dir: $work"
