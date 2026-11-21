//! Layer 4: append-only local audit trail. JSON lines are deliberately boring:
//! they survive crashes, can be tailed, and do not need a database daemon.

use agentvault_core::util::{atomic_write, now_ms};
use agentvault_core::{Result, Root};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

pub const EV_SECRET_ADD: &str = "secret.add";
pub const EV_SECRET_REMOVE: &str = "secret.remove";
pub const EV_SECRET_ROTATE: &str = "secret.rotate";
pub const EV_TICKET_ISSUE: &str = "ticket.issue";
pub const EV_TICKET_RESOLVE: &str = "ticket.resolve";
pub const EV_TICKET_REVOKE: &str = "ticket.revoke";
pub const EV_REVOKE_ALL: &str = "ticket.revoke_all";
pub const EV_HONEYPOT_TRIGGERED: &str = "honeypot.triggered";
pub const EV_DAEMON_START: &str = "daemon.start";
pub const EV_INIT: &str = "config.init";
pub const EV_DEVICE_MISMATCH: &str = "device.mismatch";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AuditEvent {
    pub ts: i64,
    pub event: String,
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    pub result: String,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub meta: serde_json::Value,
}

impl AuditEvent {
    pub fn new(event: &str, actor: impl Into<String>, result: impl Into<String>) -> Self {
        Self {
            ts: now_ms(),
            event: event.to_string(),
            actor: actor.into(),
            secret: None,
            ticket: None,
            result: result.into(),
            meta: serde_json::Value::Null,
        }
    }

    pub fn secret(mut self, name: impl Into<String>) -> Self {
        self.secret = Some(name.into());
        self
    }

    pub fn ticket(mut self, jti: impl Into<String>) -> Self {
        self.ticket = Some(jti.into());
        self
    }

    pub fn meta(mut self, value: serde_json::Value) -> Self {
        self.meta = value;
        self
    }
}

pub fn append(root: &Root, event: &AuditEvent) -> Result<()> {
    root.ensure()?;
    let path = root.audit_log();
    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(&line)?;
    file.flush()?;
    Ok(())
}

pub fn append_path(path: &Path, event: &AuditEvent) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(&line)?;
    Ok(())
}

pub fn tail(root: &Root, limit: usize) -> Result<Vec<AuditEvent>> {
    let path = root.audit_log();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read_to_string(&path)?;
    let events: Vec<AuditEvent> = data
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let start = events.len().saturating_sub(limit);
    Ok(events[start..].to_vec())
}

/// Rotate the audit file when it grows beyond `max_bytes`. The old file is
/// kept so evidence is never silently destroyed.
pub fn rotate_if_large(root: &Root, max_bytes: u64) -> Result<bool> {
    let path = root.audit_log();
    if !path.exists() {
        return Ok(false);
    }
    let size = std::fs::metadata(&path)?.len();
    if size <= max_bytes {
        return Ok(false);
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let archived = root.dir.join(format!("audit-{stamp}.jsonl"));
    std::fs::rename(&path, &archived)?;
    atomic_write(&path, b"")?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> Root {
        Root::new(std::env::temp_dir().join(format!("av-audit-{}", uuid::Uuid::new_v4())))
    }

    #[test]
    fn append_and_tail() {
        let root = temp_root();
        root.ensure().unwrap();
        for i in 0..3 {
            let event = AuditEvent::new(EV_TICKET_ISSUE, "test", "ok")
                .secret("db")
                .ticket(format!("jti-{i}"));
            append(&root, &event).unwrap();
        }
        let events = tail(&root, 2).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].ticket.as_deref(), Some("jti-2"));
        let _ = std::fs::remove_dir_all(&root.dir);
    }
}
