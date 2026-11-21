use crate::Broker;
use agentvault_core::{Result, Root};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

const MAX_LINE: u64 = 1024 * 1024;

fn acquire_instance(root: &Root) -> Result<InstanceGuard> {
    let path = root.dir.join("broker.lock");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                let _ = writeln!(file, "{}", std::process::id());
                return Ok(InstanceGuard { path });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let recorded = std::fs::read_to_string(&path).unwrap_or_default();
                let recorded = recorded.trim().to_string();
                if !recorded.is_empty() && crate::client::pid_alive(&recorded) {
                    let port = std::fs::read_to_string(root.broker_port()).unwrap_or_default();
                    let port = port.trim().to_string();
                    if !port.is_empty() && ping_port(&port) {
                        return Err(agentvault_core::Error::Other(format!(
                            "broker already running (pid {recorded}, port {port}); run `agentvault stop` first"
                        )));
                    }
                    if std::time::Instant::now() > deadline {
                        return Err(agentvault_core::Error::Other(format!(
                            "broker is already starting (pid {recorded})"
                        )));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
                // The recorded process is gone; recover the stale lock.
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_file(root.broker_port());
                let _ = std::fs::remove_file(root.pid());
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn ping_port(port: &str) -> bool {
    let Ok(address) = format!("127.0.0.1:{port}").parse::<std::net::SocketAddr>() else {
        return false;
    };
    let Ok(mut stream) =
        TcpStream::connect_timeout(&address, std::time::Duration::from_millis(300))
    else {
        return false;
    };
    if stream
        .set_read_timeout(Some(std::time::Duration::from_millis(300)))
        .is_err()
    {
        return false;
    }
    if stream.write_all(b"{\"op\":\"ping\"}\n").is_err() {
        return false;
    }
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).is_ok()
        && serde_json::from_str::<Value>(&line)
            .map(|value| value.get("ok").and_then(Value::as_bool).unwrap_or(false))
            .unwrap_or(false)
}

struct InstanceGuard {
    path: std::path::PathBuf,
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn serve(root: Root, ready: Option<std::sync::mpsc::Sender<u16>>) -> Result<()> {
    let _instance = acquire_instance(&root)?;
    let broker = Arc::new(Broker::open(root.clone())?);
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();

    agentvault_keystore::write_private(&root.broker_port(), &port.to_string())?;
    let _ = std::fs::write(root.pid(), std::process::id().to_string());
    let _ = std::fs::remove_file(root.dir.join("broker.error.log"));

    if let Some(tx) = ready {
        let _ = tx.send(port);
    }

    {
        let sweeper = Arc::clone(&broker);
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            let _ = sweeper.sweep();
        });
    }

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let broker = Arc::clone(&broker);
                std::thread::spawn(move || {
                    let _ = handle(broker, stream);
                });
            }
            Err(_) => continue,
        }
    }
    Ok(())
}

fn handle(broker: Arc<Broker>, stream: TcpStream) -> Result<()> {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        if line.len() as u64 > MAX_LINE {
            return Err(agentvault_core::Error::Other(
                "request line too large".into(),
            ));
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => dispatch(&broker, request),
            Err(e) => err(format!("bad request: {e}")),
        };
        let mut body = serde_json::to_vec(&response)?;
        body.push(b'\n');
        writer.write_all(&body)?;
        writer.flush()?;
    }
}

fn dispatch(broker: &Broker, request: Value) -> Value {
    let op = request
        .get("op")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if op == "ping" {
        return ok(json!({ "pong": true, "backend": broker.backend(), "pid": std::process::id() }));
    }

    let provided = request
        .get("admin")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if op != "resolve" && provided != broker.admin_token() {
        return err("admin token missing or invalid".into());
    }

    let str_of = |key: &str| -> Option<String> {
        request.get(key).and_then(Value::as_str).map(str::to_string)
    };
    let u32_of =
        |key: &str| -> Option<u32> { request.get(key).and_then(Value::as_u64).map(|v| v as u32) };

    let result = match op {
        "shutdown" => {
            // Let the response reach the client before the process exits.
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(200));
                std::process::exit(0);
            });
            Ok(json!({ "stopping": true }))
        }
        "issue" => {
            let secret = str_of("secret").unwrap_or_default();
            let sub = str_of("sub").unwrap_or_else(|| "cli".into());
            let ttl = str_of("ttl");
            let scope = str_of("scope").unwrap_or_else(|| "read".into());
            let task = request
                .get("task")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let hard_ttl = str_of("hard_ttl");
            broker.issue(
                &secret,
                &sub,
                ttl.as_deref(),
                &scope,
                u32_of("max_uses"),
                task,
                hard_ttl.as_deref(),
            )
        }
        "child" => {
            let parent = str_of("parent").unwrap_or_default();
            let ttl = str_of("ttl");
            let sub = str_of("sub");
            broker.child(&parent, ttl.as_deref(), sub.as_deref(), u32_of("max_uses"))
        }
        "renew" => {
            let ticket = str_of("ticket").unwrap_or_default();
            let ttl = str_of("ttl");
            broker.renew(&ticket, ttl.as_deref())
        }
        "resolve" => {
            let ticket = str_of("ticket").unwrap_or_default();
            broker.resolve(&ticket)
        }
        "revoke" => {
            if request.get("all").and_then(Value::as_bool).unwrap_or(false) {
                broker.revoke_all()
            } else if let Some(secret) = str_of("secret") {
                broker.revoke_secret(&secret)
            } else if let Some(jti) = str_of("ticket") {
                broker.revoke_ticket(&jti)
            } else {
                return err("revoke needs --all, --secret or --ticket".into());
            }
        }
        "rotate" => {
            let secret = str_of("secret").unwrap_or_default();
            broker.rotate(&secret)
        }
        "bind_agent" => {
            let agent = str_of("agent").unwrap_or_default();
            let config_path = str_of("config_path").unwrap_or_default();
            let servers: Vec<String> = request
                .get("servers")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            broker.bind_agent(&agent, &config_path, servers)
        }
        "secret_add" => {
            let name = str_of("name").unwrap_or_default();
            let value = str_of("value").unwrap_or_default();
            let kind = match str_of("kind").as_deref() {
                Some("env") => agentvault_core::SecretKind::Env,
                Some("json") => agentvault_core::SecretKind::Json,
                _ => agentvault_core::SecretKind::Token,
            };
            let honeypot = request
                .get("honeypot")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            broker.add_secret(&name, &value, kind, honeypot)
        }
        "secret_remove" => {
            let name = str_of("name").unwrap_or_default();
            broker.remove_secret(&name)
        }
        "secret_unrevoke" => {
            let name = str_of("name").unwrap_or_default();
            broker.unrevoke_secret(&name)
        }
        "status" => broker.status(),
        "audit_tail" => {
            let limit = request.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
            broker.audit_tail(limit)
        }
        "honeypot_add" => {
            let name = str_of("name").unwrap_or_default();
            broker.honeypot_add(&name)
        }
        "honeypot_list" => broker.honeypot_list(),
        "honeypot_check" => {
            let token = str_of("token").unwrap_or_default();
            broker.honeypot_check(&token)
        }
        other => return err(format!("unknown op `{other}`")),
    };

    match result {
        Ok(data) => ok(data),
        Err(e) => err(e.to_string()),
    }
}

fn ok(data: Value) -> Value {
    json!({ "ok": true, "data": data })
}

fn err(message: String) -> Value {
    json!({ "ok": false, "error": message })
}
