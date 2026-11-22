//! Layer 4: append-only local audit trail. JSON lines are deliberately boring:
//! they survive crashes, can be tailed, and do not need a database daemon.

use agentvault_core::store::FileLock;
use agentvault_core::util::{atomic_write, now_ms};
use agentvault_core::{Error, Result, Root};
use agentvault_keystore::Keystore;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

/// Keystore entry holding the 32-byte Ed25519 seed as hex.
pub const AUDIT_KEY: &str = "audit/signing";

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
    /// Hash of the previous line, forming a tamper-evident chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// Ed25519 signature over `sha256(prev_hash + payload)`, hex encoded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
    /// The verifying public key itself (hex), so a line is self-verifying.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pubkey_id: Option<String>,
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
            prev_hash: None,
            hash: None,
            sig: None,
            pubkey_id: None,
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
    append_path(&root.audit_log(), event)
}

pub fn append_path(path: &Path, event: &AuditEvent) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _lock = FileLock::acquire(&path.with_extension("lock"), Duration::from_secs(10))?;
    let (sealed, _) = seal(path, event)?;
    write_line(path, &sealed)
}

/// Append a line signed with the keystore's `audit/signing` Ed25519 key.
pub fn append_signed(root: &Root, event: &AuditEvent) -> Result<()> {
    root.ensure()?;
    let signing = signing_key(root)?;
    append_signed_path(&root.audit_log(), event, &signing)
}

pub fn append_signed_path(path: &Path, event: &AuditEvent, signing: &SigningKey) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _lock = FileLock::acquire(&path.with_extension("lock"), Duration::from_secs(10))?;
    let (mut sealed, digest) = seal(path, event)?;
    let signature = signing.sign(&digest);
    sealed.sig = Some(hex::encode(signature.to_bytes()));
    sealed.pubkey_id = Some(hex::encode(signing.verifying_key().to_bytes()));
    write_line(path, &sealed)
}

fn write_line(path: &Path, event: &AuditEvent) -> Result<()> {
    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(&line)?;
    file.flush()?;
    Ok(())
}

/// The hash covers the payload without signature fields, so old unsigned
/// lines verify exactly as before.
fn seal(path: &Path, event: &AuditEvent) -> Result<(AuditEvent, Vec<u8>)> {
    let previous = last_hash(path);
    let mut sealed = event.clone();
    sealed.prev_hash = previous.clone();
    sealed.hash = None;
    sealed.sig = None;
    sealed.pubkey_id = None;
    let payload = serde_json::to_vec(&sealed)?;
    let mut hasher = Sha256::new();
    hasher.update(previous.as_deref().unwrap_or("").as_bytes());
    hasher.update(&payload);
    let digest = hasher.finalize().to_vec();
    sealed.hash = Some(hex::encode(&digest));
    Ok((sealed, digest))
}

pub fn public_key_hex(root: &Root) -> Result<String> {
    Ok(hex::encode(signing_key(root)?.verifying_key().to_bytes()))
}

fn signing_key(root: &Root) -> Result<SigningKey> {
    let value = Keystore::open(&root.dir).get_or_create_secret(AUDIT_KEY, 32)?;
    let bytes = hex::decode(value.trim())
        .map_err(|_| Error::Other("audit signing key in keystore is not hex".into()))?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| Error::Other("audit signing key has wrong length".into()))?;
    Ok(SigningKey::from_bytes(&seed))
}

fn last_hash(path: &Path) -> Option<String> {
    let data = std::fs::read_to_string(path).ok()?;
    data.lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .find_map(|line| serde_json::from_str::<AuditEvent>(line).ok()?.hash)
}

#[derive(Debug, Default, Serialize)]
pub struct AuditVerify {
    pub checked: usize,
    pub legacy: usize,
    pub broken: Vec<String>,
}

pub fn verify(root: &Root) -> Result<AuditVerify> {
    verify_path(&root.audit_log())
}

pub fn verify_path(path: &Path) -> Result<AuditVerify> {
    let mut report = AuditVerify::default();
    if !path.exists() {
        return Ok(report);
    }
    let data = std::fs::read_to_string(path)?;
    let mut previous: Option<String> = None;
    for (index, line) in data.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<AuditEvent>(line) else {
            report
                .broken
                .push(format!("line {} is not valid JSON", index + 1));
            continue;
        };
        let Some(hash) = event.hash.clone() else {
            report.legacy += 1;
            continue;
        };
        if event.prev_hash != previous {
            report
                .broken
                .push(format!("line {} previous hash mismatch", index + 1));
        }
        let digest = digest_of(&event)?;
        if hex::encode(&digest) != hash {
            report
                .broken
                .push(format!("line {} hash mismatch", index + 1));
        }
        report.checked += 1;
        previous = Some(hash);
    }
    Ok(report)
}

#[derive(Debug, Default, Serialize)]
pub struct SignatureVerify {
    pub checked: usize,
    /// Lines written before the hash chain existed.
    pub legacy: usize,
    /// Hash-chained lines written before signing was introduced.
    pub unsigned: usize,
    pub broken: Vec<String>,
}

pub fn verify_signatures(root: &Root) -> Result<SignatureVerify> {
    verify_signatures_path(&root.audit_log())
}

pub fn verify_signatures_path(path: &Path) -> Result<SignatureVerify> {
    let mut report = SignatureVerify::default();
    if !path.exists() {
        return Ok(report);
    }
    let data = std::fs::read_to_string(path)?;
    let mut previous: Option<String> = None;
    for (index, line) in data.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<AuditEvent>(line) else {
            report
                .broken
                .push(format!("line {} is not valid JSON", index + 1));
            continue;
        };
        let Some(hash) = event.hash.clone() else {
            report.legacy += 1;
            continue;
        };
        if event.prev_hash != previous {
            report
                .broken
                .push(format!("line {} previous hash mismatch", index + 1));
        }
        let digest = digest_of(&event)?;
        if hex::encode(&digest) != hash {
            report
                .broken
                .push(format!("line {} hash mismatch", index + 1));
        }
        match (&event.sig, &event.pubkey_id) {
            (Some(sig), Some(pubkey_id)) => match verify_one(&digest, sig, pubkey_id) {
                Ok(()) => report.checked += 1,
                Err(reason) => report.broken.push(format!("line {} {reason}", index + 1)),
            },
            (None, _) => report.unsigned += 1,
            _ => report
                .broken
                .push(format!("line {} signature missing pubkey_id", index + 1)),
        }
        previous = Some(hash);
    }
    Ok(report)
}

fn digest_of(event: &AuditEvent) -> Result<Vec<u8>> {
    let mut copy = event.clone();
    copy.hash = None;
    copy.sig = None;
    copy.pubkey_id = None;
    let payload = serde_json::to_vec(&copy)?;
    let mut hasher = Sha256::new();
    hasher.update(event.prev_hash.as_deref().unwrap_or("").as_bytes());
    hasher.update(&payload);
    Ok(hasher.finalize().to_vec())
}

fn verify_one(digest: &[u8], sig_hex: &str, pubkey_hex: &str) -> std::result::Result<(), String> {
    let pubkey = hex::decode(pubkey_hex).map_err(|_| "pubkey_id is not hex".to_string())?;
    let pubkey: [u8; 32] = pubkey
        .try_into()
        .map_err(|_| "pubkey_id has wrong length".to_string())?;
    let key = VerifyingKey::from_bytes(&pubkey)
        .map_err(|_| "pubkey_id is not an ed25519 key".to_string())?;
    let signature = hex::decode(sig_hex).map_err(|_| "sig is not hex".to_string())?;
    let signature: [u8; 64] = signature
        .try_into()
        .map_err(|_| "sig has wrong length".to_string())?;
    key.verify_strict(digest, &Signature::from_bytes(&signature))
        .map_err(|_| "signature mismatch".to_string())
}

pub fn read_filtered(
    root: &Root,
    event: Option<&str>,
    since: Option<i64>,
    limit: usize,
) -> Result<Vec<AuditEvent>> {
    let path = root.audit_log();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read_to_string(path)?;
    let events: Vec<AuditEvent> = data
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<AuditEvent>(line).ok())
        .filter(|item: &AuditEvent| {
            event
                .map(|prefix| item.event.contains(prefix))
                .unwrap_or(true)
        })
        .filter(|item: &AuditEvent| since.map(|since| item.ts >= since).unwrap_or(true))
        .collect();
    let start = events.len().saturating_sub(limit);
    Ok(events[start..].to_vec())
}

pub fn export(root: &Root, target: &Path) -> Result<usize> {
    let path = root.audit_log();
    let data = if path.exists() {
        std::fs::read_to_string(path)?
    } else {
        String::new()
    };
    let count = data.lines().filter(|line| !line.trim().is_empty()).count();
    atomic_write(target, data.as_bytes())?;
    Ok(count)
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

    #[test]
    fn signed_lines_verify_and_detect_tampering() {
        let root = temp_root();
        root.ensure().unwrap();
        append_signed(
            &root,
            &AuditEvent::new(EV_SECRET_ADD, "test", "ok").secret("db"),
        )
        .unwrap();
        let report = verify_signatures(&root).unwrap();
        assert_eq!(report.checked, 1);
        assert!(report.broken.is_empty(), "{:?}", report.broken);

        let path = root.audit_log();
        let data = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            data.replace("\"result\":\"ok\"", "\"result\":\"denied\""),
        )
        .unwrap();
        let tampered = verify_signatures(&root).unwrap();
        assert!(!tampered.broken.is_empty());
        let _ = std::fs::remove_dir_all(&root.dir);
    }
}
