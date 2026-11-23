//! Platform keystore backends with no third-party crypto/link dependencies.
//!
//! - Windows: DPAPI (`CryptProtectData` through PowerShell), blobs stored in
//!   `dpapi.json` under the agentvault directory. DPAPI binds to the Windows
//!   user, so copying the file to another account or machine does not help.
//! - macOS: Keychain through the `security` CLI.
//! - Linux: Secret Service through `secret-tool` (libsecret).
//!
//! Every backend exposes the same four operations. `dir` is only used by the
//! Windows DPAPI file store.

use agentvault_core::{Error, Result};
use std::path::Path;

#[cfg(windows)]
mod imp {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::process::Command;

    #[derive(Serialize, Deserialize, Default)]
    struct Vault {
        #[serde(default)]
        entries: BTreeMap<String, String>,
    }

    pub fn available() -> bool {
        run_ps("Add-Type -AssemblyName System.Security; 'ok'")
            .map(|out| out.contains("ok"))
            .unwrap_or(false)
    }

    pub fn get(dir: &Path, name: &str) -> Result<Option<String>> {
        let vault = read_vault(dir)?;
        let Some(sealed) = vault.entries.get(name) else {
            return Ok(None);
        };
        let plain = unprotect(sealed)?;
        Ok(Some(plain))
    }

    pub fn set(dir: &Path, name: &str, value: &str) -> Result<()> {
        let mut vault = read_vault(dir)?;
        vault.entries.insert(name.to_string(), protect(dir, value)?);
        write_vault(dir, &vault)
    }

    pub fn delete(dir: &Path, name: &str) -> Result<()> {
        let mut vault = read_vault(dir)?;
        vault.entries.remove(name);
        write_vault(dir, &vault)
    }

    /// Decrypt every DPAPI blob in one PowerShell process. Without this, the
    /// broker would spawn PowerShell once per secret on startup.
    pub fn unseal_all(dir: &Path) -> Result<Option<std::collections::BTreeMap<String, String>>> {
        let vault = read_vault(dir)?;
        if vault.entries.is_empty() {
            return Ok(Some(std::collections::BTreeMap::new()));
        }
        let path = vault_path(dir).display().to_string().replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Security; \
             $o = Get-Content -Raw '{path}' | ConvertFrom-Json; \
             foreach ($p in $o.entries.PSObject.Properties) {{ \
               $b = [Convert]::FromBase64String($p.Value); \
               $x = [Security.Cryptography.ProtectedData]::Unprotect($b,$null,'CurrentUser'); \
               $n = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($p.Name)); \
               Write-Output ($n + '=' + [Convert]::ToBase64String($x)) \
             }}"
        );
        let out = run_ps(&script)?;
        let mut map = std::collections::BTreeMap::new();
        for line in out.lines() {
            let Some((name_b64, b64)) = line.split_once('=') else {
                continue;
            };
            let name = STANDARD
                .decode(name_b64)
                .map_err(|_| Error::Other("batch DPAPI name is not base64".into()))?;
            let name = String::from_utf8(name)
                .map_err(|_| Error::Other("batch DPAPI name is not utf-8".into()))?;
            let bytes = STANDARD
                .decode(b64)
                .map_err(|_| Error::Other("batch DPAPI output is not base64".into()))?;
            let value = String::from_utf8(bytes)
                .map_err(|_| Error::Other("batch DPAPI plaintext is not utf-8".into()))?;
            map.insert(name, value);
        }
        Ok(Some(map))
    }

    fn vault_path(dir: &Path) -> PathBuf {
        dir.join("dpapi.json")
    }

    fn read_vault(dir: &Path) -> Result<Vault> {
        let path = vault_path(dir);
        if !path.exists() {
            return Ok(Vault::default());
        }
        let data = std::fs::read(&path)?;
        Ok(serde_json::from_slice(&data)?)
    }

    fn write_vault(dir: &Path, vault: &Vault) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        let mut data = serde_json::to_vec_pretty(vault)?;
        data.push(b'\n');
        agentvault_core::util::atomic_write(&vault_path(dir), &data)
    }

    fn protect(dir: &Path, value: &str) -> Result<String> {
        // Never put plaintext (even base64) on the PowerShell command line:
        // other processes running as the same user can read it there. The
        // value goes through a short-lived private temp file instead.
        std::fs::create_dir_all(dir)?;
        let plain_b64 = STANDARD.encode(value.as_bytes());
        let temp = dir.join(format!(
            ".dpapi-protect-{}-{}.tmp",
            std::process::id(),
            agentvault_core::util::now_ms()
        ));
        crate::write_private(&temp, &plain_b64)?;
        let temp_path = temp.display().to_string().replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Security; \
             $plain = Get-Content -Raw '{temp_path}'; \
             $b=[Convert]::FromBase64String($plain.Trim()); \
             $p=[Security.Cryptography.ProtectedData]::Protect($b,$null,'CurrentUser'); \
             [Convert]::ToBase64String($p)"
        );
        let result = run_ps(&script);
        let _ = std::fs::remove_file(&temp);
        result
    }

    fn unprotect(sealed: &str) -> Result<String> {
        let script = format!(
            "Add-Type -AssemblyName System.Security; \
             $b=[Convert]::FromBase64String('{sealed}'); \
             $p=[Security.Cryptography.ProtectedData]::Unprotect($b,$null,'CurrentUser'); \
             [Convert]::ToBase64String($p)"
        );
        let out = run_ps(&script)?;
        let bytes = STANDARD
            .decode(out.trim())
            .map_err(|_| Error::Other("DPAPI output is not base64".into()))?;
        String::from_utf8(bytes).map_err(|_| Error::Other("DPAPI plaintext is not utf-8".into()))
    }

    fn run_ps(script: &str) -> Result<String> {
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .output()
            .map_err(|e| Error::Other(format!("cannot start powershell: {e}")))?;
        if !output.status.success() {
            return Err(Error::Other(format!(
                "DPAPI call failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::process::Command;

    fn service(name: &str) -> String {
        format!("agentvault:{name}")
    }

    pub fn available() -> bool {
        Command::new("security").arg("-h").output().is_ok()
    }

    pub fn get(_dir: &Path, name: &str) -> Result<Option<String>> {
        let output = Command::new("security")
            .args(["find-generic-password", "-s", &service(name), "-w"])
            .output()
            .map_err(|e| Error::Other(format!("security CLI: {e}")))?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(Some(
            String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_string(),
        ))
    }

    pub fn set(_dir: &Path, name: &str, value: &str) -> Result<()> {
        let status = Command::new("security")
            .args([
                "add-generic-password",
                "-a",
                "agentvault",
                "-s",
                &service(name),
                "-w",
                value,
                "-U",
            ])
            .status()
            .map_err(|e| Error::Other(format!("security CLI: {e}")))?;
        if !status.success() {
            return Err(Error::Other("Keychain rejected the secret".into()));
        }
        Ok(())
    }

    pub fn delete(_dir: &Path, name: &str) -> Result<()> {
        let _ = Command::new("security")
            .args(["delete-generic-password", "-s", &service(name)])
            .status();
        Ok(())
    }

    pub fn unseal_all(_dir: &Path) -> Result<Option<std::collections::BTreeMap<String, String>>> {
        Ok(None)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    pub fn available() -> bool {
        Command::new("secret-tool").arg("--help").output().is_ok()
    }

    pub fn get(_dir: &Path, name: &str) -> Result<Option<String>> {
        let output = Command::new("secret-tool")
            .args(["lookup", "service", "agentvault", "key", name])
            .output()
            .map_err(|e| Error::Other(format!("secret-tool: {e}")))?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(Some(
            String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_string(),
        ))
    }

    pub fn set(_dir: &Path, name: &str, value: &str) -> Result<()> {
        let mut child = Command::new("secret-tool")
            .args([
                "store",
                "--label",
                &format!("agentvault:{name}"),
                "service",
                "agentvault",
                "key",
                name,
            ])
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Other(format!("secret-tool: {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(value.as_bytes())?;
        }
        let status = child.wait()?;
        if !status.success() {
            return Err(Error::Other("Secret Service rejected the secret".into()));
        }
        Ok(())
    }

    pub fn delete(_dir: &Path, name: &str) -> Result<()> {
        let _ = Command::new("secret-tool")
            .args(["clear", "service", "agentvault", "key", name])
            .status();
        Ok(())
    }

    pub fn unseal_all(_dir: &Path) -> Result<Option<std::collections::BTreeMap<String, String>>> {
        Ok(None)
    }
}

pub fn available() -> bool {
    #[cfg(windows)]
    {
        return imp::available();
    }
    #[cfg(not(windows))]
    {
        // `security` and `secret-tool` can be installed on a host that has no
        // unlocked keychain or D-Bus session (CI runners, headless servers).
        // Checking the binary alone would select a backend that fails every
        // read/write, so verify one real round-trip first.
        static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *AVAILABLE.get_or_init(|| imp::available() && probe_roundtrip())
    }
}

#[cfg(not(windows))]
fn probe_roundtrip() -> bool {
    let dir = std::env::temp_dir();
    let name = format!("agentvault-probe-{}", std::process::id());
    if imp::set(&dir, &name, "ok").is_err() {
        return false;
    }
    let usable = matches!(imp::get(&dir, &name), Ok(Some(value)) if value == "ok");
    let _ = imp::delete(&dir, &name);
    usable
}

pub fn get(dir: &Path, name: &str) -> Result<Option<String>> {
    imp::get(dir, name)
}

pub fn set(dir: &Path, name: &str, value: &str) -> Result<()> {
    imp::set(dir, name, value)
}

pub fn delete(dir: &Path, name: &str) -> Result<()> {
    imp::delete(dir, name)
}

pub fn unseal_all(dir: &Path) -> Result<Option<std::collections::BTreeMap<String, String>>> {
    imp::unseal_all(dir)
}

pub fn backend_name() -> &'static str {
    if cfg!(windows) {
        "windows-dpapi"
    } else if cfg!(target_os = "macos") {
        "macos-keychain"
    } else {
        "linux-secret-service"
    }
}
