# Security Policy

ContextForge stores development context locally and is designed to never send it anywhere.

## Reporting a vulnerability
Please report vulnerabilities privately through GitHub's **Security → Report a vulnerability** form on this
repository. Do not open public issues for security problems. You should receive a response within 7 days.

## Security properties
* **Local only.** No telemetry, no network requests, no cloud services. The MCP server speaks stdio only and opens no sockets.
* **Secrets are redacted before storage** (API keys, tokens, private keys, credentials in URLs and assignments).
  Files matching sensitive patterns (`.env*`, keys, certificates, SSH/AWS/GPG folders, credential files, Terraform state)
  are referenced by path only; their content is never read, hashed or stored.
* **Memory is untrusted data.** Injected context is delimited and labelled as data; delimiters and control characters are
  neutralised; memories containing instruction-like text are withheld from automatic injection until reviewed.
  No tool executes code or shell commands.
* **Configuration changes are previewed, backed up, atomic and reversible.** Unparseable files are never overwritten.
  Codex hook trust is never written or bypassed.
* **Hooks never block agents**: bounded input, timeouts, and a documented no-op on any failure.

## Supported versions
Only the latest release receives security fixes during the 0.x series.
