use agentvault_core::{Error, Result, Root};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub struct BrokerClient {
    root: Root,
    stream: TcpStream,
    admin: String,
}

impl BrokerClient {
    pub fn connect(root: &Root) -> Result<Self> {
        let port = read_port(root)?;
        let addr: SocketAddr = format!("127.0.0.1:{port}")
            .parse()
            .map_err(|_| Error::Other("invalid broker address".into()))?;
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
            .map_err(|e| Error::Other(format!("cannot reach broker on port {port}: {e}")))?;
        stream.set_read_timeout(Some(Duration::from_secs(15)))?;
        stream.set_write_timeout(Some(Duration::from_secs(15)))?;
        let admin = std::fs::read_to_string(root.admin_token()).unwrap_or_default();
        Ok(Self {
            root: root.clone(),
            stream,
            admin,
        })
    }

    pub fn call(&mut self, op: &str, mut params: Value) -> Result<Value> {
        if let Some(map) = params.as_object_mut() {
            map.insert("op".into(), json!(op));
            map.insert("admin".into(), json!(self.admin));
        } else {
            params = json!({ "op": op, "admin": self.admin });
        }
        let mut body = serde_json::to_vec(&params)?;
        body.push(b'\n');
        self.stream.write_all(&body)?;
        self.stream.flush()?;
        let mut line = String::new();
        BufReader::new(&self.stream).read_line(&mut line)?;
        if line.trim().is_empty() {
            return Err(Error::Other("empty response from broker".into()));
        }
        let value: Value = serde_json::from_str(&line)?;
        if value.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            Ok(value.get("data").cloned().unwrap_or(Value::Null))
        } else {
            Err(Error::Other(
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("broker error")
                    .to_string(),
            ))
        }
    }

    pub fn root(&self) -> &Root {
        &self.root
    }
}

pub fn request(root: &Root, op: &str, params: Value) -> Result<Value> {
    BrokerClient::connect(root)?.call(op, params)
}

fn read_port(root: &Root) -> Result<u16> {
    let port = std::fs::read_to_string(root.broker_port())
        .map_err(|_| Error::Other("broker is not running (no port file)".into()))?;
    port.trim()
        .parse()
        .map_err(|_| Error::Other("corrupt broker port file".into()))
}

/// Short, non-blocking readiness probe. Never waits 15 seconds on a half-open
/// socket.
pub fn ping(root: &Root) -> Result<Value> {
    let port = read_port(root)?;
    let addr: SocketAddr = format!("127.0.0.1:{port}")
        .parse()
        .map_err(|_| Error::Other("invalid broker address".into()))?;
    let budget = Duration::from_millis(600);
    let mut stream = TcpStream::connect_timeout(&addr, budget)?;
    stream.set_read_timeout(Some(budget))?;
    stream.set_write_timeout(Some(budget))?;
    stream.write_all(b"{\"op\":\"ping\"}\n")?;
    stream.flush()?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;
    let value: Value = serde_json::from_str(&line)?;
    if value.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        Ok(value)
    } else {
        Err(Error::Other("broker ping rejected".into()))
    }
}

/// Ensure a broker is reachable, starting a detached one when needed.
///
/// The child is spawned with null stdio so `$(agentvault ...)` still returns:
/// a broker that inherited the shell's stdout pipe used to keep command
/// substitution open on Windows.
pub fn ensure_running(root: &Root) -> Result<()> {
    if ping(root).is_ok() {
        return Ok(());
    }
    if std::env::var_os("AGENTVAULT_NO_AUTOSTART").is_some() {
        return Err(Error::Other(format!(
            "broker is not running and autostart is disabled (state: {})",
            root.dir.display()
        )));
    }
    start_detached(root)
}

pub fn start_detached(root: &Root) -> Result<()> {
    if ping(root).is_ok() {
        return Ok(());
    }
    let exe = std::env::current_exe()
        .map_err(|e| Error::Other(format!("cannot locate the agentvault binary: {e}")))?;
    let mut command = std::process::Command::new(exe);
    command
        .arg("--home")
        .arg(&root.dir)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NO_WINDOW: no console, no inherited pipes.
        command.creation_flags(0x0000_0008 | 0x0800_0000);
    }
    spawn_without_inheriting_stdio(&mut command)
        .map_err(|e| Error::Other(format!("cannot start the broker: {e}")))?;

    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if ping(root).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let log = std::fs::read_to_string(root.dir.join("broker.error.log"))
        .unwrap_or_default()
        .trim()
        .to_string();
    Err(Error::Other(if log.is_empty() {
        format!(
            "broker did not become ready within 15s (state: {})",
            root.dir.display()
        )
    } else {
        format!("broker failed to start: {log}")
    }))
}

/// Ask the broker to exit, then fall back to killing its PID.
pub fn stop(root: &Root) -> Result<bool> {
    let mut stopped = false;
    if read_port(root).is_ok() {
        if let Ok(mut client) = BrokerClient::connect(root) {
            if client.call("shutdown", serde_json::json!({})).is_ok() {
                stopped = true;
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                while std::time::Instant::now() < deadline {
                    if ping(root).is_err() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
    if !stopped {
        let pid = std::fs::read_to_string(root.pid()).unwrap_or_default();
        let pid = pid.trim().to_string();
        if !pid.is_empty() && pid_alive(&pid) {
            #[cfg(windows)]
            {
                stopped = std::process::Command::new("taskkill")
                    .args(["/PID", &pid, "/F"])
                    .output()
                    .map(|output| output.status.success())
                    .unwrap_or(false);
            }
            #[cfg(not(windows))]
            {
                stopped = std::process::Command::new("kill")
                    .arg(&pid)
                    .output()
                    .map(|output| output.status.success())
                    .unwrap_or(false);
            }
        }
    }
    let _ = std::fs::remove_file(root.broker_port());
    let _ = std::fs::remove_file(root.pid());
    let _ = std::fs::remove_file(root.dir.join("broker.lock"));
    Ok(stopped)
}

/// Windows `CreateProcess` is called with `bInheritHandles=TRUE` whenever the
/// child has explicit stdio handles. That leaks the parent's stdout pipe into
/// the long-running broker, so `$(agentvault status)` never sees EOF. Mark the
/// parent's standard handles non-inheritable for the duration of the spawn.
fn spawn_without_inheriting_stdio(
    command: &mut std::process::Command,
) -> std::io::Result<std::process::Child> {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        extern "system" {
            fn GetStdHandle(which: u32) -> *mut c_void;
            fn GetHandleInformation(handle: *mut c_void, flags: *mut u32) -> i32;
            fn SetHandleInformation(handle: *mut c_void, mask: u32, flags: u32) -> i32;
        }
        const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
        const STD_INPUT_HANDLE: u32 = -10i32 as u32;
        const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
        const STD_ERROR_HANDLE: u32 = -12i32 as u32;
        let mut saved = Vec::new();
        unsafe {
            for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
                let handle = GetStdHandle(which);
                if handle.is_null() || handle as isize == -1 {
                    continue;
                }
                let mut flags = 0u32;
                if GetHandleInformation(handle, &mut flags) != 0 && flags & HANDLE_FLAG_INHERIT != 0
                {
                    if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) != 0 {
                        saved.push(handle);
                    }
                }
            }
            let result = command.spawn();
            for handle in saved {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
            }
            return result;
        }
    }
    #[cfg(not(windows))]
    {
        command.spawn()
    }
}

pub fn pid_alive(pid: &str) -> bool {
    if pid.trim().is_empty() {
        return false;
    }
    #[cfg(windows)]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|output| {
                let text = String::from_utf8_lossy(&output.stdout);
                text.contains(pid.trim()) && text.to_ascii_lowercase().contains("agentvault")
            })
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}
