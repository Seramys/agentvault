//! Adapters for agent MCP configuration files.
//!
//! v0.1 supports the JSON `mcpServers` shape used by Claude Code and Cursor.
//! Codex's TOML config is detected and reported as unsupported instead of being
//! silently rewritten.

pub mod codex;

use agentvault_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Cursor,
    Codex,
    Gemini,
    Windsurf,
    Cline,
    Qoder,
    Generic,
}

impl AgentKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "claude" | "claude-code" => Some(Self::Claude),
            "cursor" => Some(Self::Cursor),
            "codex" => Some(Self::Codex),
            "gemini" | "gemini-cli" => Some(Self::Gemini),
            "windsurf" | "codeium" => Some(Self::Windsurf),
            "cline" | "claude-dev" => Some(Self::Cline),
            "qoder" | "qoder-cli" => Some(Self::Qoder),
            "generic" | "auto" => Some(Self::Generic),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Cursor => "cursor",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Windsurf => "windsurf",
            Self::Cline => "cline",
            Self::Qoder => "qoder",
            Self::Generic => "generic",
        }
    }

    pub fn supports_toml(&self) -> bool {
        matches!(self, Self::Codex)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct McpServer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Fields this adapter does not interpret (`type`, `cwd`, `disabled`,
    /// `headers`, `timeout`, `trust`, `autoApprove`, ...). MCP clients define
    /// their own extras, so dropping them would change server behaviour.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct McpConfig {
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: BTreeMap<String, McpServer>,
    /// Preserve unrelated top-level keys instead of dropping them on rewrite.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl McpConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read(path)?;
        let data = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&data);
        let text = std::str::from_utf8(data)
            .map_err(|e| Error::Other(format!("config is not valid UTF-8: {e}")))?;
        match serde_json::from_str(text) {
            Ok(cfg) => Ok(cfg),
            Err(e) => {
                if looks_like_jsonc(text) {
                    Err(Error::Other(format!(
                        "{} looks like JSONC (comments or trailing commas). \
                         Remove them or rewrite the file as strict JSON before running init",
                        path.display()
                    )))
                } else {
                    Err(e.into())
                }
            }
        }
    }

    pub fn load_or_default(path: &Path) -> Result<Self> {
        if path.exists() {
            Self::load(path)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let mut data = serde_json::to_vec_pretty(self)?;
        data.push(b'\n');
        agentvault_core::util::atomic_write(path, &data)
    }
}

#[derive(Clone, Debug)]
pub struct Change {
    pub server: String,
    pub before: String,
    pub after: String,
    pub migrated_env: Vec<String>,
    /// Secret-looking keys deliberately kept in the config (for example
    /// `${env:NAME}` references or keys excluded with `--keep-env`).
    pub kept_env: Vec<String>,
    /// Non-secret env keys left in the config untouched.
    pub remaining_env: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct RewritePlan {
    pub changes: Vec<Change>,
    pub skipped: Vec<String>,
}

/// Per-run overrides for the env migration heuristic.
#[derive(Clone, Debug, Default)]
pub struct Policy {
    pub force: std::collections::BTreeSet<String>,
    pub skip: std::collections::BTreeSet<String>,
}

/// True when a command is the agentvault launcher, allowing global flags such
/// as `--home DIR` before the `run` subcommand.
pub fn is_agentvault_command(command: &str) -> bool {
    let name = Path::new(command)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(command);
    name.eq_ignore_ascii_case("agentvault") || name.eq_ignore_ascii_case("agentvault.exe")
}

/// Index of the `run` subcommand in a wrapped argument vector. Global flags
/// generated by `init` may precede it (`--home <dir>`); anything else means
/// this is not an agentvault wrapper.
pub fn wrapped_run_index(args: &[String]) -> Option<usize> {
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--home" => index += 2,
            value if value.starts_with("--home=") => index += 1,
            "--no-autostart" => index += 1,
            "run" => return Some(index),
            _ => return None,
        }
    }
    None
}

pub fn is_already_wrapped(server: &McpServer) -> bool {
    let Some(command) = server.command.as_deref() else {
        return false;
    };
    is_agentvault_command(command) && wrapped_run_index(&server.args).is_some()
}

/// Decide what would change without touching the file.
pub fn plan(config: &McpConfig) -> RewritePlan {
    plan_with(config, &Policy::default())
}

pub fn plan_with(config: &McpConfig, policy: &Policy) -> RewritePlan {
    let mut out = RewritePlan::default();
    for (name, server) in &config.mcp_servers {
        if server.command.is_none() {
            out.skipped.push(format!("{name} (url-based, no command)"));
            continue;
        }
        if is_already_wrapped(server) {
            out.skipped.push(format!("{name} (already wrapped)"));
            continue;
        }
        let before = format!(
            "{} {}",
            server.command.clone().unwrap_or_default(),
            server.args.join(" ")
        )
        .trim()
        .to_string();
        let after = format!("agentvault run --name {name} -- {before}");
        let migrated_env = migrated_keys(server, policy);
        let kept_env = kept_keys(server, policy, &migrated_env);
        let remaining_env = remaining_keys(server, &migrated_env, &kept_env);
        out.changes.push(Change {
            server: name.clone(),
            before,
            after,
            migrated_env,
            kept_env,
            remaining_env,
        });
    }
    out
}

/// Rewrite in memory: wrap the command and move secret-looking env values out.
/// The caller is responsible for storing those values in the keystore first.
pub fn apply(config: &mut McpConfig) -> Vec<Change> {
    apply_with_launcher_and(config, "agentvault", None, &Policy::default())
}

/// Wrap commands with an explicit launcher path (usually the running binary).
/// This removes any dependency on `agentvault` being on PATH, which matters
/// when the user installed it in a custom directory.
pub fn apply_with_launcher(
    config: &mut McpConfig,
    launcher: &str,
    home: Option<&std::path::Path>,
) -> Vec<Change> {
    apply_with_launcher_and(config, launcher, home, &Policy::default())
}

pub fn apply_with_launcher_and(
    config: &mut McpConfig,
    launcher: &str,
    home: Option<&std::path::Path>,
    policy: &Policy,
) -> Vec<Change> {
    let mut changes = Vec::new();
    for (name, server) in config.mcp_servers.iter_mut() {
        if is_already_wrapped(server) || server.command.is_none() {
            continue;
        }
        let before = format!(
            "{} {}",
            server.command.clone().unwrap_or_default(),
            server.args.join(" ")
        )
        .trim()
        .to_string();
        let original_command = server.command.clone().unwrap_or_default();
        let original_args = server.args.clone();
        let migrated_env = migrated_keys(server, policy);
        let kept_env = kept_keys(server, policy, &migrated_env);
        let remaining_env = remaining_keys(server, &migrated_env, &kept_env);

        server.command = Some(launcher.to_string());
        let mut args = Vec::new();
        if let Some(home) = home {
            args.push("--home".to_string());
            args.push(home.display().to_string());
        }
        args.push("run".to_string());
        args.push("--name".to_string());
        args.push(name.clone());
        for key in &migrated_env {
            args.push("--env".to_string());
            args.push(format!("{key}=secret:{name}/{key}"));
        }
        args.push("--".to_string());
        args.push(original_command);
        args.extend(original_args);
        server.args = args;
        server
            .env
            .retain(|key, _| !migrated_env.iter().any(|migrated| migrated == key));

        changes.push(Change {
            server: name.clone(),
            before,
            after: "agentvault run -- ...".to_string(),
            migrated_env,
            kept_env,
            remaining_env,
        });
    }
    changes
}

fn migrated_keys(server: &McpServer, policy: &Policy) -> Vec<String> {
    server
        .env
        .iter()
        .filter(|(key, value)| should_migrate_env(key, value, policy))
        .map(|(key, _)| key.clone())
        .collect()
}

fn kept_keys(server: &McpServer, policy: &Policy, migrated: &[String]) -> Vec<String> {
    server
        .env
        .iter()
        .filter(|(key, value)| {
            !migrated.iter().any(|m| m == *key)
                && (policy.skip.contains(*key) || is_env_reference(value))
                && (looks_like_secret_key(key) || policy.force.contains(*key))
        })
        .map(|(key, _)| key.clone())
        .collect()
}

fn remaining_keys(server: &McpServer, migrated: &[String], kept: &[String]) -> Vec<String> {
    server
        .env
        .keys()
        .filter(|key| !migrated.contains(*key) && !kept.contains(*key))
        .cloned()
        .collect()
}

pub(crate) fn wrapped_home(args: &[String]) -> Option<String> {
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--home" {
            return args.get(index + 1).cloned();
        }
        if let Some(value) = args[index].strip_prefix("--home=") {
            return Some(value.to_string());
        }
        index += 1;
    }
    None
}

/// Point already wrapped servers at another launcher or home without touching
/// their env or the wrapped command.
pub fn refresh_with_launcher_and(
    config: &mut McpConfig,
    launcher: &str,
    home: Option<&Path>,
) -> Vec<Change> {
    let mut changes = Vec::new();
    for (name, server) in config.mcp_servers.iter_mut() {
        let Some(run_index) = wrapped_run_index(&server.args) else {
            continue;
        };
        let old_prefix = &server.args[..run_index];
        let new_home = home.map(|path| path.display().to_string());
        if server.command.as_deref() == Some(launcher) && wrapped_home(old_prefix) == new_home {
            continue;
        }
        let before = format!(
            "{} {}",
            server.command.clone().unwrap_or_default(),
            server.args.join(" ")
        )
        .trim()
        .to_string();
        let mut prefix = Vec::new();
        let mut index = 0;
        while index < old_prefix.len() {
            match old_prefix[index].as_str() {
                "--home" => index += 2,
                value if value.starts_with("--home=") => index += 1,
                "--no-autostart" => {
                    prefix.push("--no-autostart".to_string());
                    index += 1;
                }
                _ => index += 1,
            }
        }
        if let Some(home) = home {
            prefix.push("--home".to_string());
            prefix.push(home.display().to_string());
        }
        prefix.extend_from_slice(&server.args[run_index..]);
        server.command = Some(launcher.to_string());
        server.args = prefix;
        changes.push(Change {
            server: name.clone(),
            before,
            after: format!(
                "{} --home {}",
                launcher,
                home.map(|path| path.display().to_string())
                    .unwrap_or_else(|| "(none)".into())
            ),
            migrated_env: Vec::new(),
            kept_env: Vec::new(),
            remaining_env: server.env.keys().cloned().collect(),
        });
    }
    changes
}

/// Env values that are references to another source must stay references; the
/// MCP client expands them at spawn time. References can be embedded in a
/// larger value (`${HOME}/.config`, `prefix-{{ENV}}-suffix`), not only stand
/// alone.
pub fn is_env_reference(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    if (value.contains("${") && value.contains('}'))
        || (value.contains("{{") && value.contains("}}"))
    {
        return true;
    }
    if has_percent_reference(value) {
        return true;
    }
    if (value.starts_with('$') && value.ends_with('}'))
        || (value.starts_with('$')
            && value
                .chars()
                .skip(1)
                .all(|c| c.is_ascii_alphanumeric() || c == '_'))
    {
        return true;
    }
    // `$HOME/.config`, `$XDG_CONFIG_HOME/x`: a leading bare variable followed
    // by a path separator is still a reference.
    if let Some(rest) = value.strip_prefix('$') {
        let name_len = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .count();
        if name_len > 0 {
            let after = &rest[name_len..];
            if after.starts_with('/') || after.starts_with('\\') {
                return true;
            }
        }
    }
    false
}

fn has_percent_reference(value: &str) -> bool {
    let mut chars = value.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if c != '%' {
            continue;
        }
        let mut end = None;
        for (index, next) in chars.by_ref() {
            if next == '%' {
                end = Some(index);
                break;
            }
            if !(next.is_ascii_alphanumeric() || next == '_') {
                break;
            }
        }
        if let Some(end) = end {
            if end > start + 1 {
                return true;
            }
        }
    }
    false
}

pub fn should_migrate_env(key: &str, value: &str, policy: &Policy) -> bool {
    if policy.skip.contains(key) || is_env_reference(value) {
        return false;
    }
    if policy.force.contains(key) {
        return true;
    }
    looks_like_secret_key(key) || looks_like_secret_value(value)
}

pub fn looks_like_secret_key(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    const NON_SECRET: [&str; 13] = [
        "MAX_TOKENS",
        "TOKEN_LIMIT",
        "TOKENIZER",
        "TOKEN_COUNT",
        "CONTEXT_TOKENS",
        "TOKEN_BUDGET",
        "PUBLIC_KEY",
        "KEYBOARD",
        "MONKEY",
        "BYPASS",
        "PASSAGE",
        "COMPASS",
        "AUTHOR",
    ];
    if NON_SECRET.iter().any(|marker| key.contains(marker)) {
        return false;
    }
    const MARKERS: [&str; 25] = [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "API_KEY",
        "APIKEY",
        "ACCESS_KEY",
        "PRIVATE_KEY",
        "ENCRYPTION_KEY",
        "SIGNING_KEY",
        "DATABASE_URL",
        "POSTGRES_URL",
        "MYSQL_URL",
        "MONGO_URI",
        "REDIS_URL",
        "AMQP_URL",
        "DB_PASS",
        "DB_PWD",
        "PASS",
        "AUTH",
        "CREDENTIAL",
        "DSN",
        "WEBHOOK",
        "COOKIE",
        "SIGNING",
    ];
    MARKERS.iter().any(|marker| key.contains(marker))
        || key.ends_with("_KEY")
        || key.starts_with("KEY_")
        || key.contains("_PASS_")
}

/// Value-shaped detection catches innocuous key names (`OPENAI_KEY`,
/// `POSTGRES_URL`) without migrating every environment variable.
pub fn looks_like_secret_value(value: &str) -> bool {
    let value = value.trim();
    if value.len() < 8 {
        return false;
    }
    if value.starts_with("-----BEGIN ")
        || value.starts_with("Bearer ")
        || value.starts_with("Basic ")
    {
        return true;
    }
    if value.contains(char::is_whitespace) || value.contains('\\') {
        return false;
    }
    const PREFIXES: [&str; 15] = [
        "sk-",
        "sk_",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "xapp-",
        "AKIA",
        "AIza",
        "-----BEGIN ",
        "Bearer ",
    ];
    if PREFIXES.iter().any(|prefix| value.starts_with(prefix)) {
        return true;
    }
    if value.starts_with("eyJ") && value.matches('.').count() == 2 {
        return true;
    }
    let credential_url = [
        "postgres://",
        "postgresql://",
        "mysql://",
        "mongodb://",
        "mongodb+srv://",
        "redis://",
        "amqp://",
    ]
    .iter()
    .any(|scheme| value.starts_with(scheme))
        && value
            .split_once("://")
            .map(|(_, rest)| rest.contains('@'))
            .unwrap_or(false);
    if credential_url {
        return true;
    }
    if (value.starts_with("http://") || value.starts_with("https://"))
        && value
            .split_once("://")
            .map(|(_, rest)| rest.split('/').next().unwrap_or("").contains('@'))
            .unwrap_or(false)
    {
        return true;
    }
    if value.starts_with('/')
        || value.starts_with("./")
        || value.starts_with("~/")
        || value.starts_with("http://")
        || value.starts_with("https://")
    {
        return false;
    }
    let long_enough = value.len() >= 32;
    let hexish = value.chars().all(|c| c.is_ascii_hexdigit())
        && value.chars().any(|c| c.is_ascii_alphabetic());
    let base64ish = value.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '-' || c == '_'
    }) && value.chars().any(|c| c.is_ascii_digit())
        && value.chars().any(|c| c.is_ascii_alphabetic());
    long_enough && (hexish || base64ish)
}

/// Candidate config paths for one agent, project-local first.
pub fn candidate_paths(kind: AgentKind) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match kind {
        AgentKind::Claude => {
            out.push(cwd.join(".mcp.json"));
            if let Some(home) = agentvault_core::util::home_dir() {
                out.push(home.join(".claude.json"));
                out.push(home.join(".claude").join("mcp.json"));
            }
        }
        AgentKind::Cursor => {
            out.push(cwd.join(".cursor").join("mcp.json"));
            if let Some(home) = agentvault_core::util::home_dir() {
                out.push(home.join(".cursor").join("mcp.json"));
            }
        }
        AgentKind::Codex => {
            if let Some(home) = agentvault_core::util::home_dir() {
                out.push(home.join(".codex").join("config.toml"));
            }
        }
        AgentKind::Gemini => {
            out.push(cwd.join(".gemini").join("settings.json"));
            if let Some(home) = agentvault_core::util::home_dir() {
                out.push(home.join(".gemini").join("settings.json"));
            }
        }
        AgentKind::Windsurf => {
            out.push(cwd.join(".windsurf").join("mcp.json"));
            if let Some(home) = agentvault_core::util::home_dir() {
                out.push(
                    home.join(".codeium")
                        .join("windsurf")
                        .join("mcp_config.json"),
                );
            }
        }
        AgentKind::Cline => {
            out.extend(cline_settings_paths());
        }
        AgentKind::Qoder => {
            out.push(cwd.join(".qoder").join("settings.json"));
            out.push(cwd.join(".qoder").join("settings.local.json"));
            out.push(cwd.join(".mcp.json"));
            if let Some(home) = agentvault_core::util::home_dir() {
                out.push(home.join(".qoder").join("settings.json"));
            }
        }
        AgentKind::Generic => {
            out.push(cwd.join("mcp.json"));
            out.push(cwd.join(".mcp.json"));
        }
    }
    out
}

/// Best-effort kind inference for an explicit `--path`. The file location is
/// much more reliable than "first config that exists" because project-local
/// `.mcp.json` files are shared by several clients.
pub fn infer_kind_for_path(path: &Path) -> Option<AgentKind> {
    let text = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let file = text.rsplit('/').next().unwrap_or("");
    if file == "config.toml" && text.contains("/.codex/") {
        return Some(AgentKind::Codex);
    }
    if file == "cline_mcp_settings.json" {
        return Some(AgentKind::Cline);
    }
    if file == "mcp.json" && text.contains("/.cursor/") {
        return Some(AgentKind::Cursor);
    }
    if file == "mcp_config.json" && text.contains("windsurf") {
        return Some(AgentKind::Windsurf);
    }
    if file == "settings.json" && text.contains("/.gemini/") {
        return Some(AgentKind::Gemini);
    }
    if (file == "settings.json" || file == "settings.local.json") && text.contains("/.qoder/") {
        return Some(AgentKind::Qoder);
    }
    if file == ".mcp.json" || file == ".claude.json" || text.contains("/.claude/") {
        return Some(AgentKind::Claude);
    }
    if file == "settings.json" && text.contains("/.qoder/") {
        return Some(AgentKind::Qoder);
    }
    if file.ends_with(".toml") {
        return Some(AgentKind::Codex);
    }
    if file == "mcp.json" || file == "mcp_config.json" {
        return Some(AgentKind::Generic);
    }
    None
}

fn looks_like_jsonc(text: &str) -> bool {
    let mut in_string = false;
    let mut escaped = false;
    let bytes: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < bytes.len() {
        let ch = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if ch == '"' {
            in_string = true;
            index += 1;
            continue;
        }
        if ch == '/'
            && index + 1 < bytes.len()
            && (bytes[index + 1] == '/' || bytes[index + 1] == '*')
        {
            return true;
        }
        if ch == ',' {
            let mut next = index + 1;
            while next < bytes.len() && bytes[next].is_whitespace() {
                next += 1;
            }
            if next < bytes.len() && matches!(bytes[next], '}' | ']') {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn cline_settings_paths() -> Vec<PathBuf> {
    let variants = ["Code", "Cursor", "Code - Insiders"];
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        let Some(base) = std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| {
            agentvault_core::util::home_dir().map(|h| h.join("AppData").join("Roaming"))
        }) else {
            return out;
        };
        for variant in variants {
            out.push(
                base.join(variant)
                    .join("User")
                    .join("globalStorage")
                    .join("saoudrizwan.claude-dev")
                    .join("settings")
                    .join("cline_mcp_settings.json"),
            );
        }
    }
    #[cfg(target_os = "macos")]
    {
        let Some(home) = agentvault_core::util::home_dir() else {
            return out;
        };
        for variant in variants {
            out.push(
                home.join("Library")
                    .join("Application Support")
                    .join(variant)
                    .join("User")
                    .join("globalStorage")
                    .join("saoudrizwan.claude-dev")
                    .join("settings")
                    .join("cline_mcp_settings.json"),
            );
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let Some(home) = agentvault_core::util::home_dir() else {
            return out;
        };
        for variant in variants {
            out.push(
                home.join(".config")
                    .join(variant)
                    .join("User")
                    .join("globalStorage")
                    .join("saoudrizwan.claude-dev")
                    .join("settings")
                    .join("cline_mcp_settings.json"),
            );
        }
    }
    out
}

pub fn backup_file(path: &Path) -> Result<PathBuf> {
    let stamp = agentvault_core::now_ms();
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("config");
    let backup = path.with_file_name(format!("{name}.bak.{stamp}"));
    std::fs::copy(path, &backup)?;
    Ok(backup)
}

pub fn restore_file(backup: &Path, target: &Path) -> Result<()> {
    std::fs::copy(backup, target)?;
    Ok(())
}

pub fn verify_wrapped(path: &Path, servers: &[String]) -> Result<()> {
    let config = McpConfig::load(path)?;
    for name in servers {
        let server = config
            .mcp_servers
            .get(name)
            .ok_or_else(|| Error::Other(format!("server `{name}` missing after rewrite")))?;
        if !is_already_wrapped(server) {
            return Err(Error::Other(format!("server `{name}` was not wrapped")));
        }
    }
    Ok(())
}

pub fn resolve_target(path: &Path) -> PathBuf {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.display().to_string();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text).to_string();
    PathBuf::from(text)
}

pub fn first_existing(kind: AgentKind) -> Option<PathBuf> {
    candidate_paths(kind).into_iter().find(|p| p.exists())
}

pub fn load_any(path: &Path) -> Result<McpConfig> {
    if path.extension().and_then(|e| e.to_str()) == Some("toml") {
        return Err(Error::Other(
            "TOML configs are handled by `agentvault_adapters::codex` in the CLI init path".into(),
        ));
    }
    McpConfig::load(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> McpConfig {
        let mut env = BTreeMap::new();
        env.insert(
            "DATABASE_URL".to_string(),
            "postgres://u:p@h/db".to_string(),
        );
        env.insert("LOG_LEVEL".to_string(), "debug".to_string());
        env.insert("MAX_TOKENS".to_string(), "4096".to_string());
        env.insert("OPENAI_KEY".to_string(), "sk-live-123".to_string());
        env.insert(
            "GITHUB_TOKEN_REF".to_string(),
            "${env:DATABASE_URL}".to_string(),
        );
        let mut extra = BTreeMap::new();
        extra.insert("type".into(), serde_json::json!("stdio"));
        extra.insert("disabled".into(), serde_json::json!(true));
        let mut servers = BTreeMap::new();
        servers.insert(
            "postgres".to_string(),
            McpServer {
                command: Some("npx".into()),
                args: vec!["-y".into(), "@modelcontextprotocol/server-postgres".into()],
                env,
                url: None,
                extra,
            },
        );
        servers.insert(
            "remote".to_string(),
            McpServer {
                command: None,
                args: vec![],
                env: BTreeMap::new(),
                url: Some("https://x".into()),
                extra: BTreeMap::new(),
            },
        );
        McpConfig {
            mcp_servers: servers,
            ..Default::default()
        }
    }

    #[test]
    fn plan_lists_changes_and_skips() {
        let cfg = sample();
        let plan = plan(&cfg);
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].server, "postgres");
        assert!(plan.skipped.iter().any(|s| s.contains("remote")));
        assert_eq!(plan.changes[0].migrated_env.len(), 2);
        assert!(plan.changes[0]
            .migrated_env
            .contains(&"DATABASE_URL".into()));
        assert!(plan.changes[0].migrated_env.contains(&"OPENAI_KEY".into()));
        assert!(plan.changes[0]
            .kept_env
            .contains(&"GITHUB_TOKEN_REF".into()));
        assert!(plan.changes[0].remaining_env.contains(&"MAX_TOKENS".into()));
        assert!(plan.changes[0].remaining_env.contains(&"LOG_LEVEL".into()));
        assert!(!plan.changes[0]
            .remaining_env
            .contains(&"GITHUB_TOKEN_REF".into()));
    }

    #[test]
    fn refresh_updates_wrapped_launcher_and_home() {
        let mut cfg = sample();
        apply_with_launcher_and(
            &mut cfg,
            "/old/agentvault",
            Some(Path::new("/old/home")),
            &Policy::default(),
        );
        let changes =
            refresh_with_launcher_and(&mut cfg, "/new/agentvault", Some(Path::new("/new/home")));
        assert_eq!(changes.len(), 1);
        let server = &cfg.mcp_servers["postgres"];
        assert_eq!(server.command.as_deref(), Some("/new/agentvault"));
        assert_eq!(wrapped_home(&server.args).as_deref(), Some("/new/home"));
        assert!(server.args.contains(&"npx".to_string()));
        assert!(refresh_with_launcher_and(
            &mut cfg,
            "/new/agentvault",
            Some(Path::new("/new/home"))
        )
        .is_empty());
    }

    #[test]
    fn apply_wraps_and_strips_only_secret_env() {
        let mut cfg = sample();
        let changes = apply(&mut cfg);
        assert_eq!(changes.len(), 1);
        let server = &cfg.mcp_servers["postgres"];
        assert_eq!(server.command.as_deref(), Some("agentvault"));
        assert_eq!(server.args[0], "run");
        assert_eq!(server.args[2], "postgres");
        assert!(!server.env.contains_key("DATABASE_URL"));
        assert!(!server.env.contains_key("OPENAI_KEY"));
        assert_eq!(
            server.env.get("MAX_TOKENS").map(String::as_str),
            Some("4096")
        );
        assert_eq!(
            server.env.get("LOG_LEVEL").map(String::as_str),
            Some("debug")
        );
        assert_eq!(
            server.extra.get("disabled").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            server.extra.get("type").and_then(|v| v.as_str()),
            Some("stdio")
        );
    }

    #[test]
    fn apply_is_idempotent() {
        let mut cfg = sample();
        apply(&mut cfg);
        let again = apply(&mut cfg);
        assert!(again.is_empty());
    }

    #[test]
    fn wrapped_with_home_is_recognized() {
        let server = McpServer {
            command: Some("/usr/local/bin/agentvault".into()),
            args: vec![
                "--home".into(),
                "/tmp/av home".into(),
                "run".into(),
                "--name".into(),
                "pg".into(),
                "--".into(),
                "npx".into(),
            ],
            env: BTreeMap::new(),
            url: None,
            extra: BTreeMap::new(),
        };
        assert!(is_already_wrapped(&server));
    }

    #[test]
    fn env_references_are_never_migrated() {
        let policy = Policy::default();
        assert!(!should_migrate_env(
            "DATABASE_URL",
            "${env:DATABASE_URL}",
            &policy
        ));
        assert!(!should_migrate_env(
            "DATABASE_URL",
            "$DATABASE_URL",
            &policy
        ));
        assert!(!should_migrate_env(
            "DATABASE_URL",
            "%DATABASE_URL%",
            &policy
        ));
        assert!(!should_migrate_env(
            "PASS_THROUGH",
            "${HOME}/.config",
            &policy
        ));
        assert!(!should_migrate_env(
            "PASS_THROUGH",
            "$HOME/.config",
            &policy
        ));
        assert!(!should_migrate_env(
            "PASS_THROUGH",
            "%USERPROFILE%\\AppData",
            &policy
        ));
        assert!(!should_migrate_env(
            "API_KEY",
            "prefix-${API_KEY}-suffix",
            &policy
        ));
        assert!(!should_migrate_env(
            "API_KEY",
            "prefix-{{env}}-suffix",
            &policy
        ));
        assert!(should_migrate_env("API_KEY", "p$ssword!", &policy));
        assert!(should_migrate_env(
            "DATABASE_URL",
            "postgres://u:p@h/db",
            &policy
        ));
        assert!(should_migrate_env("DB_PASS", "hunter2", &policy));
        assert!(should_migrate_env(
            "POSTGRES_URL",
            "postgres://u:p@h/db",
            &policy
        ));
        assert!(!should_migrate_env("MAX_TOKENS", "4096", &policy));
    }

    #[test]
    fn policy_can_force_and_skip() {
        let mut policy = Policy::default();
        policy.force.insert("CUSTOM_ENV".into());
        policy.skip.insert("DATABASE_URL".into());
        assert!(should_migrate_env("CUSTOM_ENV", "plain", &policy));
        assert!(!should_migrate_env(
            "DATABASE_URL",
            "postgres://u:p@h/db",
            &policy
        ));

        let plan = plan_with(&sample(), &policy);
        assert!(!plan.changes[0]
            .migrated_env
            .contains(&"DATABASE_URL".into()));
        assert!(plan.changes[0].kept_env.contains(&"DATABASE_URL".into()));
    }

    #[test]
    fn infers_kind_from_path() {
        assert_eq!(
            infer_kind_for_path(Path::new(r"C:\Users\x\.codex\config.toml")),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            infer_kind_for_path(Path::new(r"C:\Users\x\.qoder\settings.json")),
            Some(AgentKind::Qoder)
        );
        assert_eq!(
            infer_kind_for_path(Path::new(r"C:\Users\x\.cursor\mcp.json")),
            Some(AgentKind::Cursor)
        );
        assert_eq!(
            infer_kind_for_path(Path::new("config.toml")),
            Some(AgentKind::Codex)
        );
    }
}
