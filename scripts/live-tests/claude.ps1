# Live test: Claude Code ↔ ContextForge (isolated; does not modify the user's Claude configuration).
# Requires: claude CLI on PATH, signed in. Usage: pwsh scripts/live-tests/claude.ps1 [-Exe path\to\contextforge.exe] [-Model haiku]
param(
    [string]$Exe = (Resolve-Path "$PSScriptRoot\..\..\target\release\contextforge.exe" -ErrorAction SilentlyContinue),
    [string]$Model = "haiku"
)
$ErrorActionPreference = "Stop"
if (-not $Exe -or -not (Test-Path $Exe)) { $Exe = (Resolve-Path "$PSScriptRoot\..\..\target\debug\contextforge.exe").Path }
$work = Join-Path ([IO.Path]::GetTempPath()) "cf-live-claude-$(Get-Random)"
$data = Join-Path $work "data"; $repo = Join-Path $work "repo"
New-Item -ItemType Directory -Force $data, $repo | Out-Null
git -C $repo init -q -b main; git -C $repo config user.email live@example.invalid; git -C $repo config user.name live
Set-Content "$repo\inventory.py" "def stock(item):`n    return 0`n"; git -C $repo add -A; git -C $repo commit -q -m init
$env:CONTEXTFORGE_HOME = $data
& $Exe project add $repo --name LiveClaude | Out-Null
& $Exe memory add --kind decision --title "Inventory counts are stored in Redis (marker ZEBRA-7)" --content "Chosen for atomic INCR." --project $repo | Out-Null

$hooks = @{}
foreach ($h in @(@("SessionStart","session-start"),@("UserPromptSubmit","user-prompt-submit"),@("PostToolUse","post-tool-use"),@("Stop","stop"),@("SessionEnd","session-end"))) {
    $hooks[$h[0]] = @(@{ hooks = @(@{ type = "command"; command = "$Exe"; args = @("hook","claude",$h[1]); timeout = 30 }) })
}
$settings = Join-Path $work "settings.json"; (@{ hooks = $hooks } | ConvertTo-Json -Depth 10) | Set-Content $settings
$mcp = Join-Path $work "mcp.json"
(@{ mcpServers = @{ contextforge = @{ type = "stdio"; command = "$Exe"; args = @("mcp"); env = @{ CONTEXTFORGE_HOME = $data } } } } | ConvertTo-Json -Depth 10) | Set-Content $mcp

# Run as a fresh top-level Claude Code process.
foreach ($v in "CLAUDECODE","CLAUDE_CODE_ENTRYPOINT","CLAUDE_CODE_SESSION_ID","CLAUDE_CODE_CHILD_SESSION") { Remove-Item "Env:$v" -ErrorAction SilentlyContinue }
Push-Location $repo
$prompt = "1) Without using any tools, quote the marker that appears in the ContextForge project memory you were given. 2) Then call the mcp__contextforge__record_task tool with title 'Add Redis TTLs (live test)' and status 'pending'. Reply with the marker and the memory_id returned."
$out = claude -p $prompt --model $Model --settings $settings --mcp-config $mcp --strict-mcp-config --allowedTools "mcp__contextforge__record_task" --output-format text 2>&1
Pop-Location
"--- Claude output"; $out
"--- ContextForge sessions"; & $Exe sessions
"--- Tasks recorded via MCP"; & $Exe --json memory list --project $repo --kind task | ConvertFrom-Json | ForEach-Object { $_.hits } | ForEach-Object { "$($_.memory.id) $($_.memory.title) source=$($_.memory.source_agent) client=$($_.memory.metadata.mcp_client)" }
$pass = ($out -join "`n") -match "ZEBRA-7"
"RESULT context-injection: " + ($(if ($pass) { "PASS" } else { "FAIL" }))
Write-Output "work dir: $work"
