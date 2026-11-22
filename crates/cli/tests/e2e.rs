use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

static COUNTER: AtomicU64 = AtomicU64::new(0);
static TEST_LOCK: Mutex<()> = Mutex::new(());

struct Ctx {
    _guard: MutexGuard<'static, ()>,
    home: PathBuf,
    work: PathBuf,
}

impl Ctx {
    fn new(label: &str) -> Self {
        // Brokers are separate processes; running every e2e test in parallel
        // makes startup readiness compete for CPU and can exceed the 15s wait.
        let guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("av-e2e-{}-{}-{}", std::process::id(), n, label));
        let home = base.join("home");
        let work = base.join("work");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        Self {
            _guard: guard,
            home,
            work,
        }
    }

    fn bin(&self) -> &'static str {
        env!("CARGO_BIN_EXE_agentvault")
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(self.bin())
            .args(args)
            .current_dir(&self.work)
            .env("AGENTVAULT_HOME", &self.home)
            .env_remove("AGENTVAULT_NO_AUTOSTART")
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "command {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn write(&self, relative: &str, body: &str) -> PathBuf {
        let path = self.work.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, body).unwrap();
        path
    }
}

impl Drop for Ctx {
    fn drop(&mut self) {
        let _ = Command::new(self.bin())
            .args(["stop"])
            .current_dir(&self.work)
            .env("AGENTVAULT_HOME", &self.home)
            .output();
        if let Some(base) = self.home.parent() {
            let _ = std::fs::remove_dir_all(base);
        }
    }
}

#[test]
fn init_wraps_preserves_fields_and_is_idempotent() {
    let ctx = Ctx::new("init-json");
    let path = ctx.write(
        ".cursor/mcp.json",
        r#"{
          "mcpServers": {
            "db": {
              "command": "node",
              "args": ["server.js"],
              "env": {
                "DATABASE_URL": "postgres://u:p@h/db",
                "MAX_TOKENS": "4096",
                "REF": "${env:DATABASE_URL}"
              },
              "type": "stdio",
              "disabled": true
            },
            "remote": { "url": "https://example.com/mcp", "type": "sse" },
            "wrapped": {
              "command": "agentvault",
              "args": ["run", "--name", "wrapped", "--", "x"],
              "env": { "LEGACY_TOKEN": "must-not-be-migrated" }
            }
          }
        }"#,
    );

    let first = ctx.run(&["init", "--path", path.to_str().unwrap()]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let output = String::from_utf8_lossy(&first.stdout);
    assert!(output.contains("agent:        cursor"), "{output}");
    assert!(output.contains("wrapped 1 server(s)"), "{output}");
    let stderr = String::from_utf8_lossy(&first.stderr);
    assert!(!stderr.contains("already existed"), "{stderr}");

    let config: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let server = &config["mcpServers"]["db"];
    assert_eq!(server["type"], "stdio");
    assert_eq!(server["disabled"], true);
    assert_eq!(server["env"]["MAX_TOKENS"], "4096");
    assert_eq!(server["env"]["REF"], "${env:DATABASE_URL}");
    assert!(server["env"].get("DATABASE_URL").is_none());
    let args = server["args"].as_array().unwrap();
    assert!(args.iter().any(|value| value == "--home"));
    assert!(args.iter().any(|value| value == "--env"));
    assert!(args.iter().any(|value| value == "db"));
    assert_eq!(config["mcpServers"]["remote"]["type"], "sse");
    assert_eq!(
        config["mcpServers"]["wrapped"]["env"]["LEGACY_TOKEN"],
        "must-not-be-migrated"
    );
    let secrets = ctx.ok(&["secret", "list"]);
    assert!(secrets.contains("db/DATABASE_URL"), "{secrets}");
    assert!(!secrets.contains("wrapped/LEGACY_TOKEN"), "{secrets}");
    assert!(!secrets.contains("remote/TOKEN"), "{secrets}");

    let again = ctx.ok(&["init", "--path", path.to_str().unwrap()]);
    assert!(again.contains("already wrapped"), "{again}");
}

#[test]
fn init_dry_run_keeps_embedded_env_references() {
    let ctx = Ctx::new("init-dry-run-refs");
    let original = r#"{
      "mcpServers": {
        "db": {
          "command": "node",
          "args": ["server.js"],
          "env": {
            "DATABASE_URL": "postgres://u:p@h/db",
            "PASS_THROUGH": "${HOME}/.config",
            "LOG_LEVEL": "info",
            "MAX_TOKENS": "4096"
          }
        }
      }
    }"#;
    let path = ctx.write(".cursor/mcp.json", original);

    let dry = ctx.ok(&[
        "init",
        "--path",
        path.to_str().unwrap(),
        "--dry-run",
        "--no-env",
    ]);
    assert!(dry.contains("migrate env: DATABASE_URL"), "{dry}");
    assert!(
        dry.contains("keep env:    PASS_THROUGH (reference/override)"),
        "{dry}"
    );
    assert!(
        dry.contains("unchanged env: LOG_LEVEL, MAX_TOKENS"),
        "{dry}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    let secrets = ctx.ok(&["secret", "list"]);
    assert!(!secrets.contains("db/"), "{secrets}");
}

#[test]
fn init_refresh_updates_wrapped_launcher_and_home() {
    let ctx = Ctx::new("refresh");
    let body = format!(
        r#"{{
          "mcpServers": {{
            "db": {{
              "command": {:?},
              "args": ["--home", "C:\\old", "run", "--name", "db", "--", "node", "server.js"],
              "env": {{ "MAX_TOKENS": "4096", "LOG_LEVEL": "debug" }},
              "timeout": 30
            }}
          }}
        }}"#,
        "C:\\old\\agentvault.exe"
    );
    let path = ctx.write(".cursor/mcp.json", &body);

    let dry = ctx.ok(&[
        "init",
        "--path",
        path.to_str().unwrap(),
        "--refresh",
        "--dry-run",
    ]);
    assert!(dry.contains("would refresh 1 wrapped server"), "{dry}");
    assert!(dry.contains("unchanged env:"), "{dry}");
    assert!(dry.contains("MAX_TOKENS"), "{dry}");
    assert!(dry.contains("LOG_LEVEL"), "{dry}");

    let output = ctx.ok(&["init", "--path", path.to_str().unwrap(), "--refresh"]);
    assert!(output.contains("refreshed 1 wrapped server"), "{output}");
    let config: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let server = &config["mcpServers"]["db"];
    assert_eq!(server["command"], ctx.bin());
    let args = server["args"].as_array().unwrap();
    assert!(args.iter().any(|value| value == "--home"));
    assert!(args.iter().any(|value| value == ctx.home.to_str().unwrap()));
    assert!(args.iter().any(|value| value == "server.js"));
    assert_eq!(server["env"]["MAX_TOKENS"], "4096");
    assert_eq!(server["env"]["LOG_LEVEL"], "debug");
    assert_eq!(server["timeout"], 30);

    let again = ctx.ok(&["init", "--path", path.to_str().unwrap(), "--refresh"]);
    assert!(
        again.contains("no wrapped servers need refreshing"),
        "{again}"
    );
}

#[test]
fn tickets_respect_hard_limits_revocation_and_rotation() {
    let ctx = Ctx::new("tickets");
    ctx.ok(&["status"]);
    ctx.ok(&["secret", "add", "db", "--value", "db-value"]);

    let task: Value = serde_json::from_str(&ctx.ok(&[
        "issue",
        "--secret",
        "db",
        "--task",
        "--ttl",
        "60s",
        "--hard-ttl",
        "60s",
        "--json",
    ]))
    .unwrap();
    let parent = task["ticket"].as_str().unwrap();
    let hard_exp = task["hard_exp"].as_i64().unwrap();

    let shortened: Value =
        serde_json::from_str(&ctx.ok(&["renew", "--ticket", parent, "--ttl", "1s", "--json"]))
            .unwrap();
    assert_eq!(shortened["exp"], task["exp"]);

    let child: Value =
        serde_json::from_str(&ctx.ok(&["child", "--parent", parent, "--ttl", "7d", "--json"]))
            .unwrap();
    assert_eq!(child["capped_by_parent"], true);
    assert!(child["ttl_secs"].as_u64().unwrap() <= 60);
    assert!(child["exp"].as_i64().unwrap() <= hard_exp);

    let expiring: Value =
        serde_json::from_str(&ctx.ok(&["issue", "--secret", "db", "--ttl", "1s", "--json"]))
            .unwrap();
    std::thread::sleep(std::time::Duration::from_secs(2));
    let expired = ctx.run(&["resolve", "--ticket", expiring["ticket"].as_str().unwrap()]);
    assert!(!expired.status.success());
    assert!(String::from_utf8_lossy(&expired.stderr).contains("expired"));

    ctx.ok(&["revoke", "--secret", "db"]);
    let denied = ctx.run(&["issue", "--secret", "db", "--ttl", "30s"]);
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("revoked"));

    ctx.ok(&["rotate", "--secret", "db"]);
    let leaf: Value =
        serde_json::from_str(&ctx.ok(&["issue", "--secret", "db", "--ttl", "30s", "--json"]))
            .unwrap();
    let token = leaf["ticket"].as_str().unwrap();
    ctx.ok(&["revoke", "--ticket", token]);
    let revoked = ctx.run(&["resolve", "--ticket", token]);
    assert!(!revoked.status.success());
    assert!(String::from_utf8_lossy(&revoked.stderr).contains("revoked"));
}

#[test]
fn codex_multiline_args_rewrite_is_valid_and_idempotent() {
    let ctx = Ctx::new("codex");
    let path = ctx.write(
        "config.toml",
        r#"
[mcp_servers.multiline]
command = "npx"
args = [
  "-y",
  "@modelcontextprotocol/server-postgres",
]
env = { DATABASE_URL = "postgres://u:p@h/db", LOG_LEVEL = "info" }
startup_timeout_sec = 20
enabled = true
"#,
    );
    let output = ctx.ok(&["init", "--agent", "codex", "--path", path.to_str().unwrap()]);
    assert!(output.contains("wrapped 1 server(s)"), "{output}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("  \"-y\","), "{text}");
    assert!(!text.contains("DATABASE_URL ="), "{text}");
    assert!(text.contains("LOG_LEVEL = \"info\""));
    assert!(text.contains("startup_timeout_sec = 20"));
    assert!(text.contains("enabled = true"));

    let again = ctx.ok(&["init", "--agent", "codex", "--path", path.to_str().unwrap()]);
    assert!(
        again.contains("no `[mcp_servers.*]` entries to wrap"),
        "{again}"
    );
}

#[test]
fn autostart_can_be_disabled() {
    let ctx = Ctx::new("no-autostart");
    let output = ctx.run(&["--no-autostart", "status"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("autostart is disabled"));
}

#[test]
fn qoder_project_config_is_detected() {
    let ctx = Ctx::new("qoder");
    ctx.write(
        ".qoder/settings.json",
        r#"{
          "theme": "dark",
          "mcp": { "enableAllProjectMcpServers": true },
          "mcpServers": {
            "files": { "command": "node", "args": ["server.js"], "cwd": "./tools" }
          }
        }"#,
    );
    let output = ctx.ok(&["init", "--dry-run"]);
    assert!(output.contains("agent:        qoder"), "{output}");
    assert!(output.contains("would wrap 1 server(s)"), "{output}");
}

#[test]
fn env_show_and_install_dry_run_do_not_touch_the_system() {
    let ctx = Ctx::new("env");
    let shown = ctx.ok(&["env", "show"]);
    assert!(shown.contains("home from:  AGENTVAULT_HOME"), "{shown}");
    let dry = ctx.ok(&["env", "install", "--dry-run"]);
    assert!(dry.contains("would set AGENTVAULT_HOME"), "{dry}");
    assert!(dry.contains("would add"), "{dry}");
}

#[test]
fn doctor_reports_sources_and_agent_configs() {
    let ctx = Ctx::new("doctor");
    ctx.write(
        ".cursor/mcp.json",
        r#"{"mcpServers":{"x":{"command":"node","args":[]}}}"#,
    );
    let output = ctx.ok(&["doctor"]);
    assert!(output.contains("home from: AGENTVAULT_HOME"), "{output}");
    assert!(output.contains("[x] cursor"), "{output}");
    assert!(output.contains("cursor"), "{output}");
}

#[test]
fn codex_invalid_toml_is_rejected_without_writing() {
    let ctx = Ctx::new("codex-invalid");
    let original = "[mcp_servers.db\ncommand = \"npx\"\n";
    let path = ctx.write("config.toml", original);
    let output = ctx.run(&["init", "--agent", "codex", "--path", path.to_str().unwrap()]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid Codex TOML"), "{stderr}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn codex_init_preserves_bom_crlf_comments_and_escapes() {
    let ctx = Ctx::new("codex-format");
    let original = "\u{feff}# keep comment\r\nmodel = \"gpt-5\"\r\n\r\n[mcp_servers.db]\r\ncommand = \"npx\" # inline comment\r\nargs = [\r\n  \"-y\",\r\n  \"@modelcontextprotocol/server-postgres\",\r\n]\r\nenv = { DATABASE_URL = 'postgres://demo:demo@localhost/demo', LOG_LEVEL = \"info\" }\r\n";
    let path = ctx.write("config.toml", original);
    let output = ctx.ok(&["init", "--agent", "codex", "--path", path.to_str().unwrap()]);
    assert!(output.contains("wrapped 1 server(s)"), "{output}");

    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "BOM was dropped");
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("# keep comment"), "{text}");
    assert!(text.contains("model = \"gpt-5\""), "{text}");
    assert!(text.contains("LOG_LEVEL"), "{text}");
    assert!(
        !text.contains("postgres://demo:demo@localhost/demo"),
        "{text}"
    );
    assert!(!text.contains("DATABASE_URL = "), "{text}");
    let without_crlf = text.replace("\r\n", "");
    assert!(!without_crlf.contains('\n'), "mixed newlines: {text:?}");

    let backup = std::fs::read_dir(&ctx.work)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.contains(".bak."))
                .unwrap_or(false)
        })
        .expect("backup file missing");
    let backup_bytes = std::fs::read(&backup).unwrap();
    assert!(
        backup_bytes.starts_with(b"AVBAK1"),
        "backup is not encrypted"
    );
    assert!(
        !String::from_utf8_lossy(&backup_bytes).contains("postgres://demo:demo@localhost/demo"),
        "backup leaked plaintext secret"
    );
}
