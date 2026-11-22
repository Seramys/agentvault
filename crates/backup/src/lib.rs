//! Encrypted config snapshots. The key lives in the OS keystore, so a backup
//! file copied off the machine is useless without the device-bound keystore.

use agentvault_core::store::FileLock;
use agentvault_core::util::{atomic_write, now_ms, random_bytes};
use agentvault_core::{Error, Result, Root};
use agentvault_keystore::Keystore;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MAGIC: &[u8; 6] = b"AVBAK1";
pub const KEY_NAME: &str = "backup/master";
const NONCE_LEN: usize = 12;

/// Encrypt `config` next to itself as `<config>.bak.<ms>.enc`.
pub fn create(config: &Path, root: &Root) -> Result<PathBuf> {
    let _lock = backup_lock(root)?;
    let plaintext = std::fs::read(config)?;
    let nonce = random_bytes::<NONCE_LEN>();
    let sealed = cipher(root)?
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_slice())
        .map_err(|_| Error::Other("backup encryption failed".into()))?;

    let mut bytes = Vec::with_capacity(MAGIC.len() + NONCE_LEN + sealed.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&nonce);
    bytes.extend_from_slice(&sealed);

    let backup = backup_path(config, now_ms());
    atomic_write(&backup, &bytes)?;
    Ok(backup)
}

/// Decrypt `backup` into `target` and re-parse it so a corrupt or truncated
/// backup never replaces a working config.
pub fn restore(backup: &Path, target: &Path, root: &Root) -> Result<()> {
    let _lock = backup_lock(root)?;
    let bytes = std::fs::read(backup)?;
    if bytes.len() <= MAGIC.len() + NONCE_LEN || &bytes[..MAGIC.len()] != MAGIC {
        return Err(Error::Other(format!(
            "not an AVBAK1 backup: {}",
            backup.display()
        )));
    }
    let nonce = &bytes[MAGIC.len()..MAGIC.len() + NONCE_LEN];
    let sealed = &bytes[MAGIC.len() + NONCE_LEN..];
    let plaintext = cipher(root)?
        .decrypt(Nonce::from_slice(nonce), sealed)
        .map_err(|_| Error::Other("backup decryption failed (wrong key or corrupt file)".into()))?;

    reparse(target, &plaintext)?;
    atomic_write(target, &plaintext)?;
    Ok(())
}

/// Remove every backup next to `config`: current `.bak.<ms>.enc` files and
/// legacy plaintext `.bak.<stamp>` files.
pub fn cleanup(config: &Path) -> Result<usize> {
    let mut removed = 0;
    for candidate in candidates(config)? {
        std::fs::remove_file(&candidate)?;
        removed += 1;
    }
    Ok(removed)
}

pub fn list(config: &Path) -> Result<Vec<PathBuf>> {
    candidates(config)
}

fn candidates(config: &Path) -> Result<Vec<PathBuf>> {
    let parent = match config.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let Some(name) = config.file_name().and_then(|value| value.to_str()) else {
        return Ok(Vec::new());
    };
    let prefix = format!("{name}.bak.");
    if !parent.exists() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in std::fs::read_dir(parent)? {
        let candidate = entry?.path();
        if !candidate.is_file() {
            continue;
        }
        let Some(file_name) = candidate.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if file_name.starts_with(&prefix) {
            found.push(candidate);
        }
    }
    found.sort();
    Ok(found)
}

fn backup_path(config: &Path, stamp: i64) -> PathBuf {
    let name = config
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("config");
    config.with_file_name(format!("{name}.bak.{stamp}.enc"))
}

fn cipher(root: &Root) -> Result<ChaCha20Poly1305> {
    let key_hex = Keystore::open(&root.dir).get_or_create_secret(KEY_NAME, 32)?;
    let key = hex::decode(key_hex.trim())
        .map_err(|_| Error::Other("backup key in keystore is not hex".into()))?;
    if key.len() != 32 {
        return Err(Error::Other("backup key has wrong length".into()));
    }
    Ok(ChaCha20Poly1305::new(Key::from_slice(&key)))
}

fn backup_lock(root: &Root) -> Result<FileLock> {
    root.ensure()?;
    FileLock::acquire(&root.dir.join("backup.lock"), Duration::from_secs(10))
}

fn reparse(target: &Path, plaintext: &[u8]) -> Result<()> {
    let is_toml = target.extension().and_then(|value| value.to_str()) == Some("toml");
    if is_toml {
        let text = std::str::from_utf8(plaintext)
            .map_err(|_| Error::Other("restored TOML is not valid UTF-8".into()))?;
        text.parse::<toml_edit::DocumentMut>()
            .map_err(|error| Error::Other(format!("restored file is not valid TOML: {error}")))?;
    } else {
        serde_json::from_slice::<serde_json::Value>(plaintext)
            .map_err(|error| Error::Other(format!("restored file is not valid JSON: {error}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_is_encrypted_and_restores() {
        let root =
            Root::new(std::env::temp_dir().join(format!("av-backup-{}", uuid::Uuid::new_v4())));
        root.ensure().unwrap();
        let config = root.dir.join("config.json");
        let body = br#"{"secrets":[{"name":"db"}]}"#;
        std::fs::write(&config, body).unwrap();

        let backup = create(&config, &root).unwrap();
        let raw = std::fs::read(&backup).unwrap();
        assert_eq!(&raw[..MAGIC.len()], MAGIC);
        assert!(!String::from_utf8_lossy(&raw).contains("\"db\""));

        std::fs::write(&config, b"{}").unwrap();
        restore(&backup, &config, &root).unwrap();
        assert_eq!(std::fs::read(&config).unwrap(), body);
        assert_eq!(list(&config).unwrap().len(), 1);
        assert_eq!(cleanup(&config).unwrap(), 1);
        assert!(list(&config).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root.dir);
    }
}
