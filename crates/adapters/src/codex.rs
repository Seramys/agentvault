//! Codex `config.toml` adapter backed by the full TOML parser in `toml_edit`.
//!
//! Codex stores MCP servers as TOML tables:
//!
//! ```toml
//! [mcp_servers.postgres]
//! command = "npx"
//! args = ["-y", "@modelcontextprotocol/server-postgres"]
//! env = { DATABASE_URL = "postgres://..." }
//! # or a sub-table:
//! [mcp_servers.postgres.env]
//! DATABASE_URL = "postgres://..."
//! ```
//!
//! Only the `mcp_servers.*` keys are interpreted. `toml_edit` preserves
//! comments, unknown settings, key order and formatting for everything else.

use agentvault_core::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use toml_edit::{Array, DocumentMut, Item, TableLike, Value};

#[derive(Clone, Debug, Default)]
pub struct CodexServer {
    pub name: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// Semantic limitations that prevent safe wrapping (for example a
    /// non-string `args` entry). Such servers are reported and left untouched.
    pub unsupported: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CodexChange {
    pub server: String,
    pub before: String,
    pub migrated_env: Vec<(String, String)>,
    pub remaining_env: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CodexConfig {
    doc: DocumentMut,
    pub servers: Vec<CodexServer>,
    /// File-level traits preserved from the source so a rewrite stays
    /// byte-faithful for editors and Windows tooling.
    bom: bool,
    crlf: bool,
    trailing_newline: bool,
}

impl CodexConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::parse(text)
    }

    pub fn parse(text: String) -> Result<Self> {
        let bom = text.starts_with('\u{feff}');
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let crlf = text.contains("\r\n");
        let trailing_newline = text.ends_with('\n') || text.ends_with('\r');
        let doc: DocumentMut = text
            .parse()
            .map_err(|error| Error::Other(format!("invalid Codex TOML: {error}")))?;
        let servers = Self::read_servers(&doc);
        Ok(Self {
            doc,
            servers,
            bom,
            crlf,
            trailing_newline,
        })
    }

    pub fn has_bom(&self) -> bool {
        self.bom
    }

    pub fn uses_crlf(&self) -> bool {
        self.crlf
    }

    fn read_servers(doc: &DocumentMut) -> Vec<CodexServer> {
        let mut servers = Vec::new();
        let Some(root) = doc.get("mcp_servers").and_then(Item::as_table_like) else {
            return servers;
        };
        for (name, item) in root.iter() {
            let Some(table) = item.as_table_like() else {
                servers.push(CodexServer {
                    name: name.to_string(),
                    unsupported: Some("mcp server is not a table".into()),
                    ..Default::default()
                });
                continue;
            };

            let mut unsupported = None;
            let command = match table.get("command") {
                None => None,
                Some(item) => match item.as_str() {
                    Some(value) => Some(value.to_string()),
                    None => {
                        unsupported = Some("command is not a string".into());
                        None
                    }
                },
            };

            let args = match table.get("args") {
                None => Vec::new(),
                Some(item) => match item.as_array() {
                    None => {
                        unsupported = Some("args is not an array".into());
                        Vec::new()
                    }
                    Some(array) => {
                        let mut args = Vec::new();
                        for value in array.iter() {
                            if let Some(value) = value.as_str() {
                                args.push(value.to_string());
                            } else {
                                unsupported = Some("args contains a non-string".into());
                            }
                        }
                        args
                    }
                },
            };

            let env = read_env(table, &mut unsupported);
            servers.push(CodexServer {
                name: name.to_string(),
                command,
                args,
                env,
                unsupported,
            });
        }
        servers
    }

    pub fn is_wrapped(server: &CodexServer) -> bool {
        if server.unsupported.is_some() {
            return false;
        }
        let Some(command) = server.command.as_deref() else {
            return false;
        };
        crate::is_agentvault_command(command) && crate::wrapped_run_index(&server.args).is_some()
    }

    pub fn plan(&self) -> Vec<CodexChange> {
        self.plan_with(&crate::Policy::default())
    }

    pub fn plan_with(&self, policy: &crate::Policy) -> Vec<CodexChange> {
        self.servers
            .iter()
            .filter(|server| {
                server.unsupported.is_none()
                    && server.command.is_some()
                    && !Self::is_wrapped(server)
            })
            .map(|server| change_for(server, policy))
            .collect()
    }

    pub fn apply(&mut self) -> Vec<CodexChange> {
        self.apply_with_launcher("agentvault", None)
    }

    /// Wrap commands with an explicit launcher path so the install directory
    /// does not have to be on PATH.
    pub fn apply_with_launcher(
        &mut self,
        launcher: &str,
        home: Option<&std::path::Path>,
    ) -> Vec<CodexChange> {
        self.apply_with_launcher_and(launcher, home, &crate::Policy::default())
    }

    pub fn apply_with_launcher_and(
        &mut self,
        launcher: &str,
        home: Option<&std::path::Path>,
        policy: &crate::Policy,
    ) -> Vec<CodexChange> {
        let mut changes = Vec::new();
        let Some(root) = self
            .doc
            .get_mut("mcp_servers")
            .and_then(Item::as_table_like_mut)
        else {
            return changes;
        };

        for server in self.servers.clone() {
            if server.unsupported.is_some() || server.command.is_none() || Self::is_wrapped(&server)
            {
                continue;
            }

            let migrated = secret_env(&server, policy);
            let migrated_keys: BTreeSet<String> =
                migrated.iter().map(|(key, _)| key.clone()).collect();
            let mut args = Vec::new();
            if let Some(home) = home {
                args.push("--home".to_string());
                args.push(home.display().to_string());
            }
            args.push("run".to_string());
            args.push("--name".to_string());
            args.push(server.name.clone());
            for (key, _) in &migrated {
                args.push("--env".to_string());
                args.push(format!("{key}=secret:{}/{key}", server.name));
            }
            args.push("--".to_string());
            args.push(server.command.clone().unwrap_or_default());
            args.extend(server.args.clone());

            let Some(item) = root.get_mut(&server.name) else {
                continue;
            };
            let Some(table) = item.as_table_like_mut() else {
                continue;
            };
            write_wrapped_command(table, launcher, &args);
            if !migrated_keys.is_empty() {
                remove_env_keys(table, &migrated_keys);
            }

            changes.push(CodexChange {
                server: server.name.clone(),
                before: before_command(&server),
                remaining_env: server
                    .env
                    .keys()
                    .filter(|key| !migrated_keys.contains(*key))
                    .cloned()
                    .collect(),
                migrated_env: migrated,
            });
        }

        if !changes.is_empty() {
            self.servers = Self::read_servers(&self.doc);
        }
        changes
    }

    /// Update the launcher and `--home` of already wrapped servers while
    /// leaving their env, timeout and wrapped command untouched.
    pub fn refresh_launcher(&mut self, launcher: &str, home: Option<&Path>) -> Vec<CodexChange> {
        let mut changes = Vec::new();
        let Some(root) = self
            .doc
            .get_mut("mcp_servers")
            .and_then(Item::as_table_like_mut)
        else {
            return changes;
        };

        for server in self.servers.clone() {
            if !Self::is_wrapped(&server) {
                continue;
            }
            let Some(run_index) = crate::wrapped_run_index(&server.args) else {
                continue;
            };
            let old_prefix = &server.args[..run_index];
            let new_home = home.map(|path| path.display().to_string());
            if server.command.as_deref() == Some(launcher)
                && crate::wrapped_home(old_prefix) == new_home
            {
                continue;
            }

            let mut args = Vec::new();
            let mut index = 0;
            while index < old_prefix.len() {
                match old_prefix[index].as_str() {
                    "--home" => index += 2,
                    value if value.starts_with("--home=") => index += 1,
                    value => {
                        args.push(value.to_string());
                        index += 1;
                    }
                }
            }
            if let Some(home) = home {
                args.push("--home".to_string());
                args.push(home.display().to_string());
            }
            args.extend_from_slice(&server.args[run_index..]);

            let Some(item) = root.get_mut(&server.name) else {
                continue;
            };
            let Some(table) = item.as_table_like_mut() else {
                continue;
            };
            write_wrapped_command(table, launcher, &args);
            changes.push(CodexChange {
                server: server.name.clone(),
                before: before_command(&server),
                migrated_env: Vec::new(),
                remaining_env: server.env.keys().cloned().collect(),
            });
        }

        if !changes.is_empty() {
            self.servers = Self::read_servers(&self.doc);
        }
        changes
    }

    pub fn render(&self) -> String {
        self.doc.to_string()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let original = std::fs::read(path).ok();
        let mut text = self.doc.to_string();
        if self.trailing_newline && !text.ends_with('\n') {
            text.push('\n');
        }
        if self.crlf {
            // TOML normalizes CRLF to LF when parsing, so converting every
            // newline (including multi-line strings) is semantically safe.
            text = text.replace("\r\n", "\n").replace('\n', "\r\n");
        }
        // Fail closed: never write a document that cannot be parsed again.
        Self::parse(text.clone())?;
        let mut data = Vec::with_capacity(text.len() + 3);
        if self.bom {
            data.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        data.extend_from_slice(text.as_bytes());

        if let Err(error) = agentvault_core::util::atomic_write(path, &data) {
            restore(path, original.as_deref());
            return Err(error);
        }
        match Self::load(path) {
            Ok(_) => Ok(()),
            Err(error) => {
                restore(path, original.as_deref());
                Err(Error::Other(format!(
                    "Codex config failed post-write validation and was restored: {error}"
                )))
            }
        }
    }
}

/// Put the exact previous bytes back, or remove a file this write created.
fn restore(path: &Path, original: Option<&[u8]>) {
    match original {
        Some(bytes) => {
            let _ = agentvault_core::util::atomic_write(path, bytes);
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn read_env(server: &dyn TableLike, unsupported: &mut Option<String>) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    let Some(item) = server.get("env") else {
        return env;
    };
    let Some(table) = item.as_table_like() else {
        *unsupported = Some("env is not a table".into());
        return env;
    };
    for (key, value) in table.iter() {
        match value.as_str() {
            Some(value) => {
                env.insert(key.to_string(), value.to_string());
            }
            None => {
                *unsupported = Some("env contains a non-string value".into());
            }
        }
    }
    env
}

fn write_wrapped_command(server: &mut dyn TableLike, launcher: &str, args: &[String]) {
    server.insert("command", Item::Value(Value::from(launcher)));
    let mut array = Array::new();
    for arg in args {
        array.push(Value::from(arg.as_str()));
    }
    server.insert("args", Item::Value(Value::Array(array)));
}

fn remove_env_keys(server: &mut dyn TableLike, keys: &BTreeSet<String>) {
    let Some(item) = server.get_mut("env") else {
        return;
    };
    let Some(table) = item.as_table_like_mut() else {
        return;
    };
    for key in keys {
        table.remove(key);
    }
}

fn change_for(server: &CodexServer, policy: &crate::Policy) -> CodexChange {
    let migrated = secret_env(server, policy);
    let migrated_keys: BTreeSet<String> = migrated.iter().map(|(key, _)| key.clone()).collect();
    CodexChange {
        server: server.name.clone(),
        before: before_command(server),
        remaining_env: server
            .env
            .keys()
            .filter(|key| !migrated_keys.contains(*key))
            .cloned()
            .collect(),
        migrated_env: migrated,
    }
}

fn before_command(server: &CodexServer) -> String {
    format!(
        "{} {}",
        server.command.clone().unwrap_or_default(),
        server.args.join(" ")
    )
    .trim()
    .to_string()
}

fn secret_env(server: &CodexServer, policy: &crate::Policy) -> Vec<(String, String)> {
    server
        .env
        .iter()
        .filter(|(key, value)| crate::should_migrate_env(key, value, policy))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
model = "gpt-5"

[mcp_servers.postgres]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-postgres"]
env = { DATABASE_URL = "postgres://u:p@h/db", LOG_LEVEL = "info" }
startup_timeout_sec = 20

[mcp_servers.remote]
url = "https://example.com/mcp"
"#;

    fn parse(text: &str) -> CodexConfig {
        CodexConfig::parse(text.to_string()).unwrap()
    }

    #[test]
    fn parses_servers_and_env() {
        let cfg = parse(SAMPLE);
        assert_eq!(cfg.servers.len(), 2);
        let pg = cfg
            .servers
            .iter()
            .find(|server| server.name == "postgres")
            .unwrap();
        assert_eq!(pg.command.as_deref(), Some("npx"));
        assert_eq!(pg.args.len(), 2);
        assert_eq!(
            pg.env.get("DATABASE_URL").map(String::as_str),
            Some("postgres://u:p@h/db")
        );
    }

    #[test]
    fn rewrites_and_strips_secret_env() {
        let mut cfg = parse(SAMPLE);
        let changes = cfg.apply();
        assert_eq!(changes.len(), 1);
        let text = cfg.render();
        assert!(text.contains("command = \"agentvault\""));
        assert!(text.contains("\"run\""));
        assert!(text.contains("--env"));
        assert!(text.contains("secret:postgres/DATABASE_URL"));
        assert!(!text.contains("DATABASE_URL ="));
        assert!(text.contains("LOG_LEVEL"));
        assert!(text.contains("startup_timeout_sec = 20"));
        assert!(text.contains("[mcp_servers.remote]"));
        assert!(changes[0].remaining_env.contains(&"LOG_LEVEL".into()));
        assert!(!changes[0].remaining_env.contains(&"DATABASE_URL".into()));
    }

    #[test]
    fn refresh_updates_wrapped_launcher_and_home() {
        let mut cfg = parse(SAMPLE);
        cfg.apply_with_launcher("/old/agentvault", Some(std::path::Path::new("/old/home")));
        let mut cfg = parse(&cfg.render());
        let changes =
            cfg.refresh_launcher("/new/agentvault", Some(std::path::Path::new("/new/home")));
        assert_eq!(changes.len(), 1);
        let reparsed = parse(&cfg.render());
        let server = reparsed
            .servers
            .iter()
            .find(|server| server.name == "postgres")
            .unwrap();
        assert_eq!(server.command.as_deref(), Some("/new/agentvault"));
        assert_eq!(
            crate::wrapped_home(&server.args).as_deref(),
            Some("/new/home")
        );
        assert!(server.args.contains(&"npx".to_string()));
        assert!(server.env.contains_key("LOG_LEVEL"));
        assert!(CodexConfig::is_wrapped(server));
        assert!(cfg
            .refresh_launcher("/new/agentvault", Some(std::path::Path::new("/new/home")))
            .is_empty());
    }

    #[test]
    fn apply_is_idempotent() {
        let mut cfg = parse(SAMPLE);
        cfg.apply();
        let mut reparsed = parse(&cfg.render());
        assert!(reparsed.apply().is_empty());
    }

    #[test]
    fn supports_env_sub_table() {
        let text = r#"
[mcp_servers.db]
command = "db-mcp"

[mcp_servers.db.env]
DB_PASSWORD = "hunter2"
LOG_LEVEL = "debug"
"#;
        let mut cfg = parse(text);
        let changes = cfg.apply();
        assert_eq!(changes.len(), 1);
        let rendered = cfg.render();
        assert!(!rendered.contains("DB_PASSWORD ="));
        assert!(rendered.contains("LOG_LEVEL"));
    }

    #[test]
    fn parses_and_rewrites_multiline_args() {
        let text = r#"
[mcp_servers.db]
command = "npx"
args = [
  "-y",
  "@modelcontextprotocol/server-postgres",
]
env = { DATABASE_URL = "postgres://u:p@h/db", LOG_LEVEL = "info" }
startup_timeout_sec = 20
"#;
        let mut cfg = parse(text);
        assert_eq!(cfg.servers[0].args.len(), 2);
        cfg.apply_with_launcher("agentvault.exe", Some(std::path::Path::new(r"C:\av home")));
        let rendered = cfg.render();
        assert!(rendered.contains("--home"));
        assert!(rendered.contains("run"));
        assert!(!rendered.contains("@modelcontextprotocol/server-postgres\",\n]"));
        assert!(!rendered.contains("  \"-y\","));
        assert!(rendered.contains("startup_timeout_sec = 20"));
        let mut reparsed = parse(&rendered);
        assert!(CodexConfig::is_wrapped(&reparsed.servers[0]));
        assert!(reparsed.apply().is_empty());
    }

    #[test]
    fn parses_single_quoted_args_with_commas() {
        let text = r#"
[mcp_servers.probe]
command = 'node'
args = ['-e', 'setTimeout(()=>{},60000)']
"#;
        let cfg = parse(text);
        assert_eq!(cfg.servers[0].args.len(), 2);
        assert_eq!(cfg.servers[0].args[0], "-e");
        assert_eq!(cfg.servers[0].args[1], "setTimeout(()=>{},60000)");
    }

    #[test]
    fn handles_crlf_and_bom() {
        let text = "\u{feff}[mcp_servers.db]\r\ncommand = 'node'\r\nargs = ['-e', 'a,b']\r\n\r\n[mcp_servers.db.env]\r\nAPI_KEY = 'sk-test-crlf'\r\nKEEP = 'yes'\r\n";
        let mut cfg = parse(text);
        assert_eq!(cfg.servers[0].args[1], "a,b");
        cfg.apply();
        let rendered = cfg.render();
        assert!(!rendered.contains("sk-test-crlf"));
        assert!(rendered.contains("KEEP"));
    }

    #[test]
    fn handles_dotted_keys_and_quoted_names() {
        let text = r#"
mcp_servers.db.command = "node"
mcp_servers.db.args = ["server.js"]
mcp_servers.db.env.API_KEY = "sk-test-dotted"
mcp_servers.db.env.KEEP = "yes"

[mcp_servers."a.b"]
command = "node"
args = ["other.js"]
"#;
        let mut cfg = parse(text);
        assert_eq!(cfg.servers.len(), 2);
        let changes = cfg.apply_with_launcher("C:\\Tools\\agentvault.exe", None);
        assert_eq!(changes.len(), 2);
        let rendered = cfg.render();
        assert!(!rendered.contains("sk-test-dotted"));
        assert!(rendered.contains("KEEP"));
        assert!(rendered.contains("a.b"));
        assert!(cfg.servers.iter().all(CodexConfig::is_wrapped));
    }

    #[test]
    fn preserves_comments_unknown_fields_and_inline_env() {
        let text = r#"
# top comment: keep me
model = "gpt-5"

[mcp_servers.db]
# command comment
command = "node"
args = ["server.js"] # args comment
startup_timeout_sec = 20
custom = "keep-me"
env = { TOKEN = "sk-test-comment", LOG_LEVEL = "info" }
"#;
        let mut cfg = parse(text);
        cfg.apply();
        let rendered = cfg.render();
        assert!(rendered.contains("# top comment: keep me"));
        assert!(rendered.contains("startup_timeout_sec = 20"));
        assert!(rendered.contains("custom = \"keep-me\""));
        assert!(rendered.contains("LOG_LEVEL"));
        assert!(!rendered.contains("sk-test-comment"));
    }

    #[test]
    fn nested_tables_do_not_leak_into_the_server() {
        let text = r#"
[mcp_servers.db]
command = "db-mcp"

[mcp_servers.db.timeout]
command = "evil"
value = 5
"#;
        let mut cfg = parse(text);
        assert_eq!(cfg.servers.len(), 1);
        assert_eq!(cfg.servers[0].command.as_deref(), Some("db-mcp"));
        cfg.apply();
        let rendered = cfg.render();
        assert!(rendered.contains("[mcp_servers.db.timeout]"));
        assert!(rendered.contains("command = \"evil\""));
    }

    #[test]
    fn rejects_inline_env_plus_subtable() {
        let text = r#"
[mcp_servers.db]
command = "db-mcp"
env = { DB_PASSWORD = "hunter2", LOG_LEVEL = "info" }

[mcp_servers.db.env]
EXTRA_FLAG = "1"
"#;
        assert!(CodexConfig::parse(text.to_string()).is_err());
    }

    #[test]
    fn rejects_invalid_toml() {
        assert!(CodexConfig::parse("[mcp_servers.db\ncommand = \"x\"\n".to_string()).is_err());
    }

    #[test]
    fn refresh_preserves_non_home_prefix_flags() {
        let text = r#"
[mcp_servers.db]
command = "C:\\old\\agentvault.exe"
args = ["--no-autostart", "--home", "C:\\old", "run", "--name", "db", "--", "node", "server.js"]
"#;
        let mut cfg = parse(text);
        let changes = cfg.refresh_launcher(
            "C:\\new\\agentvault.exe",
            Some(std::path::Path::new("C:\\new")),
        );
        assert_eq!(changes.len(), 1);
        let rendered = cfg.render();
        assert!(rendered.contains("--no-autostart"));
        assert!(!rendered.contains("C:\\old"));
        assert!(rendered.contains("C:\\new"));
        assert!(rendered.contains("server.js"));
    }

    #[test]
    fn reports_unsupported_args_without_wrapping() {
        let text = r#"
[mcp_servers.db]
command = "node"
args = ["server.js", 42]
env = { API_KEY = "sk-test-unsupported" }
"#;
        let cfg = parse(text);
        assert!(cfg.servers[0].unsupported.is_some());
        assert!(cfg.plan().is_empty());
        assert!(!cfg.render().contains("agentvault"));
    }

    #[test]
    fn decodes_escaped_and_literal_strings() {
        let text = r#"
[mcp_servers.probe]
command = "node"
args = ["-e", "console.log(\"hi\")", 'C:\tmp\dir', "line1\nline2", "tab\tend", "quote'inside"]
env = { API_KEY = "sk-test-escaped", PATH_HINT = 'C:\tools\bin', QUOTED = "a\"b", MULTILINE = """first
second""" }
"#;
        let mut cfg = parse(text);
        let server = &cfg.servers[0];
        assert_eq!(server.args[1], "console.log(\"hi\")");
        assert_eq!(server.args[2], r"C:\tmp\dir");
        assert_eq!(server.args[3], "line1\nline2");
        assert_eq!(server.args[4], "tab\tend");
        assert_eq!(server.args[5], "quote'inside");
        assert_eq!(
            server.env.get("PATH_HINT").map(String::as_str),
            Some(r"C:\tools\bin")
        );
        assert_eq!(
            server.env.get("MULTILINE").map(String::as_str),
            Some("first\nsecond")
        );
        assert_eq!(server.env.get("QUOTED").map(String::as_str), Some("a\"b"));

        let changes = cfg.apply();
        assert_eq!(changes.len(), 1);
        let rendered = cfg.render();
        assert!(!rendered.contains("sk-test-escaped"), "{rendered}");
        assert!(rendered.contains("PATH_HINT"), "{rendered}");
        assert!(
            rendered.contains("LOG_LEVEL") || !rendered.contains("API_KEY ="),
            "{rendered}"
        );

        // Round-trip: escaped/literal values must survive the rewrite.
        let reparsed = parse(&rendered);
        let wrapped = &reparsed.servers[0];
        assert!(CodexConfig::is_wrapped(wrapped));
        assert!(wrapped.args.iter().any(|arg| arg == r"C:\tmp\dir"));
        assert!(wrapped.args.iter().any(|arg| arg == "line1\nline2"));
        assert!(wrapped.args.iter().any(|arg| arg == "quote'inside"));
    }
}
