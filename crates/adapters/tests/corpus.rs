//! Compatibility corpus and round-trip tests for every config format.
//!
//! Each fixture is parsed, rewritten, re-parsed and asserted semantically:
//! wrapping is idempotent, url-only servers stay untouched, and unrelated
//! top-level keys survive the rewrite.

use agentvault_adapters::codex::{CodexConfig, CodexServer};
use agentvault_adapters::McpConfig;
use serde_json::Value;
use std::path::{Path, PathBuf};

const LAUNCHER: &str = "/opt/agentvault/agentvault";
const HOME: &str = "/home/demo/.agentvault";

fn fixture_dir(kind: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(kind)
}

fn fixtures(kind: &str, extension: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(fixture_dir(kind))
        .unwrap_or_else(|error| panic!("fixture directory {kind} is missing: {error}"))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some(extension))
        .collect();
    paths.sort();
    paths
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("av-corpus-{}-{label}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn codex_corpus_round_trips() {
    let files = fixtures("codex", "toml");
    assert!(
        files.len() >= 4,
        "expected at least four Codex version samples, found {files:?}"
    );

    for path in files {
        let label = path.display().to_string();
        let original = std::fs::read_to_string(&path).unwrap();
        let mut config = CodexConfig::parse(original.clone())
            .unwrap_or_else(|error| panic!("{label} failed to parse: {error}"));

        let before: Vec<CodexServer> = config.servers.clone();
        let planned = config.plan();
        let changes = config.apply_with_launcher(LAUNCHER, Some(Path::new(HOME)));
        assert_eq!(changes.len(), planned.len(), "{label}");

        let rendered = config.render();
        let reparsed = CodexConfig::parse(rendered.clone())
            .unwrap_or_else(|error| panic!("{label} failed to re-parse after rewrite: {error}"));

        // Wrapping a second time must be a no-op.
        assert!(
            reparsed.clone().apply().is_empty(),
            "{label} is not idempotent"
        );
        assert_eq!(reparsed.servers.len(), before.len(), "{label}");

        for old in &before {
            let new = reparsed
                .servers
                .iter()
                .find(|server| server.name == old.name)
                .unwrap_or_else(|| panic!("{label}: server `{}` disappeared", old.name));
            match (&old.command, old.unsupported.is_some()) {
                (_, true) => assert!(new.unsupported.is_some(), "{label}: {}", old.name),
                (Some(_), false) => assert!(
                    CodexConfig::is_wrapped(new),
                    "{label}: server `{}` was not wrapped",
                    old.name
                ),
                (None, false) => {
                    assert!(
                        new.command.is_none(),
                        "{label}: `{}` gained a command",
                        old.name
                    );
                    assert!(new.unsupported.is_none(), "{label}: {}", old.name);
                }
            }
            // Non-secret env keys must survive; secret keys must be stripped.
            for key in old.env.keys() {
                assert!(
                    !rendered.contains(&format!("{key} = \"sk-corpus-")),
                    "{label}: secret `{key}` was left in the file"
                );
            }
        }

        if label.ends_with("codex-2026-real.toml") {
            // The real-world sample exercises literals, quoted tables and
            // sub-tables that must not be normalized away.
            assert!(rendered.contains("[desktop]"), "{rendered}");
            assert!(rendered.contains("[windows]"), "{rendered}");
            assert!(
                rendered.contains("[plugins.\"browser@openai-bundled\"]"),
                "{rendered}"
            );
            assert!(rendered.contains("notify = ["), "{rendered}");
            assert!(rendered.contains("source = '\\\\?\\C:"), "{rendered}");
            assert!(rendered.contains("trust_level = \"trusted\""), "{rendered}");
            assert!(
                rendered.contains("appearanceTheme = \"light\""),
                "{rendered}"
            );
            assert!(!rendered.contains("DATABASE_URL ="), "{rendered}");
            assert!(rendered.contains("LOG_LEVEL"), "{rendered}");
        }
        if label.ends_with("codex-0.50-rmcp.toml") {
            assert!(rendered.contains("[mcp_servers.remote.http_headers]"));
            assert!(rendered.contains("bearer_token_env_var = \"REMOTE_MCP_TOKEN\""));
        }
    }
}

#[test]
fn json_corpus_round_trips() {
    let files = fixtures("json", "json");
    assert!(files.len() >= 4, "expected JSON samples, found {files:?}");

    for path in files {
        let label = path.display().to_string();
        let original = std::fs::read_to_string(&path).unwrap();
        let mut config = McpConfig::load(&path).unwrap();
        let before: Value = serde_json::from_str(&original).unwrap();

        agentvault_adapters::apply_with_launcher(&mut config, LAUNCHER, Some(Path::new(HOME)));
        let rendered = serde_json::to_string_pretty(&config).unwrap();
        let mut reparsed: McpConfig = serde_json::from_str(&rendered)
            .unwrap_or_else(|error| panic!("{label} failed to re-parse: {error}"));
        let after: Value = serde_json::from_str(&rendered).unwrap();

        assert!(
            agentvault_adapters::apply_with_launcher(
                &mut reparsed,
                LAUNCHER,
                Some(Path::new(HOME))
            )
            .is_empty(),
            "{label} is not idempotent"
        );

        let before_servers = before["mcpServers"].as_object().unwrap();
        let after_servers = after["mcpServers"].as_object().unwrap();
        assert_eq!(before_servers.len(), after_servers.len(), "{label}");
        for (name, original_server) in before_servers {
            let rewritten = after_servers
                .get(name)
                .unwrap_or_else(|| panic!("{label}: server `{name}` disappeared"));
            // Fields the adapter does not interpret (`type`, `disabled`,
            // `timeout`, `cwd`, `headers`, `autoApprove`, ...) must survive.
            let mut left = original_server.as_object().unwrap().clone();
            let mut right = rewritten.as_object().unwrap().clone();
            left.remove("command");
            left.remove("args");
            left.remove("env");
            right.remove("command");
            right.remove("args");
            right.remove("env");
            assert_eq!(Value::Object(left), Value::Object(right), "{label}: {name}");
        }
        // Unrelated top-level keys are preserved by the flattened `extra` map.
        for (key, value) in before.as_object().unwrap() {
            if key != "mcpServers" {
                assert_eq!(after.get(key), Some(value), "{label}: top-level `{key}`");
            }
        }
    }
}

#[test]
fn codex_save_preserves_bom_crlf_and_reloads() {
    let dir = temp_dir("codex-save");
    let path = dir.join("config.toml");
    let original = "\u{feff}# keep me\r\nmodel = \"gpt-5\"\r\n\r\n[mcp_servers.db]\r\ncommand = \"npx\"\r\nargs = [\r\n  \"-y\",\r\n  \"server.js\",\r\n]\r\nenv = { DATABASE_URL = 'postgres://demo:demo@localhost/demo', LOG_LEVEL = \"info\" }\r\n";
    std::fs::write(&path, original).unwrap();

    let mut config = CodexConfig::load(&path).unwrap();
    assert!(config.has_bom());
    assert!(config.uses_crlf());
    config.apply_with_launcher(LAUNCHER, Some(Path::new(HOME)));
    config.save(&path).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "BOM was dropped");
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("# keep me"), "{text}");
    assert!(!text.contains("DATABASE_URL ="), "{text}");
    assert!(text.contains("LOG_LEVEL"), "{text}");
    let without_crlf = text.replace("\r\n", "");
    assert!(!without_crlf.contains('\n'), "mixed newlines: {text:?}");

    let reloaded = CodexConfig::load(&path).unwrap();
    assert!(CodexConfig::is_wrapped(&reloaded.servers[0]));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn json_save_preserves_bom_crlf_and_reloads() {
    let dir = temp_dir("json-save");
    let path = dir.join("mcp.json");
    let original = "\u{feff}{\r\n  \"mcpServers\": {\r\n    \"db\": {\r\n      \"command\": \"node\",\r\n      \"args\": [\"server.js\"],\r\n      \"env\": { \"DATABASE_URL\": \"postgres://demo:demo@localhost/demo\", \"LOG_LEVEL\": \"info\" },\r\n      \"timeout\": 30\r\n    }\r\n  },\r\n  \"editor\": { \"fontSize\": 13 }\r\n}\r\n";
    std::fs::write(&path, original).unwrap();

    let mut config = McpConfig::load(&path).unwrap();
    agentvault_adapters::apply_with_launcher(&mut config, LAUNCHER, Some(Path::new(HOME)));
    config.save(&path).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "BOM was dropped");
    let text = String::from_utf8(bytes).unwrap();
    let without_crlf = text.replace("\r\n", "");
    assert!(!without_crlf.contains('\n'), "mixed newlines: {text:?}");

    let reloaded = McpConfig::load(&path).unwrap();
    assert!(agentvault_adapters::is_already_wrapped(
        &reloaded.mcp_servers["db"]
    ));
    assert_eq!(reloaded.mcp_servers["db"].extra["timeout"], 30);
    assert_eq!(reloaded.extra["editor"]["fontSize"], 13);

    let _ = std::fs::remove_dir_all(&dir);
}
