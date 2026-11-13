# Live test: Codex ↔ ContextForge MCP (isolated: per-run -c overrides only; no persistent config changes).
# Hook execution in Codex requires the user's trust review (/hooks); this script never bypasses it.
param([string]$Exe = "")
$ErrorActionPreference = "Stop"
if (-not $Exe) { $Exe = (Resolve-Path "$PSScriptRoot\..\..\target\debug\contextforge.exe").Path }
$work = Join-Path ([IO.Path]::GetTempPath()) "cf-live-codex-$(Get-Random)"
$data = Join-Path $work "data"; $repo = Join-Path $work "repo"
New-Item -ItemType Directory -Force $data, $repo | Out-Null
git -C $repo init -q -b main; git -C $repo config user.email live@example.invalid; git -C $repo config user.name live
Set-Content "$repo\billing.py" "def charge(): pass`n"; git -C $repo add -A; git -C $repo commit -q -m init 2>$null
$env:CONTEXTFORGE_HOME = $data
& $Exe project add $repo --name LiveCodex | Out-Null
& $Exe memory add --kind decision --title "Billing uses idempotency keys (marker OTTER-42)" --content "Every charge request carries an idempotency key." --project $repo | Out-Null

$exeToml = "'" + $Exe + "'"; $dataToml = "'" + $data + "'"
$overrides = @(
  "-c", "mcp_servers.contextforge.command=$exeToml",
  "-c", "mcp_servers.contextforge.args=['mcp']",
  "-c", "mcp_servers.contextforge.env.CONTEXTFORGE_HOME=$dataToml",
  "-c", "mcp_servers.contextforge.default_tools_approval_mode='approve'"
)
$prompt = "Use the contextforge MCP server: call its search_memory tool with query 'billing idempotency', then call record_task with title 'Add idempotency tests (codex live)' and status 'pending'. Report the marker you found and the memory_id returned. Do not edit files."
Push-Location $repo
$out = codex exec @overrides --sandbox read-only $prompt 2>&1
Pop-Location
"--- Codex output (tail)"; $out | Select-Object -Last 25
"--- Memories recorded"; & $Exe --json memory list --project $repo --kind task | ConvertFrom-Json | ForEach-Object { $_.hits } | ForEach-Object { "$($_.memory.id) $($_.memory.title) source=$($_.memory.source_agent) client=$($_.memory.metadata.mcp_client)" }
"RESULT mcp-search: " + $(if (($out -join "`n") -match "OTTER-42") { "PASS" } else { "FAIL" })
"work dir: $work"
