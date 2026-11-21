<p align="center">
  <img src="docs/assets/brand/logo.png" width="150" alt="AgentVault logo">
</p>

<h1 align="center">AgentVault</h1>

<p align="center">
  <strong>Turn long-lived AI-agent credentials into one-use tickets.</strong><br>
  Secrets stay in the OS keystore; agents receive tickets that expire, can be revoked, and default to a single use.
</p>

<p align="center">
  <a href="https://github.com/Seramys/agentvault/actions/workflows/ci.yml"><img src="https://github.com/Seramys/agentvault/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-6D5BD0.svg" alt="License"></a>
  <img src="https://img.shields.io/badge/rust-stable-CE422B.svg" alt="Rust stable">
  <img src="https://img.shields.io/badge/status-v0.1-0F9F6E.svg" alt="v0.1">
</p>

<p align="center">
  <a href="README.md">简体中文</a> · <b>English</b> · <a href="README_JA.md">日本語</a> · <a href="README_KO.md">한국어</a>
</p>

<p align="center">
  <a href="#understand-it-in-30-seconds">30 seconds</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="#step-by-step-guide">Guide</a> ·
  <a href="#command-reference">Commands</a> ·
  <a href="#security-model">Security</a> ·
  <a href="#real-test-evidence">Evidence</a>
</p>

> **v0.1 scope**: layers 0 (secret custody), 1 (short-lived tickets + local broker) and 4 (audit + honeypots) are implemented. Sandboxing (L2) and egress control/DLP (L3) are explicitly out of scope. The web console is deferred; this repository ships the CLI only.
>
> **In one sentence**: stop deciding whether a token belongs in `mcp.json`. Put it in the vault and let the agent exchange a ticket for it.

---

## Contents

<details>
<summary>Expand</summary>

- [The problem](#the-problem)
- [Understand it in 30 seconds](#understand-it-in-30-seconds)
- [What it is, and what it is not](#what-it-is-and-what-it-is-not)
- [Supported agents](#supported-agents)
- [Five layers](#five-layers)
- [Install](#install)
- [Quick start](#quick-start)
- [Step-by-step guide](#step-by-step-guide)
  - [Step 0: see the whole picture](#step-0-see-the-whole-picture)
  - [Step 1: install and inspect](#step-1-install-and-inspect)
  - [Step 2: discover agent configs](#step-2-discover-agent-configs)
  - [Step 3: dry-run before touching anything](#step-3-dry-run-before-touching-anything)
  - [Step 4: run init for real](#step-4-run-init-for-real)
  - [Step 5: issue your first ticket](#step-5-issue-your-first-ticket)
  - [Step 6: one use, replay denied](#step-6-one-use-replay-denied)
  - [Step 7: task tickets and child tickets](#step-7-task-tickets-and-child-tickets)
  - [Step 8: wrap a real process](#step-8-wrap-a-real-process)
  - [Step 9: refresh the launcher after an upgrade](#step-9-refresh-the-launcher-after-an-upgrade)
  - [Step 10: offboarding, lost laptops and honeypots](#step-10-offboarding-lost-laptops-and-honeypots)
  - [Step 11: run the real tests](#step-11-run-the-real-tests)
- [Config discovery and env migration](#config-discovery-and-env-migration)
- [Command reference](#command-reference)
- [First run and AGENTVAULT_HOME](#first-run-and-agentvault_home)
- [Real test evidence](#real-test-evidence)
- [Security model](#security-model)
- [Known limits](#known-limits)
- [FAQ](#faq)
- [Repository layout](#repository-layout)
- [Roadmap](#roadmap)
- [Contributing and security](#contributing-and-security)
- [License](#license)

</details>

---

## The problem

When you connect Claude Code, Cursor, Codex or Qoder to a database, GitHub, a browser or an internal API over MCP, the common pattern is to write the secret into the config:

```json
{
  "mcpServers": {
    "postgres": {
      "command": "npx",
      "args": ["@modelcontextprotocol/server-postgres"],
      "env": {
        "DATABASE_URL": "postgres://app:real-password@prod:5432/app",
        "API_KEY": "sk-live-..."
      }
    }
  }
}
```

Three practical problems follow:

1. **The config is a plaintext secret store.** Copy it, screenshot it, commit it — the secret travels with it.
2. **Access has no time boundary.** Once an agent has the value, it has it forever. "Read-only for 15 minutes" depends on human discipline.
3. **Incident response has no answers.** Which key was used, when, by which agent? There is no audit, no revocation, no one-use semantics.

AgentVault changes the path:

```text
long-lived secret  ──►  OS keystore  ──►  short-lived ticket (1 use)  ──►  agent process
       ▲                                      │
       └──────── audit / revoke / rotate ◄────┘
```

The config no longer contains plaintext. The agent receives an `av1...` ticket. If the ticket expires, is used up, is revoked, or a global kill switch fires, resolve is denied.

---

## Understand it in 30 seconds

<p align="center">
  <img src="docs/assets/diagrams/architecture.svg" alt="AgentVault architecture" width="100%">
</p>

Three roles:

| Role | Does | Does not |
|---|---|---|
| **Vault** | Keeps long-lived secrets in the OS keystore; signs and verifies tickets | Hand long-lived secrets to agents |
| **Broker** | Listens on `127.0.0.1` only; handles issue/resolve/revoke/rotate | Listen on the network; act as a remote service |
| **Agent wrapper** | `agentvault run` starts the MCP server and injects a ticket or short-lived value | Change the MCP protocol of the wrapped server |

The shortest possible run:

```bash
# 1. Store a long-lived secret (it never enters the config)
agentvault secret add demo/token --value 'sk-test-demo-123' --kind env

# 2. Issue a 60-second, one-use ticket
agentvault issue --secret demo/token --ttl 60s

# 3. First resolve succeeds; the second is denied
agentvault resolve --ticket av1...
```

---

## What it is, and what it is not

**It is:**

- A local CLI + local broker **credential lifecycle layer**.
- A migration tool that moves plaintext env from MCP configs into the **OS keystore** and rewrites commands to `agentvault run ...`.
- A ticket system with **TTL, max_uses, task tickets, child tickets, renew, rotate and revoke**.
- A local observability layer with **JSONL audit, honeypot canaries and global revocation**.

**It is not:**

- A sandbox. It does not stop a malicious process running as the same OS user from reading files.
- A network proxy. It does not provide egress control or DLP.
- A cloud KMS. No account, no upload of secrets.
- A zero-trust silver bullet. It is a practical, auditable, revocable local layer.

---

## Supported agents

Path discovery is not guesswork: it is a **static per-client registry with project-first ordering**. Every candidate is printed by `init --list` and `doctor`, including whether it exists, how many servers it has, and how many are already wrapped.

| Agent | Candidate configs | Write format |
|---|---|---|
| **Claude Code** | project `.mcp.json` → `~/.claude.json` → `~/.claude/mcp.json` | JSON `mcpServers` |
| **Cursor** | project `.cursor/mcp.json` → `~/.cursor/mcp.json` | JSON `mcpServers` |
| **Codex** | `~/.codex/config.toml` | TOML (inline env, env sub-table, multi-line args) |
| **Gemini CLI** | project `.gemini/settings.json` → `~/.gemini/settings.json` | JSON `mcpServers` |
| **Windsurf** | project `.windsurf/mcp.json` → `~/.codeium/windsurf/mcp_config.json` | JSON `mcpServers` |
| **Cline** | VS Code / Cursor / Insiders `globalStorage/.../cline_mcp_settings.json` | JSON `mcpServers` |
| **Qoder** | project `.qoder/settings.json` → `.qoder/settings.local.json` → `.mcp.json` → `~/.qoder/settings.json` | JSON `mcpServers` |
| **Other MCP clients** | any `mcpServers` JSON via `--path` | JSON `mcpServers` |

> There is no recursive disk scan. Only registry candidates plus an explicit `--path`. That keeps behavior predictable and avoids touching unrelated files.

Unknown fields (`type`, `cwd`, `disabled`, `headers`, `timeout`, `trust`, `autoApprove`, ...) are preserved on rewrite; the config is not flattened into command + args.

---

## Five layers

<p align="center">
  <img src="docs/assets/diagrams/security-layers.svg" alt="Security layers" width="100%">
</p>

| Layer | What it does | v0.1 |
|---|---|---|
| **L0 secret custody** | OS keystore; device/user + hardware fingerprint binding; signed tickets | ✅ implemented |
| **L1 tickets + broker** | command wrapping, local broker, short tickets, task tickets, renew, rotate, revoke | ✅ implemented |
| **L2 sandbox** | process isolation / microVM | ⏳ later |
| **L3 egress control** | egress allowlist / DLP | ⏳ later |
| **L4 audit + honeypots** | append-only JSONL, canaries, global revocation | ✅ implemented |

L0 + L1 already solve the three practical issues: no plaintext in configs, access with a time boundary, and incident response with revocation. L2/L3 are larger projects and are not pretended to be complete.

---

## Install

### Option A: build from source (recommended for now)

Requires stable Rust (edition 2021) on Windows, macOS or Linux.

```bash
git clone https://github.com/Seramys/agentvault.git
cd agentvault
cargo build --release
```

Artifacts:

- Windows: `target\release\agentvault.exe`
- macOS / Linux: `target/release/agentvault`

Put the binary in a stable directory (for example `~/.local/bin` or `C:\Tools\agentvault`), then persist the user env:

```bash
agentvault env install --dry-run   # inspect first
agentvault env install             # user-scope AGENTVAULT_HOME + PATH
```

> Only new terminals / new login sessions see the change. Restart an already-open shell.

### Option B: `cargo install`

```bash
cargo install --path crates/cli
agentvault --version
```

### Option C: GitHub Releases

Once releases are published, download the binary for your platform. For v0.1, building from source is the primary path so the binary matches this documentation.

---

## Quick start

> Every example below uses `--home` to point at an isolated directory. Try it safely, then remove `--home` for real use.

### 1. Store a long-lived secret

```bash
agentvault --home "$HOME/.agentvault-demo" secret add demo/token \
  --value 'sk-test-demo-123' --kind env
```

```json
{
  "replaced": false,
  "secret": "demo/token",
  "stored": true,
  "value_changed": false
}
```

### 2. Inspect it

```bash
agentvault --home "$HOME/.agentvault-demo" secret list
```

```text
demo/token                   kind=env    epoch=0   honeypot=false revoked=false
```

### 3. Issue a ticket

```bash
agentvault --home "$HOME/.agentvault-demo" issue --secret demo/token --ttl 60s
```

```text
av1.eyJ2IjoxLCJqdGkiOiI1ZGU5OTY1ZC0...
kind=leaf jti=5de9965d-... expires_in=60s uses=1 hard_exp=...
```

### 4. Use it once

```bash
agentvault --home "$HOME/.agentvault-demo" resolve --ticket av1...
```

The first call returns the value; the second returns:

```text
error: denied: ticket revoked
```

### 5. Revoke everything

```bash
agentvault --home "$HOME/.agentvault-demo" revoke --all
```

`revoke --all` bumps the global epoch. Every old ticket dies immediately; tickets issued afterwards use the new epoch.

---

## Step-by-step guide

This follows the real order of use. Each step includes commands, expected output and the usual pitfalls.

### Step 0: see the whole picture

<p align="center">
  <img src="docs/assets/diagrams/architecture.svg" alt="Architecture overview" width="100%">
</p>

One pipeline:

```text
init discovers the config → dry-run shows the plan → backup → migrate env → wrap the command → verify
```

`init` does not "change whatever it finds". It only touches the servers it plans to wrap; URL-only servers, already wrapped servers and unparsable servers are skipped with a reason.

### Step 1: install and inspect

```bash
agentvault --version
agentvault doctor
```

```text
home:      C:\Users\you\AppData\Local\agentvault
home from: platform default
default:   C:\Users\you\AppData\Local\agentvault
writable:  yes
exe:       C:\Tools\agentvault\agentvault.exe
config:    missing (not created yet)
broker:    offline
keystore:  windows-dpapi
env:       AGENTVAULT_HOME is not persisted (run `agentvault env install`)
path:      missing (install dir C:\Tools\agentvault)
[x] codex    C:\Users\you\.codex\config.toml (2 server(s), 0 to wrap)
```

Check four things:

- `home`: where state lives;
- `keystore`: DPAPI / Keychain / Secret Service / encrypted file;
- `broker`: whether the local broker is online;
- the `[x]` lines: which agent configs were found and how many servers are left to wrap.

### Step 2: discover agent configs

```bash
agentvault init --list
```

```text
claude:
  [ ] .mcp.json (missing)
  [ ] C:\Users\you\.claude.json (missing)
cursor:
  [ ] .cursor\mcp.json (missing)
  [ ] C:\Users\you\.cursor\mcp.json (missing)
codex:
  [x] C:\Users\you\.codex\config.toml (2 server(s), 0 to wrap)
qoder:
  [x] C:\Users\you\.qoder\settings.json (0 server(s), 0 to wrap, 0 wrapped)

auto would pick: codex (C:\Users\you\.codex\config.toml)
```

Details:

- **Project first**: run in a repo and `.cursor/mcp.json` / `.mcp.json` come before user-global configs.
- `--agent auto` only selects among existing candidates; it does not create files.
- Target a specific file: `agentvault init --path .\mcp.json`.
- Force a client: `agentvault init --agent qoder`.

### Step 3: dry-run before touching anything

This is the most important safety valve. **Read the plan before writing.**

Create a test config (or use a copy of your real one):

```json
{
  "mcpServers": {
    "postgres": {
      "command": "npx",
      "args": ["@modelcontextprotocol/server-postgres"],
      "env": {
        "DATABASE_URL": "postgres://demo:demo@localhost/db",
        "API_KEY": "sk-test-demo-123",
        "PASS_THROUGH": "${HOME}/.config",
        "LOG_LEVEL": "info",
        "MAX_TOKENS": "4096"
      }
    }
  }
}
```

Run:

```bash
agentvault init --path .\mcp.json --dry-run --no-env
```

Actual output:

```text
agent:        generic
config:       .\mcp.json
would wrap 1 server(s):
  - postgres: npx @modelcontextprotocol/server-postgres -> agentvault run --name postgres
    migrate env: API_KEY, DATABASE_URL
    keep env:    PASS_THROUGH (reference/override)
    unchanged env: LOG_LEVEL, MAX_TOKENS
dry run: no files were changed
```

<p align="center">
  <img src="docs/assets/diagrams/env-migration.svg" alt="Env migration decision flow" width="100%">
</p>

The four buckets:

| Output | Meaning | What happens to the config |
|---|---|---|
| `migrate env` | looks secret | value goes to the keystore, env entry removed, injected at runtime via `--env KEY=secret:NAME` |
| `keep env` | `${...}` / `$VAR` / `%VAR%` / `{{...}}` reference, or forced with `--keep-env` | left verbatim for the MCP client to expand |
| `unchanged env` | ordinary setting | left verbatim; not stored |
| `skip` | URL-only, already wrapped, or unparsable | untouched |

> **Note**: reference detection now covers embedded references such as `${HOME}/.config`, `$HOME/.config`, `%USERPROFILE%\...` and `prefix-${ENV}-suffix`. The earlier check only recognized whole-value references and mis-migrated `${HOME}/.config`; this is fixed and unit-tested.

If the heuristic is wrong, override it:

```bash
# Force migration even if the key looks ordinary
agentvault init --path .\mcp.json --migrate-env SESSION_ID

# Keep a key even if it looks secret
agentvault init --path .\mcp.json --keep-env INTERNAL_HINT
```

### Step 4: run init for real

After the dry-run plan looks right:

```bash
agentvault init --path .\mcp.json
```

What happens:

1. The local broker is ensured (disable with `--no-autostart`);
2. a timestamped backup is written: `mcp.json.bak.<timestamp>`;
3. only the planned env values are migrated to the keystore;
4. the command is rewritten to `agentvault run --name postgres -- ...`;
5. the config is written back and re-parsed for verification;
6. the written file and backup path are printed.

<p align="center">
  <img src="docs/assets/diagrams/init-flow.svg" alt="init flow" width="100%">
</p>

The wrapped JSON shape:

```json
{
  "mcpServers": {
    "postgres": {
      "command": "C:\\Tools\\agentvault\\agentvault.exe",
      "args": [
        "--home", "C:\\Users\\you\\AppData\\Local\\agentvault",
        "run", "--name", "postgres",
        "--env", "API_KEY=secret:postgres/API_KEY",
        "--env", "DATABASE_URL=secret:postgres/DATABASE_URL",
        "--",
        "npx", "@modelcontextprotocol/server-postgres",
        "postgres://demo:demo@localhost/db"
      ],
      "env": {
        "PASS_THROUGH": "${HOME}/.config",
        "LOG_LEVEL": "info",
        "MAX_TOKENS": "4096"
      }
    }
  }
}
```

Details worth knowing:

- The wrapped `command` is an **absolute path** (`launcher_path()`), not bare `agentvault`, so it does not depend on the agent's PATH.
- The wrapper carries an explicit `--home`, so different shells cannot silently use different vaults.
- Ordinary env is preserved; only planned keys are removed.
- A second `init` is a no-op (idempotent): no double wrapping, no re-migration.
- Migrated values are resolved at runtime via `--env KEY=secret:NAME`; the MCP server needs no code changes.

### Step 5: issue your first ticket

```bash
agentvault issue --secret postgres/DATABASE_URL --ttl 15m --scope read
```

```text
av1.eyJ2IjoxLCJqdGkiOiI...
kind=leaf jti=... expires_in=900s uses=1 hard_exp=...
```

An `av1` ticket is HMAC-SHA256 signed. Claims include:

- `jti`: ticket ID used by audit and revocation;
- `sid`: secret ID;
- `scope`: permission scope (default `read`);
- `iat` / `exp` / `hard_exp`;
- `epoch` (secret rotation generation) and `gepoch` (global revocation generation);
- `max_uses` / `kind` / `parent` / `renewable`.

Use `--json` for scripts:

```bash
agentvault issue --secret postgres/DATABASE_URL --ttl 15m --json
```

### Step 6: one use, replay denied

```bash
TICKET=$(agentvault issue --secret postgres/DATABASE_URL --ttl 60s)
agentvault resolve --ticket "$TICKET"   # succeeds, returns the value
agentvault resolve --ticket "$TICKET"   # denied
```

<p align="center">
  <img src="docs/assets/terminal/replay-denied.svg" alt="Replay denied" width="100%">
</p>

Actual second output:

```text
error: denied: ticket revoked
```

This is the point, not a client bug: the default is `max_uses=1`, and using it revokes it. Need more uses? Be explicit:

```bash
agentvault issue --secret postgres/DATABASE_URL --max-uses 3 --ttl 5m
```

### Step 7: task tickets and child tickets

A two-hour agent task should not hold a two-hour, reusable production credential. Do this instead:

```bash
# Parent: short TTL, renewable, 2-hour hard cap
PARENT=$(agentvault issue --secret postgres/DATABASE_URL \
  --task --ttl 30s --hard-ttl 2h --json | jq -r .ticket)

# Before each tool call, mint a 60-second leaf
CHILD=$(agentvault child --parent "$PARENT" --ttl 60s)
agentvault resolve --ticket "$CHILD"
```

Rules:

- A child's expiry can never exceed the parent's `hard_exp`.
- `renew` slides the parent's `exp`, but never beyond `hard_ttl`.
- Revoking the parent invalidates children too.
- `run --session` creates a renewable parent and revokes it when the child process exits.

<p align="center">
  <img src="docs/assets/diagrams/ticket-lifecycle.svg" alt="Ticket lifecycle" width="100%">
</p>

### Step 8: wrap a real process

```bash
agentvault run --name postgres -- npx @modelcontextprotocol/server-postgres
```

Or inject several migrated secrets at once:

```bash
agentvault run --name postgres \
  --env DATABASE_URL=secret:postgres/DATABASE_URL \
  --env API_KEY=secret:postgres/API_KEY \
  -- npx @modelcontextprotocol/server-postgres
```

Behavior:

- `--name NAME`: if a secret with **exactly that name** exists, export `AGENTVAULT_TICKET` to the child.
- `--env KEY=secret:NAME`: resolve `NAME` before spawning and inject the value as ordinary env.
- `--session`: create a renewable parent and export:
  - `AGENTVAULT_PARENT_TICKET`
  - `AGENTVAULT_PARENT_FILE`
- When the child exits, the session parent is revoked and the background renewal stops.

> Why does `init` not export `AGENTVAULT_TICKET` automatically? Most MCP servers do not know that variable; they only read `DATABASE_URL`-style env. `init` therefore uses the compatible "inject values at runtime" path. Ticket-aware servers should use `run --name <same-name secret>` or `--session`.

### Step 9: refresh the launcher after an upgrade

Moved from a debug build to release, or relocated the install? No need to re-migrate env:

```bash
agentvault init --agent codex --refresh --dry-run
agentvault init --agent codex --refresh
```

<p align="center">
  <img src="docs/assets/terminal/refresh-launcher.svg" alt="Refresh launcher" width="100%">
</p>

It changes only:

- the launcher path of already wrapped servers;
- the `--home` value.

It does **not** touch env, the original command after `run`, or suffixes such as `--no-autostart`. JSON and Codex TOML are supported; every file is backed up first.

### Step 10: offboarding, lost laptops and honeypots

**Global kill switch:**

```bash
agentvault revoke --all
```

**Block new issuance for one secret:**

```bash
agentvault revoke --secret postgres/DATABASE_URL
```

**Revoke one ticket (jti or full token):**

```bash
agentvault revoke --ticket 5de9965d-...
agentvault revoke --ticket av1...
```

**Rotate:**

```bash
agentvault rotate --secret postgres/DATABASE_URL
```

Rotation generates a new value, bumps the secret epoch so every old ticket fails on resolve, and clears the secret's revoked state.

**Honeypot:**

```bash
agentvault honeypot add prod/canary --json
# Put the returned token in a .env/config an agent should never read
agentvault honeypot check <token>
```

<p align="center">
  <img src="docs/assets/terminal/honeypot-trip.svg" alt="Honeypot trip" width="100%">
</p>

Actual output:

```text
hit=true honeypot=prod/canary revoked_all=true
```

A hit triggers global revocation. A later resolve sees:

```text
error: denied: ticket was invalidated by a global revocation
```

**Audit:**

```bash
agentvault audit --tail 20
```

```text
[1795261677681] ticket.resolve     actor=cli  secret=demo/token  ticket=a705ff08-...  ok
[1795261677724] ticket.resolve     actor=cli  secret=demo/token  ticket=a705ff08-...  revoked
[1795261686286] honeypot.triggered actor=cli  secret=canary      ticket=-             blocked
```

The audit trail is append-only JSONL. Rotation keeps the previous file.

### Step 11: run the real tests

```bash
cargo test --workspace
cargo build --release
```

Local result (Windows 11, release binary):

```text
e2e:        9 passed
adapters:  16 passed
audit:      1 passed
core:      10 passed
keystore:   2 passed
total:     38 passed, 0 failed
```

<p align="center">
  <img src="docs/assets/diagrams/test-evidence.svg" alt="Test evidence" width="100%">
</p>

Coverage is not a happy path:

- JSON / Codex / Qoder config rewrites and idempotence;
- unknown-field preservation, multi-line TOML args;
- one-use tickets, expired tickets, replay denial;
- task hard-TTL caps, renew does not shorten;
- revoke token / jti / all, rotation invalidates old tickets;
- honeypot-triggered global revocation;
- autostart toggles, single instance, broken pipe;
- refresh against a real Codex config copy;
- user-scope `env install`, idempotence, uninstall and restore.

> There is no GUI-level Cursor/Qoder tool invocation automation, no macOS/Linux runtime validation, and no sandbox/egress. These are listed under known limits; they are not claimed as done.

---

## Config discovery and env migration

### Path discovery: registry, not heuristics

- Each client's candidates are a **static registry**, with project paths before user-global paths.
- `init --list` prints every candidate, existence, server count, wrapped count and what `auto` would pick.
- `--agent` forces a client; `--path` targets a file.
- No recursive scan, no browser history, no "maybe it is somewhere in this directory".

### Env migration: heuristic, visible, overridable

The decision order:

1. **Reference guard**: `${...}`, `$VAR`, `%VAR%`, `{{...}}`, including embedded forms such as `${HOME}/.config` → never migrate.
2. **Key markers**: `TOKEN`, `SECRET`, `PASSWORD`, `API_KEY`, `DATABASE_URL`, `DSN`, `WEBHOOK`, `COOKIE`, etc.
3. **Value shapes**: `sk-`, `ghp_`, JWTs, PEM, `postgres://user:pass@...`, long random strings, etc.

Heuristics will be wrong sometimes. Three safety valves:

```bash
agentvault init --dry-run                 # inspect
agentvault init --migrate-env KEY         # force migration
agentvault init --keep-env KEY            # force keep
```

`init --dry-run` prints `migrate env` / `keep env` / `unchanged env` / `skip`. Real writes always back up first.

### Backups and failure recovery

- Before writing: `mcp.json.bak.<timestamp>` / `config.toml.bak.<timestamp>`.
- After writing: re-parse and verify the wrapped result; restore from backup on failure.
- Backups contain the original plaintext. Delete them once the new config is verified.
- `init --refresh` also backs up, but does not touch env.

---

## Command reference

| Command | Purpose | Key options |
|---|---|---|
| `init` | Discover and wrap MCP configs; migrate secret env | `--dry-run` `--list` `--refresh` `--agent` `--path` `--migrate-env` `--keep-env` `--no-env` `--rebind` |
| `doctor` | Diagnose home, broker, keystore and agent configs | `--home` |
| `env show` | Show home source and persisted values | `--home` |
| `env install` | Persist `AGENTVAULT_HOME` + PATH | `--scope user\|machine` `--no-path` `--dry-run` |
| `env uninstall` | Remove persisted values | `--scope user\|machine` `--dry-run` |
| `secret add` | Store a long-lived secret | `--value` `--stdin` `--kind` `--honeypot` |
| `secret list` | List secrets (never values) | `--home` |
| `secret remove` | Delete a secret | `<NAME>` |
| `secret unrevoke` | Allow a revoked name to be issued again without rotating | `<NAME>` |
| `issue` | Issue a short-lived ticket | `--secret` `--ttl` `--scope` `--max-uses` `--task` `--hard-ttl` `--json` |
| `resolve` | Exchange a ticket (exactly once by default) | `--ticket` `--json` |
| `child` | Mint a leaf ticket from a parent | `--parent` / `--parent-file` `--ttl` `--sub` `--max-uses` |
| `renew` | Slide a parent expiry, capped by hard-ttl | `--ticket` `--ttl` |
| `run` | Wrap a process and inject a ticket or resolved values | `--name` `--env KEY=secret:NAME` `--session` `--ttl` `--hard-ttl` |
| `revoke` | Revoke a ticket / secret / everything | `--ticket` `--secret` `--all` |
| `rotate` | Rotate a value and invalidate old tickets | `--secret` |
| `status` | Show secrets, bound agents and live tickets | `--json` |
| `audit` | Read the JSONL audit trail | `--tail N` `--json` |
| `honeypot add/list/check` | Manage honeypot canaries | `--json` |
| `daemon` | Run the broker in the foreground | `--detach` |
| `stop` | Stop a background broker | `--home` |

Commands that need a broker autostart a detached one by default. Disable with `--no-autostart` or `AGENTVAULT_NO_AUTOSTART=1`.

---

## First run and AGENTVAULT_HOME

Default state directory:

| Platform | Path |
|---|---|
| Windows | `%LOCALAPPDATA%\agentvault` |
| macOS | `~/Library/Application Support/agentvault` |
| Linux | `$XDG_DATA_HOME/agentvault` or `~/.local/share/agentvault` |

Precedence: `--home` > `AGENTVAULT_HOME` > platform default.

The first real `init` (using the platform default, not a `--home` override) will:

- persist `AGENTVAULT_HOME` at **user scope**;
- add the binary directory to the **user PATH**;
- never silently write machine-wide variables; use `env install --scope machine` explicitly (usually elevated).

Management:

```bash
agentvault env show
agentvault env install --dry-run
agentvault env install
agentvault env install --scope machine
agentvault env uninstall
agentvault init --no-env   # first init must not touch the environment
```

Notes:

- Already-open terminals do not refresh; new terminals / new login sessions do.
- With `--home` or `AGENTVAULT_HOME`, first init does not persist that temporary path as the global default.
- Wrapped servers carry an explicit `--home`, so the same vault is used even if the agent's PATH/env differs.

---

## Real test evidence

This was not "a few unit tests and call it done". On a real Windows 11 machine:

- **38 Rust tests**: `cargo test --workspace` all green.
- **150 isolated CLI checks**: each with its own `--home`, covering init, tickets, revoke, rotate, env, doctor, autostart and broken pipe.
- **Real Codex config copy**: copied the real `config.toml`, added a test server, ran init/refresh, then verified launcher and home with `codex mcp get`.
- **Real Cursor / Qoder probing**: verified the path registry, empty configs and project-first precedence.
- **Real env write and restore**: user-scope `env install`, idempotence, `env uninstall` restore.
- **Real attack semantics**: replay denied, expiry denied, global revocation, rotation invalidation, honeypot trigger.

<p align="center">
  <img src="docs/assets/terminal/replay-denied.svg" alt="Replay denied" width="100%">
</p>

<p align="center">
  <img src="docs/assets/terminal/honeypot-trip.svg" alt="Honeypot trip" width="100%">
</p>

---

## Security model

<p align="center">
  <img src="docs/assets/diagrams/security-layers.svg" alt="Security model" width="100%">
</p>

### L0: secret custody

- Windows: DPAPI (invoked through PowerShell, no extra native build dependency).
- macOS: Keychain.
- Linux: Secret Service; headless falls back to a 0600 encrypted file.
- `status` prints the active backend.

### Device and hardware binding

- Device fingerprint: hostname + user.
- `init --rebind` records a hardware fingerprint (WMI UUID / IOPlatformUUID / DMI product UUID).
- The broker refuses to start on mismatch.
- This is not TPM sealing; TPM is a later enterprise direction.

### Tickets

- HMAC-SHA256, with a key derived from the master key + global revocation epoch.
- `revoke --all` bumps the global epoch; all old tickets die at once.
- Secret rotation bumps the secret epoch; old tickets fail on resolve.
- Default `max_uses=1`; exhaustion revokes automatically.
- Task tickets have `hard_exp`; `renew` cannot cross it.
- Child expiry is capped by the parent's `hard_exp`.
- `revoke --ticket` accepts a jti or a full token.

### Local broker

- Binds `127.0.0.1` only.
- Single-instance lock.
- `admin.token` uses an owner-only ACL (`icacls` on Windows, 0600 on Unix).
- Decrypted secrets are cached in memory; DPAPI is not invoked on every tool call.
- After the broker exits, the next command that needs it autostarts one.

### Audit and honeypots

- Append-only JSONL: timestamp, actor, event, secret, ticket, result.
- `audit --tail N` to read.
- Rotation keeps the old audit file.
- Honeypot tokens are unique; a hit triggers global revocation.
- Honeypot checks never reveal the real secret.

---

## Known limits

These are real v0.1 limits, not footnotes:

1. **Not a sandbox**: a malicious process running as the same OS user can still read user files, `admin.token` and backups.
2. **The broker is local TCP**: not a remote service; any local process that can read `admin.token` can administer it.
3. **Device binding is not TPM**: it is a fingerprint check, not hardware sealing.
4. **Unmigrated plaintext is out of scope**: v0.1 handles configs it knows; secrets in scripts, `.env` files and logs are not covered.
5. **Env migration is heuristic**: key markers + value shapes can be wrong; use dry-run + `--migrate-env` / `--keep-env`.
6. **Codex TOML support is bounded**: quoted server names, inline/env sub-tables and multi-line args work; bare nested tables other than `.env` are ignored.
7. **JSON must be strict**: JSONC comments/trailing commas are rejected with a clear error; UTF-8 BOM is accepted.
8. **`~/.claude.json` is top-level only**: `projects.*.mcpServers` are not rewritten automatically; use `--path` on a project `.mcp.json`.
9. **`init` injects values, not tickets, by default**: compatible with stock MCP servers; ticket-aware use cases should use `run --name <same-name secret>` or `--session`.
10. **No GUI automation**: Cursor/Qoder GUI-level tool invocation is not automated; CLI and config layers are verified.
11. **No macOS/Linux runtime validation yet**: code and unit tests are cross-platform, but the real-machine run is Windows 11.
12. **Web console deferred**: this repository ships the CLI only; web code is not part of this release.

---

## FAQ

### Q1: How is this different from putting the token in a system env var?

A system env var is still a long-lived plaintext value readable by any same-user process, with no TTL, use limit, revocation or audit. AgentVault keeps the long-lived value in the OS keystore; the agent receives a short-lived ticket that expires, runs out, or can be revoked.

### Q2: Can `init` break my config?

It dry-runs a plan, backs up, only touches planned servers, re-parses after writing, and restores on failure. Unknown fields are preserved. Still, copy a production config first, or try `--path` on a copy.

### Q3: Why wasn't `${HOME}/.config` migrated?

On purpose. It is a reference, not a secret; migrating it would turn an expansion into a literal and break the MCP client. It appears under `keep env`.

### Q4: Why does the agent get a value instead of a ticket after `init`?

Most MCP servers only read ordinary env such as `DATABASE_URL` / `API_KEY`. `init` uses the compatible path: exchange a ticket for a value at startup, then inject the value. Ticket-aware custom servers should use `run --name <same-name secret>` or `--session`.

### Q5: Can I keep working after `revoke --all`?

Yes. Global revocation bumps the epoch; newly issued tickets use the new epoch and work normally. All old tickets are dead.

### Q6: What is the difference between revoke and rotate?

- `revoke --secret`: stops issuance for that secret; existing tickets also fail; the value remains.
- `rotate --secret`: generates a new value, bumps the epoch, invalidates old tickets, and clears the secret's revoked state.
- `revoke --all`: invalidates every old ticket for every secret at once.

### Q7: Does the broker slow down every tool call?

The broker caches decrypted secrets in memory; DPAPI runs once at startup. Normal issue/resolve is local TCP + HMAC verification, which is cheap.

### Q8: Can I connect my own MCP server?

Yes, two ways:

```bash
# 1. Ordinary env injection (stock server compatible)
agentvault run --name my-server --env API_KEY=secret:my/API_KEY -- node server.js

# 2. Ticket-aware
agentvault run --name my/API_KEY -- node server.js
# the child reads AGENTVAULT_TICKET and calls agentvault resolve
```

### Q9: Why not a web console first?

The CLI is the core and the smallest trusted surface. Make config migration, ticket semantics, revocation and audit solid first; this repository ships the CLI only.

### Q10: Can a team share one vault?

v0.1 is a single-machine, single-user local vault. Team sharing, central policy and remote audit are future work. Do not put the same home directory on a network share.

---

## Repository layout

```text
crates/
  core/      domain types, token signing, paths, JSON store
  keystore/  OS keystore + encrypted-file fallback + derivation
  audit/     append-only JSONL audit
  adapters/  JSON mcpServers + Codex TOML discovery and rewrite
  broker/    local TCP broker, ticket lifecycle, client
  cli/       agentvault binary
docs/
  assets/    logo, diagrams, terminal evidence
```

Key entry points:

- `crates/cli/src/main.rs`: command parsing, init/issue/run/doctor, etc.
- `crates/adapters/src/lib.rs`: path registry, JSON plan/apply/refresh, env heuristic.
- `crates/adapters/src/codex.rs`: Codex TOML parsing and rewriting.
- `crates/broker/src/lib.rs`: local broker and ticket lifecycle.
- `crates/core/src/token.rs`: HMAC ticket signing/verification.
- `crates/keystore/src/lib.rs`: DPAPI / Keychain / Secret Service / file backend.

---

## Roadmap

- [x] L0: OS keystore, device binding, hardware fingerprint, signed tickets
- [x] L1: local broker, short tickets, task/child/renew, rotate/revoke
- [x] L4: JSONL audit, honeypot canaries, global revocation
- [x] `init --refresh`: painless launcher/home update after an upgrade
- [x] Embedded env reference guard (`${HOME}/...`, `%VAR%\...`)
- [ ] L2: process sandbox / microVM
- [ ] L3: egress allowlist / DLP
- [ ] Web console (separate repo / later version)
- [ ] macOS/Linux real-machine validation and CI coverage
- [ ] TPM / Secure Enclave binding
- [ ] Team policy and central audit

---

## Contributing and security

- Contribution guide: [CONTRIBUTING.md](CONTRIBUTING.md)
- Security policy and vulnerability reporting: [SECURITY.md](SECURITY.md)
- Never paste real tokens, cookies or production configs into issues/PRs; use `sk-test-...`.
- Config rewrites must keep backups; do not commit `*.bak.*`.
- The Chinese README is the source of truth; update EN/JA/KO when behavior changes.

---

## License

Apache License 2.0. See [LICENSE](LICENSE).

---

<p align="center">
  <sub>Built for agents that should borrow credentials, not own them.</sub>
</p>

<p align="center">
  <a href="https://star-history.com/#Seramys/agentvault&Date">
    <img src="https://api.star-history.com/svg?repos=Seramys/agentvault&type=Date" alt="Star history" width="720">
  </a>
</p>