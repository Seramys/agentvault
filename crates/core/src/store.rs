use crate::error::{Error, Result};
use crate::model::{Config, HoneypotStore, RevokedStore};
use crate::paths::Root;
use crate::util::atomic_write;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let data = std::fs::read(path)?;
    Ok(serde_json::from_slice(&data)?)
}

/// Cross-process write lock.
///
/// The broker is the only writer in normal operation, but `init`, `rotate`,
/// `revoke` and a second broker instance can race. A lock file created with
/// `create_new` gives every platform an atomic mutex without extra crates.
pub struct FileLock {
    path: PathBuf,
}

impl FileLock {
    pub fn acquire(path: &Path, timeout: Duration) -> Result<Self> {
        use std::io::Write;
        let start = Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(mut file) => {
                    let _ = writeln!(file, "{}", std::process::id());
                    return Ok(Self {
                        path: path.to_path_buf(),
                    });
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::AlreadyExists
                        || e.kind() == std::io::ErrorKind::PermissionDenied =>
                {
                    // Recover a lock left behind by a crashed process.
                    if let Ok(meta) = std::fs::metadata(path) {
                        if let Ok(modified) = meta.modified() {
                            if modified.elapsed().unwrap_or_default() > Duration::from_secs(30) {
                                let _ = std::fs::remove_file(path);
                                continue;
                            }
                        }
                    }
                    if start.elapsed() > timeout {
                        return Err(Error::Other(format!(
                            "timed out waiting for lock {}",
                            path.display()
                        )));
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn lock_path(path: &Path) -> PathBuf {
    path.with_extension("lock")
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let lock = FileLock::acquire(&lock_path(path), Duration::from_secs(10))?;
    let mut data = serde_json::to_vec_pretty(value)?;
    data.push(b'\n');
    atomic_write(path, &data)?;
    drop(lock);
    Ok(())
}

pub fn load_config(root: &Root) -> Result<Config> {
    let path = root.config();
    if !path.exists() {
        return Ok(Config::fresh());
    }
    read_json(&path)
}

pub fn load_config_checked(root: &Root) -> Result<Config> {
    let cfg: Config = load_config(root)?;
    cfg.check_device()?;
    Ok(cfg)
}

pub fn save_config(root: &Root, cfg: &Config) -> Result<()> {
    write_json(&root.config(), cfg)
}

pub fn load_revoked(root: &Root) -> Result<RevokedStore> {
    let path = root.revoked();
    if !path.exists() {
        return Ok(RevokedStore::default());
    }
    read_json(&path)
}

pub fn save_revoked(root: &Root, store: &RevokedStore) -> Result<()> {
    write_json(&root.revoked(), store)
}

pub fn load_honeypots(root: &Root) -> Result<HoneypotStore> {
    let path = root.honeypots();
    if !path.exists() {
        return Ok(HoneypotStore::default());
    }
    read_json(&path)
}

pub fn save_honeypots(root: &Root, store: &HoneypotStore) -> Result<()> {
    write_json(&root.honeypots(), store)
}

pub fn ensure_config(root: &Root) -> Result<Config> {
    root.ensure()?;
    let path = root.config();
    if path.exists() {
        let cfg: Config = read_json(&path)?;
        cfg.check_device()?;
        return Ok(cfg);
    }
    let cfg = Config::fresh();
    save_config(root, &cfg)?;
    Ok(cfg)
}

pub fn find_secret<'a>(cfg: &'a Config, name: &str) -> Result<&'a crate::model::SecretRecord> {
    cfg.secret(name)
        .ok_or_else(|| Error::NotFound(format!("secret `{name}`")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SecretKind;

    #[test]
    fn config_roundtrip() {
        let dir = std::env::temp_dir().join(format!("av-test-{}", uuid::Uuid::new_v4()));
        let root = Root::new(&dir);
        root.ensure().unwrap();
        let mut cfg = Config::fresh();
        cfg.upsert_secret(crate::model::SecretRecord::new(
            "postgres/prod",
            SecretKind::Env,
            false,
        ));
        save_config(&root, &cfg).unwrap();
        let loaded = load_config(&root).unwrap();
        assert_eq!(loaded.secrets.len(), 1);
        assert_eq!(loaded.secrets[0].name, "postgres/prod");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_writes_do_not_corrupt_json() {
        let dir = std::env::temp_dir().join(format!("av-lock-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.json");

        let mut handles = Vec::new();
        for i in 0..8 {
            let path = path.clone();
            handles.push(std::thread::spawn(move || {
                for n in 0..10 {
                    let value = serde_json::json!({ "writer": i, "n": n });
                    write_json(&path, &value).unwrap();
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }

        // The last write must still be a complete, parseable JSON document.
        let parsed: serde_json::Value = read_json(&path).unwrap();
        assert!(parsed.get("writer").is_some());
        assert!(!dir.join("data.lock").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
