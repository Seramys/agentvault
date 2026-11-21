//! Layer 0: OS keystore-backed master key, device binding and per-ticket
//! derivation. The long-lived secret never has to sit in a plaintext file.
//!
//! Backends:
//! - Windows: DPAPI-protected blobs (`dpapi.json`)
//! - macOS: Keychain (`security` CLI)
//! - Linux: Secret Service (`secret-tool`)
//! - Fallback: encrypted file, used only when the platform backend is absent

pub mod file_backend;
pub mod platform;

use agentvault_core::util::{atomic_write, random_bytes};
use agentvault_core::{Error, Result};
use hkdf::Hkdf;
use sha2::Sha256;
use std::path::{Path, PathBuf};

pub const MASTER_NAME: &str = "master";

#[derive(Clone, Debug)]
pub enum Backend {
    Os,
    File,
}

#[derive(Clone, Debug)]
pub struct Keystore {
    backend: Backend,
    dir: PathBuf,
}

impl Keystore {
    pub fn open(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref().to_path_buf();
        let backend = if platform::available() {
            Backend::Os
        } else {
            Backend::File
        };
        Self { backend, dir }
    }

    pub fn backend_name(&self) -> &'static str {
        match self.backend {
            Backend::Os => platform::backend_name(),
            Backend::File => "encrypted-file-fallback",
        }
    }

    pub fn master_key(&self) -> Result<[u8; 32]> {
        match self.backend {
            Backend::Os => load_or_create_os_master(&self.dir),
            Backend::File => file_backend::load_or_create_master(&self.dir),
        }
    }

    pub fn get(&self, name: &str) -> Result<Option<String>> {
        match self.backend {
            Backend::Os => platform::get(&self.dir, name),
            Backend::File => file_backend::get_secret(&self.dir, &self.master_key()?, name),
        }
    }

    pub fn set(&self, name: &str, value: &str) -> Result<()> {
        match self.backend {
            Backend::Os => platform::set(&self.dir, name, value),
            Backend::File => {
                let master = self.master_key()?;
                file_backend::set_secret(&self.dir, &master, name, value)
            }
        }
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        match self.backend {
            Backend::Os => platform::delete(&self.dir, name),
            Backend::File => {
                let master = self.master_key()?;
                file_backend::delete_secret(&self.dir, &master, name)
            }
        }
    }

    /// Load many secrets at once. On Windows this runs a single PowerShell
    /// DPAPI batch instead of spawning one process per secret, which keeps the
    /// per-tool-call latency at zero once the broker has warmed up.
    pub fn load_all(&self, names: &[String]) -> Result<std::collections::BTreeMap<String, String>> {
        if matches!(self.backend, Backend::Os) {
            if let Some(all) = platform::unseal_all(&self.dir)? {
                return Ok(names
                    .iter()
                    .filter_map(|name| all.get(name).map(|value| (name.clone(), value.clone())))
                    .collect());
            }
        }
        let mut out = std::collections::BTreeMap::new();
        for name in names {
            if let Some(value) = self.get(name)? {
                out.insert(name.clone(), value);
            }
        }
        Ok(out)
    }

    /// Derive a fresh, non-persisted value for one ticket. This is the
    /// "every-ticket derivation" part of layer 0: even if a derived value
    /// leaks, it is not the long-lived secret and it is not reusable.
    pub fn derive_ticket_value(&self, master: &[u8; 32], secret: &str, jti: &str) -> String {
        let hk = Hkdf::<Sha256>::new(Some(b"agentvault-derive"), master);
        let mut out = [0u8; 32];
        hk.expand(format!("{secret}:{jti}").as_bytes(), &mut out)
            .expect("32 bytes is a valid hkdf output length");
        hex::encode(out)
    }

    /// Rotate a stored secret: generate a new random value, overwrite it and
    /// return it so the caller can print it once (never again from disk).
    pub fn rotate(&self, name: &str) -> Result<String> {
        let new_value = hex::encode(random_bytes::<24>());
        self.set(name, &new_value)?;
        Ok(new_value)
    }
}

fn load_or_create_os_master(dir: &Path) -> Result<[u8; 32]> {
    match platform::get(dir, MASTER_NAME)? {
        Some(hex_key) => {
            let bytes = hex::decode(hex_key.trim())
                .map_err(|_| Error::Other("corrupt master key in keystore".into()))?;
            if bytes.len() != 32 {
                return Err(Error::Other("master key has wrong length".into()));
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            Ok(key)
        }
        None => {
            let key = random_bytes::<32>();
            platform::set(dir, MASTER_NAME, &hex::encode(key))?;
            Ok(key)
        }
    }
}

/// Write a sensitive file with restrictive permissions on Unix and an explicit
/// owner-only ACL on Windows.
pub fn write_private(path: &Path, value: &str) -> Result<()> {
    atomic_write(path, value.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(windows)]
    {
        // Remove inherited ACEs and grant only the current user. This is the
        // Windows analogue of chmod 0600 for admin.token and session files.
        if let Ok(user) = std::env::var("USERNAME") {
            if !user.is_empty() {
                let _ = std::process::Command::new("icacls")
                    .arg(path)
                    .arg("/inheritance:r")
                    .arg("/grant:r")
                    .arg(format!("{user}:F"))
                    .output();
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_is_deterministic_per_ticket() {
        let ks = Keystore::open(std::env::temp_dir());
        let master = [9u8; 32];
        let a = ks.derive_ticket_value(&master, "db", "jti-1");
        let b = ks.derive_ticket_value(&master, "db", "jti-1");
        let c = ks.derive_ticket_value(&master, "db", "jti-2");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64);
    }
}
