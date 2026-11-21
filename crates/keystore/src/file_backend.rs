//! Encrypted-file fallback for environments without an OS keystore
//! (for example a headless Linux box). Not a replacement for DPAPI/Keychain:
//! the master key is protected by file permissions only.

use agentvault_core::util::{hmac_sha256, random_bytes};
use agentvault_core::{Error, Result};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MASTER_FILE: &str = "master.key";
const SECRETS_FILE: &str = "secrets.enc.json";

#[derive(Serialize, Deserialize, Default)]
struct SecretFile {
    items: BTreeMap<String, SealedValue>,
}

#[derive(Serialize, Deserialize, Clone)]
struct SealedValue {
    nonce: String,
    ciphertext: String,
    tag: String,
}

pub fn load_or_create_master(dir: &Path) -> Result<[u8; 32]> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(MASTER_FILE);
    if path.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        let raw = std::fs::read_to_string(&path)?;
        let bytes =
            hex::decode(raw.trim()).map_err(|_| Error::Other("corrupt master key file".into()))?;
        if bytes.len() != 32 {
            return Err(Error::Other("master key has wrong length".into()));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        return Ok(key);
    }
    let key = random_bytes::<32>();
    crate::write_private(&path, &hex::encode(key))?;
    Ok(key)
}

pub fn get_secret(dir: &Path, master: &[u8; 32], name: &str) -> Result<Option<String>> {
    let file = read_file(dir)?;
    let Some(sealed) = file.items.get(name) else {
        return Ok(None);
    };
    let plain = open_sealed(master, name, sealed)?;
    String::from_utf8(plain)
        .map(Some)
        .map_err(|_| Error::Other("stored secret is not valid utf-8".into()))
}

pub fn set_secret(dir: &Path, master: &[u8; 32], name: &str, value: &str) -> Result<()> {
    let mut file = read_file(dir)?;
    let sealed = seal(master, name, value.as_bytes());
    file.items.insert(name.to_string(), sealed);
    write_file(dir, &file)
}

pub fn delete_secret(dir: &Path, _master: &[u8; 32], name: &str) -> Result<()> {
    let mut file = read_file(dir)?;
    file.items.remove(name);
    write_file(dir, &file)
}

fn secrets_path(dir: &Path) -> PathBuf {
    dir.join(SECRETS_FILE)
}

fn read_file(dir: &Path) -> Result<SecretFile> {
    let path = secrets_path(dir);
    if !path.exists() {
        return Ok(SecretFile::default());
    }
    let data = std::fs::read(&path)?;
    Ok(serde_json::from_slice(&data)?)
}

fn write_file(dir: &Path, file: &SecretFile) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut data = serde_json::to_vec_pretty(file)?;
    data.push(b'\n');
    let text = String::from_utf8(data)
        .map_err(|_| Error::Other("secret store serialization produced invalid utf-8".into()))?;
    crate::write_private(&secrets_path(dir), &text)
}

fn seal(master: &[u8; 32], name: &str, plaintext: &[u8]) -> SealedValue {
    let nonce = random_bytes::<16>();
    let keystream = keystream(master, name, &nonce, plaintext.len());
    let ciphertext: Vec<u8> = plaintext
        .iter()
        .zip(keystream.iter())
        .map(|(p, k)| p ^ k)
        .collect();
    let tag = hmac_sha256(master, &[nonce.as_slice(), ciphertext.as_slice()].concat());
    SealedValue {
        nonce: hex::encode(nonce),
        ciphertext: hex::encode(&ciphertext),
        tag: hex::encode(tag),
    }
}

fn open_sealed(master: &[u8; 32], name: &str, sealed: &SealedValue) -> Result<Vec<u8>> {
    let nonce = hex::decode(&sealed.nonce).map_err(|_| Error::Other("bad nonce".into()))?;
    let ciphertext =
        hex::decode(&sealed.ciphertext).map_err(|_| Error::Other("bad ciphertext".into()))?;
    let expected = hex::decode(&sealed.tag).map_err(|_| Error::Other("bad tag".into()))?;
    let actual = hmac_sha256(
        master,
        &[[nonce.as_slice(), ciphertext.as_slice()].concat()].concat(),
    );
    if expected != actual {
        return Err(Error::Other("secret integrity check failed".into()));
    }
    let keystream = keystream(master, name, &nonce, ciphertext.len());
    Ok(ciphertext
        .iter()
        .zip(keystream.iter())
        .map(|(c, k)| c ^ k)
        .collect())
}

/// Length-agnostic keystream built from HKDF blocks. Each block is bound to
/// the secret name and the random nonce, so the same plaintext never produces
/// the same ciphertext twice.
fn keystream(master: &[u8; 32], name: &str, nonce: &[u8], len: usize) -> Vec<u8> {
    let hk = Hkdf::<Sha256>::new(Some(nonce), master);
    let mut out = Vec::with_capacity(len);
    let mut block = 0u32;
    while out.len() < len {
        let mut buf = [0u8; 32];
        hk.expand(format!("{name}:{block}").as_bytes(), &mut buf)
            .expect("32 bytes is a valid hkdf output length");
        out.extend_from_slice(&buf);
        block += 1;
    }
    out.truncate(len);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("av-ks-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn file_backend_roundtrip_and_tamper_detection() {
        let dir = temp_dir();
        let master = load_or_create_master(&dir).unwrap();
        set_secret(&dir, &master, "db", "postgres://user:pass@host/db").unwrap();
        assert_eq!(
            get_secret(&dir, &master, "db").unwrap().as_deref(),
            Some("postgres://user:pass@host/db")
        );

        let mut file = read_file(&dir).unwrap();
        file.items.get_mut("db").unwrap().ciphertext.push_str("00");
        write_file(&dir, &file).unwrap();
        assert!(get_secret(&dir, &master, "db").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
