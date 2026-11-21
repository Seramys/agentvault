mod envcmd;

use agentvault_adapters::AgentKind;
use agentvault_broker::{client, server};
use agentvault_core::store;
use agentvault_core::{Result, Root, RootSource};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "agentvault",
    version,
    about = "Short-lived credentials for AI agents",
    long_about = "agentvault keeps long-lived secrets in the OS keystore, hands agents short-lived tickets, and records every use locally."
)]
struct Cli {
    #[arg(long, global = true, value_name = "DIR")]
    home: Option<PathBuf>,

    /// Do not start a broker automatically for commands that need one.
    #[arg(long, global = true)]
    no_autostart: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Discover agent MCP configs, migrate plaintext secrets and wrap commands.
    Init {
        #[arg(
            long,
            default_value = "auto",
            value_name = "claude|cursor|codex|gemini|windsurf|cline|qoder|generic|auto"
        )]
        agent: String,
        #[arg(long, value_name = "FILE")]
        path: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        /// Point already wrapped servers at the current binary and home.
        #[arg(long)]
        refresh: bool,
        #[arg(long)]
        rebind: bool,
        /// List every candidate config path and whether it exists.
        #[arg(long)]
        list: bool,
        /// Migrate these env keys even if the heuristic would keep them.
        #[arg(long = "migrate-env", value_name = "KEY", value_delimiter = ',')]
        migrate_env: Vec<String>,
        /// Keep these env keys in the config even if they look secret.
        #[arg(long = "keep-env", value_name = "KEY", value_delimiter = ',')]
        keep_env: Vec<String>,
        /// Do not add AGENTVAULT_HOME / the binary directory to the user env.
        #[arg(long)]
        no_env: bool,
    },
    /// Run the local broker (in the foreground by default).
    Daemon {
        #[arg(long, hide = true)]
        foreground: bool,
        /// Start a detached broker and return when it is ready.
        #[arg(long)]
        detach: bool,
    },
    /// Stop a broker started in the background.
    Stop,
    /// Manage long-lived secrets (layer 0).
    Secret {
        #[command(subcommand)]
        cmd: SecretCmd,
    },
    /// Issue a short-lived ticket (layer 1).
    Issue {
        #[arg(long, value_name = "NAME")]
        secret: String,
        #[arg(long, default_value = "cli")]
        sub: String,
        #[arg(long, default_value = "15m")]
        ttl: String,
        #[arg(long, default_value = "read")]
        scope: String,
        #[arg(long = "max-uses")]
        max_uses: Option<u32>,
        /// Issue a task (parent) ticket for a long-running agent task.
        #[arg(long)]
        task: bool,
        /// Absolute ceiling for a task ticket, e.g. 8h.
        #[arg(long = "hard-ttl", default_value = "8h")]
        hard_ttl: String,
        #[arg(long)]
        json: bool,
    },
    /// Exchange a ticket for its value exactly once.
    Resolve {
        #[arg(long)]
        ticket: String,
        #[arg(long)]
        json: bool,
    },
    /// Mint a short leaf ticket from a parent task ticket.
    Child {
        #[arg(long, value_name = "TICKET")]
        parent: Option<String>,
        #[arg(long = "parent-file", value_name = "FILE")]
        parent_file: Option<PathBuf>,
        #[arg(long, default_value = "60s")]
        ttl: String,
        #[arg(long)]
        sub: Option<String>,
        #[arg(long = "max-uses")]
        max_uses: Option<u32>,
        #[arg(long)]
        json: bool,
    },
    /// Slide a task ticket's expiry forward, capped by its hard limit.
    Renew {
        #[arg(long)]
        ticket: String,
        #[arg(long, default_value = "15m")]
        ttl: String,
        #[arg(long)]
        json: bool,
    },
    /// Wrap a process: issue a ticket, inject it and run the command.
    Run {
        #[arg(long, value_name = "SERVER")]
        name: String,
        #[arg(long, default_value = "15m")]
        ttl: String,
        #[arg(long, default_value = "read")]
        scope: String,
        #[arg(long = "env", value_name = "KEY=secret:NAME")]
        env: Vec<String>,
        /// Create a renewing task ticket and expose it via
        /// AGENTVAULT_PARENT_TICKET / AGENTVAULT_PARENT_FILE.
        #[arg(long)]
        session: bool,
        #[arg(long = "hard-ttl", default_value = "8h")]
        hard_ttl: String,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Revoke one ticket, one secret, or everything.
    Revoke {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        secret: Option<String>,
        #[arg(long)]
        ticket: Option<String>,
    },
    /// Rotate a stored secret and invalidate all of its tickets.
    Rotate {
        #[arg(long)]
        secret: String,
        #[arg(long)]
        json: bool,
    },
    /// Show stored secrets and live tickets.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Show the local audit trail (layer 4).
    Audit {
        #[arg(long, default_value_t = 20)]
        tail: usize,
        #[arg(long)]
        json: bool,
    },
    /// Manage honeypot canaries (layer 4).
    Honeypot {
        #[command(subcommand)]
        cmd: HoneypotCmd,
    },
    /// Inspect or persist AGENTVAULT_HOME / PATH for new terminals.
    Env {
        #[command(subcommand)]
        cmd: EnvCmd,
    },
    /// Diagnose state dir, broker, keystore and detected agent configs.
    Doctor,
}

#[derive(Subcommand, Debug)]
enum EnvCmd {
    /// Show the resolved home, where it came from, and persisted values.
    Show,
    /// Persist AGENTVAULT_HOME (user scope by default) and add the install dir to PATH.
    Install {
        #[arg(long, default_value = "user", value_name = "user|machine")]
        scope: String,
        #[arg(long)]
        no_path: bool,
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the persisted AGENTVAULT_HOME and PATH entry.
    Uninstall {
        #[arg(long, default_value = "user", value_name = "user|machine")]
        scope: String,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Debug)]
enum SecretCmd {
    Add {
        name: String,
        #[arg(long)]
        value: Option<String>,
        /// Read the value from stdin instead of the command line.
        #[arg(long)]
        stdin: bool,
        #[arg(long, default_value = "token")]
        kind: String,
        #[arg(long)]
        honeypot: bool,
    },
    List,
    Remove {
        name: String,
    },
    /// Allow a revoked secret name to be issued again without rotating values.
    Unrevoke {
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum HoneypotCmd {
    Add {
        name: String,
        #[arg(long)]
        json: bool,
    },
    List,
    Check {
        token: String,
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let text = info.to_string();
        // `agentvault ... | head` closes stdout early; that is not a crash.
        if text.contains("Broken pipe")
            || text.contains("管道正在被关闭")
            || text.contains("os error 232")
        {
            std::process::exit(0);
        }
        eprintln!("{text}");
    }));
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    if cli.no_autostart {
        std::env::set_var("AGENTVAULT_NO_AUTOSTART", "1");
    }
    let root = match &cli.home {
        Some(dir) => Root::new(agentvault_core::expand_path(&dir.to_string_lossy())),
        None => Root::discover()?,
    };
    if !matches!(&cli.cmd, Cmd::Doctor) {
        root.ensure()?;
    }

    match cli.cmd {
        Cmd::Init {
            agent,
            path,
            dry_run,
            refresh,
            rebind,
            list,
            migrate_env,
            keep_env,
            no_env,
        } => cmd_init(
            &root,
            &agent,
            path,
            dry_run,
            refresh,
            rebind,
            list,
            &migrate_env,
            &keep_env,
            no_env,
        ),
        Cmd::Daemon { detach, .. } => cmd_daemon(&root, detach),
        Cmd::Stop => {
            if client::stop(&root)? {
                println!("broker stopped");
            } else {
                println!("no broker was running");
            }
            Ok(())
        }
        Cmd::Secret { cmd } => cmd_secret(&root, cmd),
        Cmd::Issue {
            secret,
            sub,
            ttl,
            scope,
            max_uses,
            task,
            hard_ttl,
            json,
        } => {
            client::ensure_running(&root)?;
            let data = client::request(
                &root,
                "issue",
                json!({
                    "secret": secret,
                    "sub": sub,
                    "ttl": ttl,
                    "scope": scope,
                    "max_uses": max_uses,
                    "task": task,
                    "hard_ttl": hard_ttl,
                }),
            )?;
            if json {
                print_json(&data)?;
            } else {
                let ticket = data
                    .get("ticket")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                println!("{ticket}");
                eprintln!(
                    "kind={} jti={} expires_in={}s uses={} hard_exp={}",
                    data.get("kind").and_then(Value::as_str).unwrap_or("-"),
                    data.get("jti").and_then(Value::as_str).unwrap_or("-"),
                    data.get("ttl_secs").and_then(Value::as_u64).unwrap_or(0),
                    data.get("max_uses").and_then(Value::as_u64).unwrap_or(0),
                    data.get("hard_exp").and_then(Value::as_i64).unwrap_or(0)
                );
            }
            Ok(())
        }
        Cmd::Resolve { ticket, json } => {
            client::ensure_running(&root)?;
            let data = client::request(&root, "resolve", json!({ "ticket": ticket }))?;
            if json {
                print_json(&data)?;
            } else {
                println!(
                    "{}",
                    data.get("value")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                );
            }
            Ok(())
        }
        Cmd::Child {
            parent,
            parent_file,
            ttl,
            sub,
            max_uses,
            json,
        } => {
            client::ensure_running(&root)?;
            let parent_token = match (parent, parent_file) {
                (Some(token), _) => token,
                (None, Some(path)) => std::fs::read_to_string(&path)
                    .map_err(|e| {
                        agentvault_core::Error::Other(format!(
                            "cannot read parent file {}: {e}",
                            path.display()
                        ))
                    })?
                    .trim()
                    .to_string(),
                (None, None) => {
                    return Err(agentvault_core::Error::Other(
                        "child needs --parent or --parent-file".into(),
                    ))
                }
            };
            let data = client::request(
                &root,
                "child",
                json!({
                    "parent": parent_token,
                    "ttl": ttl,
                    "sub": sub,
                    "max_uses": max_uses,
                }),
            )?;
            if json {
                print_json(&data)?;
            } else {
                println!(
                    "{}",
                    data.get("ticket")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                );
                eprintln!(
                    "jti={} exp={} parent={}",
                    data.get("jti").and_then(Value::as_str).unwrap_or("-"),
                    data.get("exp").and_then(Value::as_i64).unwrap_or(0),
                    data.get("parent").and_then(Value::as_str).unwrap_or("-"),
                );
            }
            Ok(())
        }
        Cmd::Renew { ticket, ttl, json } => {
            client::ensure_running(&root)?;
            let data = client::request(&root, "renew", json!({ "ticket": ticket, "ttl": ttl }))?;
            if json {
                print_json(&data)?;
            } else {
                println!(
                    "{}",
                    data.get("ticket")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                );
                eprintln!(
                    "exp={} hard_exp={} renewals={}",
                    data.get("exp").and_then(Value::as_i64).unwrap_or(0),
                    data.get("hard_exp").and_then(Value::as_i64).unwrap_or(0),
                    data.get("renewals").and_then(Value::as_u64).unwrap_or(0),
                );
            }
            Ok(())
        }
        Cmd::Run {
            name,
            ttl,
            scope,
            env,
            session,
            hard_ttl,
            command,
        } => cmd_run(
            &root, &name, &ttl, &scope, &env, session, &hard_ttl, &command,
        ),
        Cmd::Revoke {
            all,
            secret,
            ticket,
        } => {
            client::ensure_running(&root)?;
            let data = client::request(
                &root,
                "revoke",
                json!({ "all": all, "secret": secret, "ticket": ticket }),
            )?;
            print_json(&data)
        }
        Cmd::Rotate { secret, json } => {
            client::ensure_running(&root)?;
            let data = client::request(&root, "rotate", json!({ "secret": secret }))?;
            if json {
                print_json(&data)?;
            } else {
                println!(
                    "rotated {}; new value (shown once): {}",
                    data.get("secret").and_then(Value::as_str).unwrap_or("-"),
                    data.get("new_value").and_then(Value::as_str).unwrap_or("-")
                );
            }
            Ok(())
        }
        Cmd::Status { json } => {
            client::ensure_running(&root)?;
            let data = client::request(&root, "status", json!({}))?;
            if json {
                print_json(&data)
            } else {
                print_status(&data);
                Ok(())
            }
        }
        Cmd::Audit { tail, json } => {
            client::ensure_running(&root)?;
            let data = client::request(&root, "audit_tail", json!({ "limit": tail }))?;
            if json {
                print_json(&data)
            } else {
                let events = data.as_array().cloned().unwrap_or_default();
                for event in events {
                    println!(
                        "[{}] {:<18} actor={:<14} secret={:<18} ticket={:<38} {}",
                        event.get("ts").and_then(Value::as_i64).unwrap_or(0),
                        event.get("event").and_then(Value::as_str).unwrap_or("-"),
                        event.get("actor").and_then(Value::as_str).unwrap_or("-"),
                        event.get("secret").and_then(Value::as_str).unwrap_or("-"),
                        event.get("ticket").and_then(Value::as_str).unwrap_or("-"),
                        event.get("result").and_then(Value::as_str).unwrap_or("-"),
                    );
                }
                Ok(())
            }
        }
        Cmd::Honeypot { cmd } => cmd_honeypot(&root, cmd),
        Cmd::Env { cmd } => match cmd {
            EnvCmd::Show => envcmd::show(&root),
            EnvCmd::Install {
                scope,
                no_path,
                dry_run,
            } => envcmd::install(&root, &scope, !no_path, dry_run),
            EnvCmd::Uninstall { scope, dry_run } => envcmd::uninstall(&scope, dry_run),
        },
        Cmd::Doctor => cmd_doctor(&root),
    }
}

fn cmd_init(
    root: &Root,
    agent: &str,
    path: Option<PathBuf>,
    dry_run: bool,
    refresh: bool,
    rebind: bool,
    list: bool,
    migrate_env: &[String],
    keep_env: &[String],
    no_env: bool,
) -> Result<()> {
    let explicit = if agent.eq_ignore_ascii_case("auto") {
        None
    } else {
        match AgentKind::parse(agent) {
            Some(kind) => Some(kind),
            None => {
                return Err(agentvault_core::Error::Other(format!(
                    "unknown agent `{agent}`; use claude|cursor|codex|gemini|windsurf|cline|qoder|generic|auto"
                )))
            }
        }
    };

    if list {
        list_candidates(explicit);
        return Ok(());
    }

    if rebind {
        let mut cfg = store::load_config(root)?;
        cfg.rebind();
        store::save_config(root, &cfg)?;
        println!("device fingerprint rebound to this machine");
    } else {
        let _ = store::ensure_config(root)?;
    }

    let mut policy = agentvault_adapters::Policy::default();
    for key in migrate_env {
        policy.force.insert(key.clone());
    }
    for key in keep_env {
        policy.skip.insert(key.clone());
    }

    let target = match path {
        Some(path) => {
            let path = agentvault_core::expand_path(&path.to_string_lossy());
            if !path.exists() {
                return Err(agentvault_core::Error::Other(format!(
                    "config path does not exist: {}",
                    path.display()
                )));
            }
            path
        }
        None => match explicit {
            Some(kind) => agentvault_adapters::first_existing(kind).unwrap_or_else(|| {
                std::env::current_dir()
                    .unwrap_or_default()
                    .join(".mcp.json")
            }),
            None => detect_config().map(|(_, path)| path).unwrap_or_else(|| {
                std::env::current_dir()
                    .unwrap_or_default()
                    .join(".mcp.json")
            }),
        },
    };
    let resolved = agentvault_adapters::resolve_target(&target);
    if resolved != target {
        println!("resolved:     {}", resolved.display());
    }
    let target = resolved;

    let toml = target.extension().and_then(|e| e.to_str()) == Some("toml");
    let inferred = if toml {
        AgentKind::Codex
    } else {
        agentvault_adapters::infer_kind_for_path(&target).unwrap_or(AgentKind::Generic)
    };
    let kind = explicit.unwrap_or(inferred);
    let kind = if toml { AgentKind::Codex } else { kind };

    println!("agent:        {}", kind.as_str());
    println!("config:       {}", target.display());

    if !dry_run {
        maybe_persist_env(root, no_env);
    }

    if !target.exists() {
        println!("config does not exist; nothing to migrate (create it first, or pass --path)");
        println!("run `agentvault init --list` to see every candidate path");
        return Ok(());
    }

    if toml {
        return cmd_init_codex(root, kind, &target, dry_run, refresh, &policy);
    }

    let mut mcp = agentvault_adapters::load_any(&target)?;
    if refresh {
        let launcher = launcher_path();
        let mut preview = mcp.clone();
        let changes = agentvault_adapters::refresh_with_launcher_and(
            &mut preview,
            &launcher,
            Some(root.dir.as_path()),
        );
        if changes.is_empty() {
            println!("no wrapped servers need refreshing");
            return Ok(());
        }
        println!("would refresh {} wrapped server(s):", changes.len());
        for change in &changes {
            println!(
                "  - {}: {} -> {}",
                change.server, change.before, change.after
            );
            if !change.remaining_env.is_empty() {
                println!("    unchanged env: {}", change.remaining_env.join(", "));
            }
        }
        if dry_run {
            println!("dry run: no files were changed");
            return Ok(());
        }

        client::ensure_running(root)?;
        let backup = agentvault_adapters::backup_file(&target)?;
        let changes = agentvault_adapters::refresh_with_launcher_and(
            &mut mcp,
            &launcher,
            Some(root.dir.as_path()),
        );
        mcp.save(&target)?;
        let names: Vec<String> = changes.iter().map(|change| change.server.clone()).collect();
        if let Err(error) = agentvault_adapters::verify_wrapped(&target, &names) {
            let _ = agentvault_adapters::restore_file(&backup, &target);
            return Err(error);
        }
        println!("wrote:        {}", target.display());
        println!("backup:       {}", backup.display());
        println!("refreshed {} wrapped server(s)", changes.len());
        return Ok(());
    }

    let plan = agentvault_adapters::plan_with(&mcp, &policy);

    if plan.changes.is_empty() {
        println!("no mcpServers to wrap ({} skipped)", plan.skipped.len());
        for item in &plan.skipped {
            println!("  skip: {item}");
        }
        return Ok(());
    }

    println!("would wrap {} server(s):", plan.changes.len());
    for change in &plan.changes {
        println!(
            "  - {}: {} -> agentvault run --name {}",
            change.server, change.before, change.server
        );
        if !change.migrated_env.is_empty() {
            println!("    migrate env: {}", change.migrated_env.join(", "));
        }
        if !change.kept_env.is_empty() {
            println!(
                "    keep env:    {} (reference/override)",
                change.kept_env.join(", ")
            );
        }
        if !change.remaining_env.is_empty() {
            println!("    unchanged env: {}", change.remaining_env.join(", "));
        }
    }
    for item in &plan.skipped {
        println!("  skip: {item}");
    }
    if dry_run {
        println!("dry run: no files were changed");
        return Ok(());
    }

    client::ensure_running(root)?;
    let backup = agentvault_adapters::backup_file(&target)?;

    // Move secret-looking env values into the keystore before stripping them.
    // Only servers that will actually be rewritten: URL-only and already
    // wrapped servers must not have their env touched.
    let planned: std::collections::BTreeSet<&str> = plan
        .changes
        .iter()
        .map(|change| change.server.as_str())
        .collect();
    let mut jobs: Vec<(String, String)> = Vec::new();
    for (server_name, server) in &mcp.mcp_servers {
        if !planned.contains(server_name.as_str()) {
            continue;
        }
        for (key, value) in &server.env {
            if agentvault_adapters::should_migrate_env(key, value, &policy) {
                jobs.push((format!("{server_name}/{key}"), value.clone()));
            }
        }
    }
    let mut broker = client::BrokerClient::connect(root)?;
    for (secret_name, value) in &jobs {
        let stored = broker.call(
            "secret_add",
            json!({ "name": secret_name, "value": value, "kind": "env" }),
        )?;
        if stored
            .get("value_changed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            eprintln!(
                "warning: `{secret_name}` already existed with a different value; it was overwritten"
            );
        }
        println!("migrated {secret_name} into the keystore");
    }

    let launcher = launcher_path();
    let changes = agentvault_adapters::apply_with_launcher_and(
        &mut mcp,
        &launcher,
        Some(root.dir.as_path()),
        &policy,
    );
    mcp.save(&target)?;

    let server_names: Vec<String> = changes.iter().map(|change| change.server.clone()).collect();
    if let Err(error) = agentvault_adapters::verify_wrapped(&target, &server_names) {
        let _ = agentvault_adapters::restore_file(&backup, &target);
        return Err(error);
    }
    println!("wrote:        {}", target.display());
    println!("backup:       {}", backup.display());

    let path_str = target.display().to_string();
    broker.call(
        "bind_agent",
        json!({
            "agent": kind.as_str(),
            "config_path": path_str,
            "servers": changes.iter().map(|c| c.server.clone()).collect::<Vec<_>>(),
        }),
    )?;

    println!(
        "wrapped {} server(s); long-lived values now live in the keystore",
        changes.len()
    );
    Ok(())
}

fn maybe_persist_env(root: &Root, no_env: bool) {
    if no_env || root.source != RootSource::Default {
        return;
    }
    if let Err(error) = envcmd::install(root, "user", true, false) {
        eprintln!("warning: could not persist AGENTVAULT_HOME/PATH: {error}");
    }
}

fn launcher_path() -> String {
    std::env::current_exe()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "agentvault".to_string())
}

fn cmd_init_codex(
    root: &Root,
    kind: AgentKind,
    target: &std::path::Path,
    dry_run: bool,
    refresh: bool,
    policy: &agentvault_adapters::Policy,
) -> Result<()> {
    let mut codex = agentvault_adapters::codex::CodexConfig::load(target)?;
    if refresh {
        let launcher = launcher_path();
        let mut preview = codex.clone();
        let changes = preview.refresh_launcher(&launcher, Some(root.dir.as_path()));
        if changes.is_empty() {
            println!("no wrapped Codex servers need refreshing");
            return Ok(());
        }
        println!("would refresh {} wrapped server(s):", changes.len());
        for change in &changes {
            println!("  - {}: {} -> refreshed", change.server, change.before);
            if !change.remaining_env.is_empty() {
                println!("    unchanged env: {}", change.remaining_env.join(", "));
            }
        }
        if dry_run {
            println!("dry run: no files were changed");
            return Ok(());
        }

        client::ensure_running(root)?;
        let backup = agentvault_adapters::backup_file(target)?;
        let changes = codex.refresh_launcher(&launcher, Some(root.dir.as_path()));
        codex.save(target)?;
        let reloaded = agentvault_adapters::codex::CodexConfig::load(target)?;
        let failed = changes.iter().any(|change| {
            reloaded
                .servers
                .iter()
                .find(|server| server.name == change.server)
                .map(|server| {
                    server.command.as_deref() != Some(launcher.as_str())
                        || !agentvault_adapters::codex::CodexConfig::is_wrapped(server)
                })
                .unwrap_or(true)
        });
        if failed {
            let _ = agentvault_adapters::restore_file(&backup, target);
            return Err(agentvault_core::Error::Other(
                "Codex config did not contain refreshed wrapped servers".into(),
            ));
        }
        println!("wrote:        {}", target.display());
        println!("backup:       {}", backup.display());
        println!(
            "refreshed {} wrapped server(s) in the Codex config",
            changes.len()
        );
        return Ok(());
    }
    let plan = codex.plan_with(policy);
    let skipped: Vec<String> = codex
        .servers
        .iter()
        .filter_map(|server| {
            if server.command.is_none() {
                Some(format!("{} (url-based, no command)", server.name))
            } else if agentvault_adapters::codex::CodexConfig::is_wrapped(server) {
                Some(format!("{} (already wrapped)", server.name))
            } else {
                None
            }
        })
        .collect();

    if plan.is_empty() {
        println!(
            "no `[mcp_servers.*]` entries to wrap ({} server(s) found)",
            codex.servers.len()
        );
        for item in &skipped {
            println!("  skip: {item}");
        }
        return Ok(());
    }

    println!("would wrap {} server(s):", plan.len());
    for change in &plan {
        println!(
            "  - {}: {} -> agentvault run --name {}",
            change.server, change.before, change.server
        );
        if !change.migrated_env.is_empty() {
            println!(
                "    migrate env: {}",
                change
                    .migrated_env
                    .iter()
                    .map(|(k, _)| k.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !change.remaining_env.is_empty() {
            println!("    unchanged env: {}", change.remaining_env.join(", "));
        }
    }
    for item in &skipped {
        println!("  skip: {item}");
    }
    if dry_run {
        println!("dry run: no files were changed");
        return Ok(());
    }

    client::ensure_running(root)?;
    let backup = agentvault_adapters::backup_file(target)?;
    let mut broker = client::BrokerClient::connect(root)?;
    for change in &plan {
        for (key, value) in &change.migrated_env {
            let secret_name = format!("{}/{}", change.server, key);
            let stored = broker.call(
                "secret_add",
                json!({ "name": secret_name, "value": value, "kind": "env" }),
            )?;
            if stored
                .get("value_changed")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                eprintln!(
                    "warning: `{secret_name}` already existed with a different value; it was overwritten"
                );
            }
            println!("migrated {secret_name} into the keystore");
        }
    }

    let launcher = launcher_path();
    let changes = codex.apply_with_launcher_and(&launcher, Some(root.dir.as_path()), policy);
    codex.save(target)?;

    let reloaded = agentvault_adapters::codex::CodexConfig::load(target)?;
    let failed = changes.iter().any(|change| {
        reloaded
            .servers
            .iter()
            .find(|server| server.name == change.server)
            .map(|server| !agentvault_adapters::codex::CodexConfig::is_wrapped(server))
            .unwrap_or(true)
    });
    if failed {
        let _ = agentvault_adapters::restore_file(&backup, target);
        return Err(agentvault_core::Error::Other(
            "Codex config did not contain wrapped servers after rewrite".into(),
        ));
    }
    println!("wrote:        {}", target.display());
    println!("backup:       {}", backup.display());

    let path_str = target.display().to_string();
    broker.call(
        "bind_agent",
        json!({
            "agent": kind.as_str(),
            "config_path": path_str,
            "servers": changes.iter().map(|c| c.server.clone()).collect::<Vec<_>>(),
        }),
    )?;

    println!("wrapped {} server(s) in the Codex config", changes.len());
    Ok(())
}

fn all_kinds() -> Vec<AgentKind> {
    vec![
        AgentKind::Claude,
        AgentKind::Cursor,
        AgentKind::Codex,
        AgentKind::Gemini,
        AgentKind::Windsurf,
        AgentKind::Cline,
        AgentKind::Qoder,
    ]
}

fn list_candidates(explicit: Option<AgentKind>) {
    let kinds = match explicit {
        Some(kind) => vec![kind],
        None => all_kinds(),
    };
    for kind in &kinds {
        println!("{}:", kind.as_str());
        for path in agentvault_adapters::candidate_paths(*kind) {
            let summary = if path.exists() {
                describe_config(&path)
            } else {
                "missing".to_string()
            };
            println!(
                "  [{}] {} ({})",
                if path.exists() { "x" } else { " " },
                path.display(),
                summary
            );
        }
    }
    if explicit.is_none() {
        match detect_config() {
            Some((kind, path)) => {
                println!("\nauto would pick: {} ({})", kind.as_str(), path.display())
            }
            None => println!("\nauto would pick: no existing config"),
        }
    }
}

fn describe_config(path: &std::path::Path) -> String {
    if path.extension().and_then(|e| e.to_str()) == Some("toml") {
        return match agentvault_adapters::codex::CodexConfig::load(path) {
            Ok(config) => format!(
                "{} server(s), {} to wrap",
                config.servers.len(),
                config.plan().len()
            ),
            Err(error) => format!("error: {error}"),
        };
    }
    match agentvault_adapters::McpConfig::load(path) {
        Ok(config) => {
            let wrapped = config
                .mcp_servers
                .values()
                .filter(|server| agentvault_adapters::is_already_wrapped(server))
                .count();
            format!(
                "{} server(s), {} to wrap, {} wrapped",
                config.mcp_servers.len(),
                agentvault_adapters::plan(&config).changes.len(),
                wrapped
            )
        }
        Err(error) => format!("error: {error}"),
    }
}

fn config_has_servers(path: &std::path::Path) -> bool {
    if path.extension().and_then(|e| e.to_str()) == Some("toml") {
        return agentvault_adapters::codex::CodexConfig::load(path)
            .map(|config| !config.servers.is_empty())
            .unwrap_or(false);
    }
    agentvault_adapters::McpConfig::load(path)
        .map(|config| !config.mcp_servers.is_empty())
        .unwrap_or(false)
}

/// Prefer a project-local config that actually contains servers, then any
/// config with servers, then the first config that exists. This keeps a global
/// Codex config from shadowing a project's `.qoder/settings.json`.
fn detect_config() -> Option<(AgentKind, PathBuf)> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut project = None;
    let mut user = None;
    let mut first = None;
    for kind in all_kinds() {
        for path in agentvault_adapters::candidate_paths(kind) {
            if !path.exists() {
                continue;
            }
            if first.is_none() {
                first = Some((kind, path.clone()));
            }
            if !config_has_servers(&path) {
                continue;
            }
            if path.starts_with(&cwd) {
                if project.is_none() {
                    project = Some((kind, path));
                }
            } else if user.is_none() {
                user = Some((kind, path));
            }
        }
    }
    project.or(user).or(first)
}

fn cmd_doctor(root: &Root) -> Result<()> {
    println!("home:      {}", root.dir.display());
    println!("home from: {}", root.source.as_str());
    println!(
        "default:   {}",
        agentvault_core::default_data_dir().display()
    );
    let probe = root.dir.join(".write-probe");
    let writable = match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    };
    println!("writable:  {}", if writable { "yes" } else { "no" });
    println!("exe:       {}", launcher_path());

    if root.config().exists() {
        match store::load_config(root) {
            Ok(config) => {
                let device_ok = config.device_fingerprint == agentvault_core::device_fingerprint();
                println!(
                    "config:    exists  device={} ({})",
                    config
                        .device_fingerprint
                        .chars()
                        .take(8)
                        .collect::<String>(),
                    if device_ok {
                        "match"
                    } else {
                        "MISMATCH; run init --rebind"
                    }
                );
                if let Some(expected) = &config.hardware_fp {
                    let actual = agentvault_core::hardware_fingerprint();
                    println!(
                        "hardware:  {} ({})",
                        expected.chars().take(8).collect::<String>(),
                        if expected == &actual {
                            "match"
                        } else {
                            "MISMATCH; run init --rebind"
                        }
                    );
                } else {
                    println!("hardware:  not bound (init --rebind records it)");
                }
                println!("agents:    {} bound config(s)", config.agents.len());
            }
            Err(error) => println!("config:    exists but cannot be read: {error}"),
        }
    } else {
        println!("config:    missing (not created yet)");
    }

    match client::ping(root) {
        Ok(value) => println!(
            "broker:    online (pid {}, port {})",
            value
                .get("data")
                .and_then(|d| d.get("pid"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
            std::fs::read_to_string(root.broker_port())
                .unwrap_or_default()
                .trim()
        ),
        Err(_) => {
            let stale = root.broker_port().exists() || root.pid().exists();
            println!(
                "broker:    offline{}",
                if stale { " (stale port/pid file)" } else { "" }
            );
        }
    }
    let keystore = agentvault_keystore::Keystore::open(&root.dir);
    println!("keystore:  {}", keystore.backend_name());
    envcmd::diagnostics(root);

    let mut seen = std::collections::BTreeSet::new();
    for kind in all_kinds() {
        for path in agentvault_adapters::candidate_paths(kind) {
            if path.exists() {
                println!(
                    "[x] {:<8} {} ({})",
                    kind.as_str(),
                    path.display(),
                    describe_config(&path)
                );
                seen.insert(path);
            }
        }
    }
    for path in seen {
        warn_missing_launcher(&path);
    }
    Ok(())
}

fn warn_missing_launcher(path: &std::path::Path) {
    let commands: Vec<String> = if path.extension().and_then(|e| e.to_str()) == Some("toml") {
        match agentvault_adapters::codex::CodexConfig::load(path) {
            Ok(config) => config
                .servers
                .iter()
                .filter(|server| agentvault_adapters::codex::CodexConfig::is_wrapped(server))
                .filter_map(|server| server.command.clone())
                .collect(),
            Err(_) => return,
        }
    } else {
        match agentvault_adapters::McpConfig::load(path) {
            Ok(config) => config
                .mcp_servers
                .values()
                .filter(|server| agentvault_adapters::is_already_wrapped(server))
                .filter_map(|server| server.command.clone())
                .collect(),
            Err(_) => return,
        }
    };
    for command in commands {
        let launcher = std::path::Path::new(&command);
        if launcher.is_absolute() && !launcher.exists() {
            println!(
                "[!] wrapped launcher is missing: {} (in {})",
                command,
                path.display()
            );
        } else if is_debug_launcher(&command) {
            println!(
                "[!] wrapped launcher is a debug build: {} (in {}); \
                 run `agentvault init --refresh` from the release binary",
                command,
                path.display()
            );
        }
    }
}

fn is_debug_launcher(command: &str) -> bool {
    command
        .replace('\\', "/")
        .to_ascii_lowercase()
        .contains("/target/debug/")
}

fn cmd_daemon(root: &Root, detach: bool) -> Result<()> {
    if detach {
        client::start_detached(root)?;
        println!(
            "broker ready on 127.0.0.1:{} (state: {})",
            std::fs::read_to_string(root.broker_port())
                .unwrap_or_default()
                .trim(),
            root.dir.display()
        );
        return Ok(());
    }
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let state = root.dir.display().to_string();
    std::thread::spawn(move || {
        if let Ok(port) = ready_rx.recv() {
            println!(
                "agentvault broker listening on 127.0.0.1:{port} (state: {state}). Leave this running."
            );
        }
    });
    let result = server::serve(root.clone(), Some(ready_tx));
    if let Err(e) = &result {
        let _ = std::fs::write(root.dir.join("broker.error.log"), format!("{e}\n"));
    }
    result
}

fn cmd_secret(root: &Root, cmd: SecretCmd) -> Result<()> {
    match cmd {
        SecretCmd::Add {
            name,
            value,
            stdin,
            kind,
            honeypot,
        } => {
            let value = match (value, stdin) {
                (Some(_), true) => {
                    return Err(agentvault_core::Error::Other(
                        "use either --value or --stdin, not both".into(),
                    ))
                }
                (Some(value), false) => value,
                (None, true) => {
                    let mut value = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut value)?;
                    value.trim_end_matches(['\r', '\n']).to_string()
                }
                (None, false) => {
                    print!("value for {name}: ");
                    std::io::stdout().flush()?;
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    line.trim_end().to_string()
                }
            };
            if value.is_empty() {
                return Err(agentvault_core::Error::Other("empty secret value".into()));
            }
            client::ensure_running(root)?;
            let data = client::request(
                root,
                "secret_add",
                json!({ "name": name, "value": value, "kind": kind, "honeypot": honeypot }),
            )?;
            print_json(&data)
        }
        SecretCmd::List => {
            client::ensure_running(root)?;
            let data = client::request(root, "status", json!({}))?;
            let secrets = data
                .get("secrets")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if secrets.is_empty() {
                println!("no secrets stored");
            }
            for secret in secrets {
                println!(
                    "{:<28} kind={:<6} epoch={:<3} honeypot={} revoked={}",
                    secret.get("name").and_then(Value::as_str).unwrap_or("-"),
                    secret.get("kind").and_then(Value::as_str).unwrap_or("-"),
                    secret.get("epoch").and_then(Value::as_u64).unwrap_or(0),
                    secret
                        .get("honeypot")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    secret
                        .get("revoked")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                );
            }
            Ok(())
        }
        SecretCmd::Remove { name } => {
            client::ensure_running(root)?;
            let data = client::request(root, "secret_remove", json!({ "name": name }))?;
            print_json(&data)
        }
        SecretCmd::Unrevoke { name } => {
            client::ensure_running(root)?;
            let data = client::request(root, "secret_unrevoke", json!({ "name": name }))?;
            print_json(&data)
        }
    }
}

fn cmd_run(
    root: &Root,
    name: &str,
    ttl: &str,
    scope: &str,
    env_specs: &[String],
    session: bool,
    hard_ttl: &str,
    command: &[String],
) -> Result<()> {
    client::ensure_running(root)?;

    let status = client::request(root, "status", json!({}))?;
    let known: Vec<String> = status
        .get("secrets")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|s| s.get("name").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let mut envs: Vec<(String, String)> = Vec::new();

    // `--env KEY=secret:NAME` resolves the value immediately for the child.
    for spec in env_specs {
        let (key, target) = spec.split_once('=').ok_or_else(|| {
            agentvault_core::Error::Other(format!("bad --env `{spec}`, expected KEY=secret:NAME"))
        })?;
        let secret_name = target.strip_prefix("secret:").ok_or_else(|| {
            agentvault_core::Error::Other(format!("bad --env `{spec}`, expected secret:NAME"))
        })?;
        let issued = client::request(
            root,
            "issue",
            json!({ "secret": secret_name, "sub": name, "ttl": ttl, "scope": scope, "max_uses": 1 }),
        )?;
        let ticket = issued
            .get("ticket")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let resolved = client::request(root, "resolve", json!({ "ticket": ticket }))?;
        let value = resolved
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        envs.push((key.to_string(), value));
    }

    // The server itself gets a ticket (not the secret) so it can call `resolve`.
    let mut ticket_for_child: Option<String> = None;
    let mut parent_ticket: Option<String> = None;
    let mut parent_file: Option<PathBuf> = None;
    let mut parent_jti: Option<String> = None;

    if known.iter().any(|s| s == name) {
        if session {
            let issued = client::request(
                root,
                "issue",
                json!({
                    "secret": name,
                    "sub": name,
                    "ttl": ttl,
                    "scope": scope,
                    "task": true,
                    "hard_ttl": hard_ttl,
                }),
            )?;
            let parent = issued
                .get("ticket")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            parent_jti = issued
                .get("jti")
                .and_then(Value::as_str)
                .map(str::to_string);
            let file = root
                .dir
                .join(format!("session-{}.ticket", std::process::id()));
            agentvault_keystore::write_private(&file, &parent)?;
            parent_ticket = Some(parent.clone());
            parent_file = Some(file.clone());

            // Keep the task ticket alive for the whole child process lifetime.
            // Each renewal is capped by the ticket's hard limit.
            let root_bg = root.clone();
            let file_bg = file;
            let current = Arc::new(Mutex::new(parent));
            let stop = Arc::new(AtomicBool::new(false));
            let stop_bg = Arc::clone(&stop);
            let ttl_owned = ttl.to_string();
            let ttl_secs = agentvault_core::parse_duration(ttl).unwrap_or(900).max(1);
            let interval = Duration::from_secs((ttl_secs / 3).clamp(1, 60));
            std::thread::spawn(move || {
                while !stop_bg.load(Ordering::Relaxed) {
                    std::thread::sleep(interval);
                    if stop_bg.load(Ordering::Relaxed) {
                        break;
                    }
                    let ticket = current.lock().expect("parent mutex").clone();
                    match client::request(
                        &root_bg,
                        "renew",
                        json!({ "ticket": ticket, "ttl": ttl_owned.clone() }),
                    ) {
                        Ok(data) => {
                            if let Some(renewed) = data.get("ticket").and_then(Value::as_str) {
                                *current.lock().expect("parent mutex") = renewed.to_string();
                                let _ = agentvault_keystore::write_private(&file_bg, renewed);
                            }
                        }
                        Err(error) if error.to_string().contains("hard limit") => break,
                        Err(_) => {}
                    }
                }
            });
        } else {
            let issued = client::request(
                root,
                "issue",
                json!({ "secret": name, "sub": name, "ttl": ttl, "scope": scope, "max_uses": 1 }),
            )?;
            ticket_for_child = issued
                .get("ticket")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
    } else if env_specs.is_empty() {
        eprintln!(
            "warning: no secret named `{name}`; starting without a ticket. \
             Run `agentvault secret list` or `agentvault init` first."
        );
    }

    let mut cmd = std::process::Command::new(&command[0]);
    cmd.args(&command[1..]);
    // Never leak a ticket inherited from an outer wrapped process.
    cmd.env_remove("AGENTVAULT_TICKET")
        .env_remove("AGENTVAULT_PARENT_TICKET")
        .env_remove("AGENTVAULT_PARENT_FILE");
    for (key, value) in &envs {
        cmd.env(key, value);
    }
    if let Some(ticket) = &ticket_for_child {
        cmd.env("AGENTVAULT_TICKET", ticket);
    }
    if let Some(ticket) = &parent_ticket {
        cmd.env("AGENTVAULT_PARENT_TICKET", ticket);
    }
    if let Some(file) = &parent_file {
        cmd.env("AGENTVAULT_PARENT_FILE", file);
    }
    cmd.env("AGENTVAULT_HOME", &root.dir);

    let status = cmd.status().map_err(|e| {
        agentvault_core::Error::Other(format!("cannot start `{}`: {e}", command[0]))
    })?;
    let code = status.code().unwrap_or(1);
    if let Some(file) = &parent_file {
        let _ = std::fs::remove_file(file);
    }
    if let Some(jti) = &parent_jti {
        let _ = client::request(root, "revoke", json!({ "ticket": jti }));
    }
    std::process::exit(code);
}

fn cmd_honeypot(root: &Root, cmd: HoneypotCmd) -> Result<()> {
    client::ensure_running(root)?;
    match cmd {
        HoneypotCmd::Add { name, json } => {
            let data = client::request(root, "honeypot_add", json!({ "name": name }))?;
            if json {
                return print_json(&data);
            }
            println!(
                "canary {}: {}",
                data.get("honeypot").and_then(Value::as_str).unwrap_or("-"),
                data.get("token").and_then(Value::as_str).unwrap_or("-")
            );
            println!(
                "{}",
                data.get("usage")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            );
            Ok(())
        }
        HoneypotCmd::List => {
            let data = client::request(root, "honeypot_list", json!({}))?;
            let items = data.as_array().cloned().unwrap_or_default();
            if items.is_empty() {
                println!("no honeypots");
            }
            for item in items {
                println!(
                    "{:<20} hits={} triggered={}",
                    item.get("name").and_then(Value::as_str).unwrap_or("-"),
                    item.get("hits").and_then(Value::as_u64).unwrap_or(0),
                    item.get("triggered_at").and_then(Value::as_i64).is_some()
                );
            }
            Ok(())
        }
        HoneypotCmd::Check {
            token,
            json: as_json,
        } => {
            let data = client::request(root, "honeypot_check", json!({ "token": token }))?;
            if as_json {
                print_json(&data)
            } else {
                println!(
                    "hit={} honeypot={} revoked_all={}",
                    data.get("hit").and_then(Value::as_bool).unwrap_or(false),
                    data.get("honeypot").and_then(Value::as_str).unwrap_or("-"),
                    data.get("revoked_all")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                );
                Ok(())
            }
        }
    }
}

fn print_status(data: &Value) {
    println!(
        "home:    {}",
        data.get("home").and_then(Value::as_str).unwrap_or("-")
    );
    println!(
        "backend: {}  device: {}  hardware: {}  epoch: {}",
        data.get("backend").and_then(Value::as_str).unwrap_or("-"),
        data.get("device").and_then(Value::as_str).unwrap_or("-"),
        data.get("hardware").and_then(Value::as_str).unwrap_or("-"),
        data.get("global_epoch")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    );
    let secrets = data
        .get("secrets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!("secrets ({}):", secrets.len());
    for secret in secrets {
        println!(
            "  {:<28} kind={:<6} epoch={:<3} honeypot={} revoked={}",
            secret.get("name").and_then(Value::as_str).unwrap_or("-"),
            secret.get("kind").and_then(Value::as_str).unwrap_or("-"),
            secret.get("epoch").and_then(Value::as_u64).unwrap_or(0),
            secret
                .get("honeypot")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            secret
                .get("revoked")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        );
    }
    let agents = data
        .get("agents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!("bound agents ({}):", agents.len());
    for agent in agents {
        println!(
            "  {:<10} {} [{}]",
            agent.get("agent").and_then(Value::as_str).unwrap_or("-"),
            agent
                .get("config_path")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            agent
                .get("servers")
                .and_then(Value::as_array)
                .map(|servers| servers
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", "))
                .unwrap_or_default(),
        );
    }
    let tickets = data
        .get("tickets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!("live tickets ({}):", tickets.len());
    for ticket in tickets {
        println!(
            "  {:<38} kind={:<6} sub={:<14} secret={:<20} left={}s uses={} renewals={}",
            ticket.get("jti").and_then(Value::as_str).unwrap_or("-"),
            ticket.get("kind").and_then(Value::as_str).unwrap_or("-"),
            ticket.get("sub").and_then(Value::as_str).unwrap_or("-"),
            ticket.get("secret").and_then(Value::as_str).unwrap_or("-"),
            ticket
                .get("remaining_ms")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                / 1000,
            ticket.get("uses_left").and_then(Value::as_u64).unwrap_or(0),
            ticket.get("renewals").and_then(Value::as_u64).unwrap_or(0),
        );
    }
}

fn print_json(data: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(data)?);
    Ok(())
}
