# Creates a demo project and drives it through the real hook/CLI code paths of all three agent adapters,
# producing genuine ContextForge data for UI tests and README screenshots. Everything stays in -Data and -Repo.
param(
    [Parameter(Mandatory)] [string]$Data,
    [Parameter(Mandatory)] [string]$Repo,
    [string]$Exe = ""
)
$ErrorActionPreference = "Stop"
if (-not $Exe) { $Exe = (Resolve-Path "$PSScriptRoot\..\target\release\contextforge.exe" -ErrorAction SilentlyContinue).Path }
if (-not $Exe) { $Exe = (Resolve-Path "$PSScriptRoot\..\target\debug\contextforge.exe").Path }
$env:CONTEXTFORGE_HOME = $Data
New-Item -ItemType Directory -Force $Repo | Out-Null
function G { git -C $Repo @args 2>$null | Out-Null }
G init -q -b main; G config user.email demo@example.invalid; G config user.name "Demo Dev"; G config core.autocrlf false
New-Item -ItemType Directory -Force "$Repo\src\orders", "$Repo\src\payments", "$Repo\tests" | Out-Null
Set-Content "$Repo\src\orders\events.py" (1..40 | ForEach-Object { "# order events line $_" })
Set-Content "$Repo\src\orders\api.py" (1..30 | ForEach-Object { "# orders api line $_" })
Set-Content "$Repo\src\payments\stripe_client.py" (1..30 | ForEach-Object { "# payments line $_" })
Set-Content "$Repo\README.md" "# Acme Shop (ContextForge demo project)"
G add -A; G commit -q -m "Initial order service"

function Hook($provider, $event, $obj) { ($obj | ConvertTo-Json -Depth 8 -Compress) | & $Exe hook $provider $event | Out-Null }
& $Exe project add $Repo --name "acme-shop" | Out-Null
$cf = { param([string[]]$a) & $Exe @a | Out-Null }
function Mcp($client, [object[]]$calls) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo $Exe
    $psi.Arguments = "mcp"; $psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true; $psi.UseShellExecute = $false
    $psi.WorkingDirectory = $Repo; $psi.Environment["CONTEXTFORGE_HOME"] = $Data; $psi.CreateNoWindow = $true
    $p = [System.Diagnostics.Process]::Start($psi)
    $send = { param($o) $p.StandardInput.WriteLine(($o | ConvertTo-Json -Depth 10 -Compress)); $p.StandardInput.Flush() }
    & $send @{ jsonrpc = "2.0"; id = 1; method = "initialize"; params = @{ protocolVersion = "2025-06-18"; capabilities = @{}; clientInfo = @{ name = $client; version = "demo" } } }
    $null = $p.StandardOutput.ReadLine()
    & $send @{ jsonrpc = "2.0"; method = "notifications/initialized" }
    $id = 2
    foreach ($c in $calls) { & $send @{ jsonrpc = "2.0"; id = $id; method = "tools/call"; params = @{ name = $c[0]; arguments = $c[1] } }; $null = $p.StandardOutput.ReadLine(); $id++ }
    $p.StandardInput.Close(); $null = $p.WaitForExit(5000)
}
Push-Location $Repo

# --- Claude Code session
$c = @{ session_id = "demo-claude-1"; transcript_path = ""; cwd = $Repo }
Hook claude session-start ($c + @{ hook_event_name = "SessionStart"; source = "startup" })
Mcp "claude-code" @(
  ,@("record_decision", @{ title = "Orders are event-sourced from an append-only events table"; decision = "State is rebuilt by replaying order_events; orders are never updated in place."; rationale = "Auditability and easy replays."; alternatives = @("Mutable orders table with audit triggers"); files = @(@{ path = "src/orders/events.py" }) })
  ,@("save_memory", @{ kind = "convention"; title = "Money is stored as integer cents"; content = "All amounts use int cents with an ISO currency code; no floats anywhere in payments." })
  ,@("save_memory", @{ kind = "fact"; title = "Payments go through the Stripe client wrapper"; content = "src/payments/stripe_client.py centralises retries and idempotency keys."; files = @(@{ path = "src/payments/stripe_client.py" }); certainty = "verified"; evidence = @(@{ kind = "file"; reference = "src/payments/stripe_client.py"; excerpt = "retry + idempotency wrapper" }) })
)
Hook claude post-tool-use ($c + @{ hook_event_name = "PostToolUse"; tool_name = "Edit"; tool_input = @{ file_path = "$Repo\src\orders\events.py" }; tool_use_id = "t1" })
Hook claude post-tool-use ($c + @{ hook_event_name = "PostToolUse"; tool_name = "Bash"; tool_input = @{ command = "python -m pytest -q" }; tool_response = @{ stdout = "24 passed"; interrupted = $false }; tool_use_id = "t2" })
Hook claude stop ($c + @{ hook_event_name = "Stop"; last_assistant_message = "Implemented the order_events table and replay, with 24 passing tests." })
Hook claude session-end ($c + @{ hook_event_name = "SessionEnd"; reason = "prompt_input_exit" })
Add-Content "$Repo\src\orders\events.py" "# replay implemented"; G add -A; G commit -q -m "Add order event replay"

# --- Codex session
$x = @{ session_id = "demo-codex-1"; transcript_path = $null; cwd = $Repo; model = "gpt-5-codex" }
Hook codex session-start ($x + @{ hook_event_name = "SessionStart"; source = "startup" })
Mcp "codex-mcp-client" @(
  ,@("record_task", @{ title = "Expose order history endpoint"; status = "in_progress"; details = "GET /orders/{id}/history returns the event stream."; files = @(@{ path = "src/orders/api.py" }) })
  ,@("record_bug", @{ title = "Refund webhook retries create duplicate events"; details = "Stripe retries the refund webhook; the handler lacks an idempotency check."; files = @(@{ path = "src/payments/stripe_client.py" }) })
  ,@("save_memory", @{ kind = "research"; title = "Can we batch replays per customer?"; content = "Hypothesis: grouping replays by customer cuts cold-start time by ~5x."; certainty = "hypothesis" })
)
Hook codex post-tool-use ($x + @{ hook_event_name = "PostToolUse"; tool_name = "apply_patch"; tool_use_id = "c1"; tool_input = @{ command = "*** Begin Patch`n*** Update File: src/orders/api.py`n*** End Patch" } })
Hook codex stop ($x + @{ hook_event_name = "Stop"; last_assistant_message = "Started the order history endpoint; found a duplicate-event bug in refund webhooks." })
Hook codex session-end ($x + @{ hook_event_name = "SessionEnd"; reason = "other" })

# --- Antigravity session
$gemini = "C:/Users/demo/.gemini/antigravity-cli/brain/d1/.system_generated/logs/transcript.jsonl"
$a = @{ conversationId = "demo-agy-1"; workspacePaths = @($Repo); transcriptPath = $gemini; modelName = "gemini-3.6-pro" }
Hook antigravity session-start $a
Hook antigravity pre-invocation ($a + @{ invocationNum = 0; initialNumSteps = 1; lastUserInput = "continue the order history endpoint" })
Mcp "" @(
  ,@("save_memory", @{ kind = "command"; title = "docker compose up -d postgres"; content = "Starts the local Postgres used by integration tests."; certainty = "verified"; evidence = @(@{ kind = "command"; reference = "docker compose up -d postgres"; excerpt = "container healthy" }) })
)
Hook antigravity post-tool-use ($a + @{ toolCall = @{ name = "run_command"; args = @{ CommandLine = "npm run lint"; Cwd = $Repo } }; stepIdx = 3; error = "" })
Hook antigravity post-tool-use ($a + @{ toolCall = @{ name = "replace_file_content"; args = @{ TargetFile = "$Repo\src\orders\api.py" } }; stepIdx = 4; error = "" })
Hook antigravity stop ($a + @{ executionNum = 1; terminationReason = "model_stop"; fullyIdle = $true })

# A real change makes one memory potentially stale; then a handoff for the next agent.
Set-Content "$Repo\src\payments\stripe_client.py" (1..30 | ForEach-Object { "# payments v2 line $_" })
& $cf freshness
& $cf handoff --to claude_code
Pop-Location
"Seeded demo data in $Data"
