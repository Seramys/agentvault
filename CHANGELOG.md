# Changelog

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