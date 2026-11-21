//! Minimal Codex `config.toml` adapter.
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
//! Only the `[mcp_servers.*]` subset is understood. Every other line is
//! preserved so unrelated Codex settings and comments survive a rewrite.

use agentvault_core::Result;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct CodexServer {
    pub name: String,
    pub header_line: usize,
    pub end_line: usize,
    pub command: Option<String>,
    pub command_line: Option<usize>,
    pub args: Vec<String>,
    pub args_line: Option<usize>,
    pub args_end_line: Option<usize>,
    pub env: BTreeMap<String, String>,
    pub env_line: Option<usize>,
    pub env_header_line: Option<usize>,
    pub env_table_lines: Vec<(String, usize)>,
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
    pub lines: Vec<String>,
    pub servers: Vec<CodexServer>,
}

impl CodexConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text).to_string();
        Ok(Self::parse(text))
    }

    pub fn parse(text: String) -> Self {
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        let mut servers: Vec<CodexServer> = Vec::new();
        let mut current: Option<usize> = None;
        let mut in_env_table = false;
        // Multi-line arrays are common in real Codex configs:
        //   args = [
        //     "-y",
        //     "@scope/server",
        //   ]
        // Accumulate them into one logical value and remember the line range so
        // the rewrite can replace the whole array instead of leaving stray lines.
        let mut pending_args: Option<(usize, usize, String)> = None;

        for (idx, raw) in lines.iter().enumerate() {
            let line = raw.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }

            if let Some((start, server_index, mut accumulated)) = pending_args.take() {
                accumulated.push(' ');
                accumulated.push_str(strip_inline_comment(line).trim());
                if array_is_closed(&accumulated) {
                    let server = &mut servers[server_index];
                    server.args = parse_array(&accumulated);
                    server.args_line = Some(start);
                    server.args_end_line = Some(idx);
                } else {
                    pending_args = Some((start, server_index, accumulated));
                }
                continue;
            }

            if let Some(rest) = section_body(line, "mcp_servers") {
                let Some((server_name, is_env)) = parse_section(&rest) else {
                    // A nested table such as `[mcp_servers.db.timeout]` is not
                    // a server definition; stop attributing its keys to the
                    // current server.
                    if let Some(prev) = current.take() {
                        servers[prev].end_line = idx;
                    }
                    in_env_table = false;
                    continue;
                };
                if is_env {
                    // A `[mcp_servers.NAME.env]` table belongs to the server.
                    let target = current
                        .filter(|ci| servers[*ci].name == server_name)
                        .or_else(|| servers.iter().rposition(|s| s.name == server_name));
                    match target {
                        Some(ti) => {
                            servers[ti].env_header_line = Some(idx);
                            servers[ti].end_line = lines.len();
                            current = Some(ti);
                        }
                        None => {
                            if let Some(prev) = current.take() {
                                servers[prev].end_line = idx;
                            }
                            servers.push(CodexServer {
                                name: server_name,
                                header_line: idx,
                                end_line: lines.len(),
                                env_header_line: Some(idx),
                                ..Default::default()
                            });
                            current = Some(servers.len() - 1);
                        }
                    }
                    in_env_table = true;
                    continue;
                }

                if let Some(prev) = current.take() {
                    servers[prev].end_line = idx;
                }
                servers.push(CodexServer {
                    name: server_name,
                    header_line: idx,
                    end_line: lines.len(),
                    ..Default::default()
                });
                current = Some(servers.len() - 1);
                in_env_table = false;
                continue;
            }

            if line.starts_with('[') {
                if let Some(prev) = current.take() {
                    servers[prev].end_line = idx;
                }
                in_env_table = false;
                continue;
            }

            let Some(ci) = current else {
                continue;
            };
            let server = &mut servers[ci];

            if in_env_table {
                if let Some((key, value)) = parse_key_value(line) {
                    server.env.insert(key.clone(), value);
                    server.env_table_lines.push((key, idx));
                }
                continue;
            }

            if let Some((key, value)) = parse_key_value(line) {
                match key.as_str() {
                    "command" => {
                        server.command = Some(unquote(&value));
                        server.command_line = Some(idx);
                    }
                    "args" => {
                        if value.trim_start().starts_with('[') && !array_is_closed(&value) {
                            server.args_line = Some(idx);
                            server.args_end_line = Some(idx);
                            pending_args = Some((idx, ci, value));
                        } else {
                            server.args = parse_array(&value);
                            server.args_line = Some(idx);
                            server.args_end_line = Some(idx);
                        }
                    }
                    "env" => {
                        server.env = parse_inline_table(&value);
                        server.env_line = Some(idx);
                    }
                    _ => {}
                }
            }
        }

        if let Some((start, server_index, accumulated)) = pending_args {
            let server = &mut servers[server_index];
            server.args = parse_array(&accumulated);
            server.args_line = Some(start);
            server.args_end_line = Some(lines.len().saturating_sub(1));
        }

        Self { lines, servers }
    }

    pub fn is_wrapped(server: &CodexServer) -> bool {
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
            .filter(|s| s.command.is_some() && !Self::is_wrapped(s))
            .map(|s| {
                let migrated = secret_env(s, policy);
                let migrated_keys: BTreeSet<String> =
                    migrated.iter().map(|(key, _)| key.clone()).collect();
                CodexChange {
                    server: s.name.clone(),
                    before: format!(
                        "{} {}",
                        s.command.clone().unwrap_or_default(),
                        s.args.join(" ")
                    )
                    .trim()
                    .to_string(),
                    remaining_env: s
                        .env
                        .keys()
                        .filter(|key| !migrated_keys.contains(*key))
                        .cloned()
                        .collect(),
                    migrated_env: migrated,
                }
            })
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
        let mut edits: Vec<(usize, usize, Vec<String>)> = Vec::new();

        for server in &self.servers {
            if server.command.is_none() || Self::is_wrapped(server) {
                continue;
            }
            let before = format!(
                "{} {}",
                server.command.clone().unwrap_or_default(),
                server.args.join(" ")
            )
            .trim()
            .to_string();
            let migrated = secret_env(server, policy);
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

            let mut rendered = vec![self.lines[server.header_line].clone()];
            rendered.push(format!("command = {}", toml_quote(launcher)));
            rendered.push(format!(
                "args = [{}]",
                args.iter()
                    .map(|a| toml_quote(a))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));

            let env_table_keys: BTreeSet<String> = server
                .env_table_lines
                .iter()
                .map(|(key, _)| key.clone())
                .collect();
            let kept_inline: BTreeMap<&String, &String> = server
                .env
                .iter()
                .filter(|(key, _)| !migrated_keys.contains(*key) && !env_table_keys.contains(*key))
                .collect();
            if server.env_header_line.is_none() && !kept_inline.is_empty() {
                rendered.push(format!(
                    "env = {{ {} }}",
                    kept_inline
                        .iter()
                        .map(|(k, v)| format!("{} = {}", toml_key(k), toml_quote(v)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }

            let env_line_idx: BTreeSet<usize> =
                server.env_table_lines.iter().map(|(_, i)| *i).collect();
            for idx in (server.header_line + 1)..server.end_line {
                let in_args = server
                    .args_line
                    .map(|start| idx >= start && idx <= server.args_end_line.unwrap_or(start))
                    .unwrap_or(false);
                if Some(idx) == server.command_line || in_args || Some(idx) == server.env_line {
                    continue;
                }
                if Some(idx) == server.env_header_line {
                    rendered.push(self.lines[idx].clone());
                    // Inline env values that are not migrated must survive even
                    // when the server also uses an env sub-table.
                    for (key, value) in &kept_inline {
                        rendered.push(format!("{} = {}", toml_key(key), toml_quote(value)));
                    }
                    continue;
                }
                if env_line_idx.contains(&idx) {
                    let key = server
                        .env_table_lines
                        .iter()
                        .find(|(_, i)| *i == idx)
                        .map(|(k, _)| k.clone())
                        .unwrap_or_default();
                    if migrated_keys.contains(&key) {
                        continue;
                    }
                    rendered.push(self.lines[idx].clone());
                    continue;
                }
                rendered.push(self.lines[idx].clone());
            }

            edits.push((server.header_line, server.end_line, rendered));
            changes.push(CodexChange {
                server: server.name.clone(),
                before,
                remaining_env: server
                    .env
                    .keys()
                    .filter(|key| !migrated_keys.contains(*key))
                    .cloned()
                    .collect(),
                migrated_env: migrated,
            });
        }

        // Apply from the bottom so earlier line numbers stay valid.
        edits.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
        for (start, end, replacement) in edits {
            self.lines.splice(start..end, replacement);
        }
        changes
    }

    /// Update the launcher and `--home` of already wrapped servers while
    /// leaving their env, timeout and wrapped command untouched.
    pub fn refresh_launcher(&mut self, launcher: &str, home: Option<&Path>) -> Vec<CodexChange> {
        let mut changes = Vec::new();
        let mut edits: Vec<(usize, usize, Vec<String>)> = Vec::new();
        for server in &self.servers {
            if !Self::is_wrapped(server) {
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
            let (Some(command_line), Some(args_start)) = (server.command_line, server.args_line)
            else {
                continue;
            };
            let args_end = server.args_end_line.unwrap_or(args_start);
            let before = format!(
                "{} {}",
                server.command.clone().unwrap_or_default(),
                server.args.join(" ")
            )
            .trim()
            .to_string();
            let mut args = Vec::new();
            let mut index = 0;
            while index < old_prefix.len() {
                match old_prefix[index].as_str() {
                    "--home" => index += 2,
                    value if value.starts_with("--home=") => index += 1,
                    "--no-autostart" => {
                        args.push("--no-autostart".to_string());
                        index += 1;
                    }
                    _ => index += 1,
                }
            }
            if let Some(home) = home {
                args.push("--home".to_string());
                args.push(home.display().to_string());
            }
            args.extend_from_slice(&server.args[run_index..]);

            let mut rendered = vec![self.lines[server.header_line].clone()];
            rendered.push(format!("command = {}", toml_quote(launcher)));
            rendered.push(format!(
                "args = [{}]",
                args.iter()
                    .map(|arg| toml_quote(arg))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            for idx in (server.header_line + 1)..server.end_line {
                if idx == command_line || (idx >= args_start && idx <= args_end) {
                    continue;
                }
                rendered.push(self.lines[idx].clone());
            }
            edits.push((server.header_line, server.end_line, rendered));
            changes.push(CodexChange {
                server: server.name.clone(),
                before,
                migrated_env: Vec::new(),
                remaining_env: server.env.keys().cloned().collect(),
            });
        }

        edits.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
        for (start, end, replacement) in edits {
            self.lines.splice(start..end, replacement);
        }
        if !changes.is_empty() {
            let reparsed = Self::parse(self.lines.join("\n"));
            self.lines = reparsed.lines;
            self.servers = reparsed.servers;
        }
        changes
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let mut text = self.lines.join("\n");
        text.push('\n');
        agentvault_core::util::atomic_write(path, text.as_bytes())
    }
}

fn secret_env(server: &CodexServer, policy: &crate::Policy) -> Vec<(String, String)> {
    server
        .env
        .iter()
        .filter(|(key, value)| crate::should_migrate_env(key, value, policy))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn section_body<'a>(line: &'a str, prefix: &str) -> Option<String> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    let rest = inner.strip_prefix(prefix)?.strip_prefix('.')?;
    Some(rest.trim().to_string())
}

fn parse_section(rest: &str) -> Option<(String, bool)> {
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }

    // Quoted server name: `"name"`, `'name'`, optionally followed by `.env`.
    if rest.starts_with('"') || rest.starts_with('\'') {
        let quote = rest.chars().next()?;
        let end = find_quote_end(rest, quote)?;
        let name = unquote(&rest[..=end]);
        let tail = rest[end + 1..].trim();
        if tail.is_empty() {
            return Some((name, false));
        }
        if tail == ".env" {
            return Some((name, true));
        }
        // A nested table under a quoted server (e.g. `"a.b".timeout`) is not
        // a server definition; leave it to the current server's tail.
        return None;
    }

    // Bare name: only `NAME` or `NAME.env`. Any other dot is a nested table,
    // not a server name (TOML bare keys cannot contain dots).
    if let Some(stripped) = rest.strip_suffix(".env") {
        if stripped.contains('.') {
            return None;
        }
        return Some((stripped.to_string(), true));
    }
    if rest.contains('.') {
        return None;
    }
    Some((rest.to_string(), false))
}

fn find_quote_end(input: &str, quote: char) -> Option<usize> {
    let mut escaped = false;
    let mut iter = input.char_indices();
    iter.next()?; // skip the opening quote
    for (idx, ch) in iter {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            return Some(idx);
        }
    }
    None
}

fn parse_key_value(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() || key.starts_with('#') {
        return None;
    }
    Some((unquote(key), unquote(strip_inline_comment(value).trim())))
}

fn strip_inline_comment(value: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (idx, ch) in value.char_indices() {
        match quote {
            Some('"') => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    quote = None;
                }
            }
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                }
            }
            _ => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch == '#' {
                    return &value[..idx];
                }
            }
        }
    }
    value
}

fn array_is_closed(value: &str) -> bool {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut saw_bracket = false;
    for ch in value.chars() {
        match quote {
            Some('"') => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    quote = None;
                }
            }
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                }
            }
            _ => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch == '[' {
                    depth += 1;
                    saw_bracket = true;
                } else if ch == ']' {
                    depth -= 1;
                    saw_bracket = true;
                }
            }
        }
    }
    saw_bracket && depth <= 0
}

fn split_top_level(input: &str, separator: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;
    for ch in input.chars() {
        match ch {
            '\\' if in_string => {
                escaped = !escaped;
                current.push(ch);
            }
            '"' if !escaped => {
                in_string = !in_string;
                current.push(ch);
            }
            c if c == separator && !in_string => {
                out.push(current.trim().to_string());
                current.clear();
            }
            _ => {
                escaped = false;
                current.push(ch);
            }
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

fn parse_array(value: &str) -> Vec<String> {
    let inner = value
        .trim()
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(value);
    split_top_level(inner, ',')
        .into_iter()
        .filter(|item| !item.is_empty())
        .map(|item| unquote(&item))
        .collect()
}

fn parse_inline_table(value: &str) -> BTreeMap<String, String> {
    let inner = value
        .trim()
        .strip_prefix('{')
        .and_then(|v| v.strip_suffix('}'))
        .unwrap_or("");
    let mut out = BTreeMap::new();
    for item in split_top_level(inner, ',') {
        if item.is_empty() {
            continue;
        }
        if let Some((key, value)) = item.split_once('=') {
            out.insert(unquote(key.trim()), unquote(value.trim()));
        }
    }
    out
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        serde_json::from_str::<String>(value)
            .unwrap_or_else(|_| value[1..value.len() - 1].to_string())
    } else if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

fn toml_quote(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\""))
}

fn toml_key(key: &str) -> String {
    if key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        key.to_string()
    } else {
        toml_quote(key)
    }
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

    #[test]
    fn parses_servers_and_env() {
        let cfg = CodexConfig::parse(SAMPLE.to_string());
        assert_eq!(cfg.servers.len(), 2);
        let pg = cfg.servers.iter().find(|s| s.name == "postgres").unwrap();
        assert_eq!(pg.command.as_deref(), Some("npx"));
        assert_eq!(pg.args.len(), 2);
        assert_eq!(
            pg.env.get("DATABASE_URL").map(String::as_str),
            Some("postgres://u:p@h/db")
        );
    }

    #[test]
    fn rewrites_and_strips_secret_env() {
        let mut cfg = CodexConfig::parse(SAMPLE.to_string());
        let changes = cfg.apply();
        assert_eq!(changes.len(), 1);
        let text = cfg.lines.join("\n");
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
        let mut cfg = CodexConfig::parse(SAMPLE.to_string());
        cfg.apply_with_launcher("/old/agentvault", Some(std::path::Path::new("/old/home")));
        let mut cfg = CodexConfig::parse(cfg.lines.join("\n"));
        let changes =
            cfg.refresh_launcher("/new/agentvault", Some(std::path::Path::new("/new/home")));
        assert_eq!(changes.len(), 1);
        let rendered = cfg.lines.join("\n");
        let reparsed = CodexConfig::parse(rendered);
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
        assert!(reparsed
            .servers
            .iter()
            .find(|server| server.name == "postgres")
            .map(|server| CodexConfig::is_wrapped(server))
            .unwrap_or(false));
        assert!(cfg
            .refresh_launcher("/new/agentvault", Some(std::path::Path::new("/new/home")))
            .is_empty());
    }

    #[test]
    fn apply_is_idempotent() {
        let mut cfg = CodexConfig::parse(SAMPLE.to_string());
        cfg.apply();
        let rendered = cfg.lines.join("\n");
        let mut reparsed = CodexConfig::parse(rendered);
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
        let mut cfg = CodexConfig::parse(text.to_string());
        let changes = cfg.apply();
        assert_eq!(changes.len(), 1);
        let rendered = cfg.lines.join("\n");
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
        let cfg = CodexConfig::parse(text.to_string());
        let server = &cfg.servers[0];
        assert_eq!(server.args.len(), 2);
        assert_eq!(server.args[1], "@modelcontextprotocol/server-postgres");
        let mut cfg = cfg;
        cfg.apply_with_launcher("agentvault.exe", Some(std::path::Path::new(r"C:\av home")));
        let rendered = cfg.lines.join("\n");
        assert!(rendered.contains("--home"));
        assert!(rendered.contains("run"));
        assert!(!rendered.contains("@modelcontextprotocol/server-postgres\",\n]"));
        assert!(!rendered.contains("  \"-y\","));
        assert!(rendered.contains("startup_timeout_sec = 20"));
        let mut reparsed = CodexConfig::parse(rendered);
        assert!(CodexConfig::is_wrapped(&reparsed.servers[0]));
        assert!(reparsed.apply().is_empty());
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
        let cfg = CodexConfig::parse(text.to_string());
        assert_eq!(cfg.servers.len(), 1);
        assert_eq!(cfg.servers[0].command.as_deref(), Some("db-mcp"));
        let mut cfg = cfg;
        cfg.apply();
        let rendered = cfg.lines.join("\n");
        assert!(rendered.contains("[mcp_servers.db.timeout]"));
        assert!(rendered.contains("command = \"evil\""));
    }

    #[test]
    fn keeps_inline_env_when_env_subtable_exists() {
        let text = r#"
[mcp_servers.db]
command = "db-mcp"
env = { DB_PASSWORD = "hunter2", LOG_LEVEL = "info" }

[mcp_servers.db.env]
EXTRA_FLAG = "1"
"#;
        let mut cfg = CodexConfig::parse(text.to_string());
        cfg.apply();
        let rendered = cfg.lines.join("\n");
        assert!(!rendered.contains("DB_PASSWORD ="));
        assert!(rendered.contains("LOG_LEVEL"));
        assert!(rendered.contains("EXTRA_FLAG"));
    }
}
