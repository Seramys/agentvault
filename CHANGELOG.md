# Changelog

## Unreleased

### Added

- Web console is part of the workspace: `agentvault web --port 8787`, React
  build embedded in the binary, token written to `web.token`, SSE authenticated
  with an Authorization header, CORS removed.
- Local accounts and sessions: Argon2id password hashes, SHA-256 session
  storage, owner/admin/viewer roles, tenant labels, 12-hour sessions, and a
  server-side read-only guard for viewers.
- Web API: `POST /api/tickets`, `/api/agents/candidates`,
  `/api/agents/plan`; the frontend can issue tickets and preview an agent
  rewrite before applying it.
- One-time desktop handoff: the desktop shell may pass its loopback token as a
  `?token=` query; the webapp stores it, then strips it from the address bar.
- Encrypted config backups (`AVBAK1` + ChaCha20-Poly1305, key in the OS
  keystore) with `backup create/restore/list`.
- Ed25519 signatures over the append-only audit hash chain:
  `audit --verify-signature`, `audit --pubkey`.
- Codex `config.toml` rewrite now uses `toml_edit` with full round-trip
  preservation (quotes, escapes, multiline arrays, inline tables, dotted keys,
  comments, CRLF, BOM, unknown fields) and rejects invalid TOML before writing.
- `audit --verify/--export/--event/--since`; JSONL audit lines carry a
  `prev_hash`/`hash` chain.
- `init --cleanup-backups`; `secret add` prompts with masked input
  (`rpassword`); `--value` warns about shell history and process lists.
- CI and release build the web frontend before the Rust workspace.
- Test coverage: 54 Rust tests passing plus one explicitly ignored ConPTY
  integration test, and two Playwright console tests.

### Changed

- `init` now writes an encrypted backup before rewriting, then re-parses; a
  failed verification restores the backup instead of leaving a partial file.
- Backup listing still reports legacy plaintext `*.bak.*` files so they can be
  cleaned up.

### Fixed

- Windows DPAPI no longer passes plaintext through the PowerShell command line;
  it uses a short-lived private temp file.
- `atomic_write` uses a unique temp file per write and retries the Windows
  rename, fixing concurrent JSON writes.
- Codex TOML single-quoted args containing commas are no longer split
  incorrectly; unsupported non-string args/env fail closed.
- Parallel CLI e2e tests no longer race the broker startup lock.

### Removed

- TUI experiment (`crates/tui`, `tui-prototype`).

## 0.1.0 — 2026-11-21

First CLI-only release.

### Added

- `init` for JSON `mcpServers` and Codex TOML: discovery, dry-run plan, backup,
  env migration, command wrapping and re-parse verification.
- Static per-client path registry (Claude Code, Cursor, Codex, Gemini CLI,
  Windsurf, Cline, Qoder, generic `--path`) with project-first selection.
- Env migration heuristic with `--migrate-env` / `--keep-env` overrides and
  `migrate` / `keep` / `unchanged` / `skip` reporting.
- Embedded env reference guard: `${VAR}`, `$VAR`, `%VAR%`, `{{VAR}}` are never
  migrated, including values such as `${HOME}/.config`.
- `init --refresh` to repoint already wrapped servers at the current launcher and
  home without re-migrating env.
- OS keystore backends: Windows DPAPI through PowerShell, macOS Keychain, Linux
  Secret Service, encrypted-file fallback.
- HMAC-SHA256 `av1` tickets: TTL, max uses, scope, parent/child, renewable task
  tickets with hard TTL, rotation epochs and global revocation epoch.
- Commands: `secret`, `issue`, `resolve`, `child`, `renew`, `run`, `revoke`,
  `rotate`, `status`, `audit`, `honeypot`, `env`, `doctor`, `daemon`, `stop`.
- Local broker on `127.0.0.1`, single-instance lock, owner-only admin token,
  autostart opt-out.
- Append-only JSONL audit and honeypot canaries that trigger global revocation.
- User-scope `AGENTVAULT_HOME` / PATH persistence on first real `init`, with
  `env show/install/uninstall` and `--no-env`.
- Real Windows 11 test run: 37 Rust tests and 150 isolated CLI checks.

### Deferred

- L2 sandbox / microVM.
- L3 egress control / DLP.
- Web console.
- macOS/Linux real-machine validation.
- TPM / Secure Enclave binding.

### Notes

- `init` wrappers inject migrated **values** through `--env` for stock MCP
  servers; ticket-aware servers should use `run --name <same-name secret>` or
  `--session`.
- Unknown config fields are preserved; JSONC comments and trailing commas are
  rejected with an error rather than silently rewritten.
- Backups contain the original plaintext values; delete them after verifying a
  rewrite.