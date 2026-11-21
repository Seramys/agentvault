# Contributing

Thanks for helping make local agent credentials less fragile.

## Development

```bash
cargo fmt --all
cargo build --workspace
cargo test --workspace
```

The test suite builds the real CLI binary and covers JSON and Codex config
rewrites, ticket lifecycle, hard limits, revocation, rotation, autostart and
environment persistence. Do not replace these with mocks.

## Adding an agent adapter

1. Add the client to `AgentKind` and its candidate paths in
   `crates/adapters/src/lib.rs`.
2. Prefer project-local paths before user-global paths.
3. Preserve unknown fields; do not round-trip a config through a lossy struct.
4. Add a fixture and an idempotence test.
5. Update `init --list`, `doctor`, README tables and the language variants.

## Security and secrets

- Never commit real secrets, tokens, cookies or production configs.
- Use `sk-test-...` / `demo-value-...` only.
- A config rewrite must write a backup before mutating the file.
- Do not add a feature that prints resolved secret values outside `resolve`.

## Documentation

Keep the Chinese README as the source of truth and update
`README_EN.md`, `README_JA.md` and `README_KO.md` when behavior changes.

## Pull requests

Keep changes focused. Include the command(s) you ran and the actual output for
behavior changes. For UI/web work: the web console is currently deferred; keep
it out of this CLI-focused repository until the CLI surface is stable.