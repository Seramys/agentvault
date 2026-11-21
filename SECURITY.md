# Security policy

## Supported versions

Security fixes target the latest `0.1.x` release line.

## Threat model

AgentVault is a credential lifecycle layer for local AI agents. It keeps
long-lived values in the OS keystore and gives agent processes short-lived,
signed tickets with expiry, scope and a use count.

It is **not** a sandbox and does not claim to stop a malicious process running
as the same OS user. The local broker and admin token live under the user
profile; any process that can read the user's files can reach them. Sandboxing
and egress control are explicit follow-ups.

The implemented boundaries are:

- Windows DPAPI, macOS Keychain, Linux Secret Service, or a 0600 encrypted-file
  fallback for the master key.
- Hostname/user and hardware fingerprint binding.
- HMAC-SHA256 ticket signatures with global and per-secret revocation epochs.
- One-use tickets by default; `max_uses` is explicit.
- Task-ticket hard expiry caps leaf tickets and sliding renewals.
- Append-only JSONL audit trail and honeypot canaries that trigger global
  revocation.
- Local-only broker listener on `127.0.0.1`; admin token restricted to the
  current user.

## Reporting a vulnerability

Please email **stacklanekit@gmail.com** with:

- affected version / commit,
- platform and reproduction steps,
- impact and whether the issue is already public.

Do not open a public GitHub issue for an exploitable secret-handling or
authentication bypass. Do not include real production secrets in a report; use
synthetic values that reproduce the issue.

There is no paid bounty. Good-faith reports will be acknowledged and credited
unless you ask to stay anonymous.

## Operational guidance

- Delete `config.toml.bak.*` / `mcp.json.bak.*` files after verifying a rewrite;
  backups contain the original plaintext values.
- Use `agentvault init --dry-run` before touching a production config.
- Keep the release binary in a stable location; `init --refresh` can repoint
  wrappers after an upgrade.
- Run `agentvault revoke --all` on a lost laptop or suspected incident.
- `agentvault env install` writes user-scope `AGENTVAULT_HOME` and PATH;
  `agentvault env uninstall` reverses it.