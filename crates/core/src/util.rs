use crate::error::{Error, Result};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Parse a human duration such as `30s`, `15m`, `2h`, `7d`.
pub fn parse_duration(input: &str) -> Result<u64> {
    let s = input.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Err(Error::Other("empty duration".into()));
    }
    let (digits, unit) = s.split_at(s.len() - 1);
    let value: u64 = digits
        .parse()
        .map_err(|_| Error::Other(format!("invalid duration: {input}")))?;
    let mult = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => return Err(Error::Other(format!("invalid duration unit: {input}"))),
    };
    value
        .checked_mul(mult)
        .ok_or_else(|| Error::Other(format!("duration is too large: {input}")))
}

/// Expand `~`, `$VAR`, `${VAR}` and `%VAR%` in a user-supplied path.
pub fn expand_path(input: &str) -> std::path::PathBuf {
    let mut text = input.trim().to_string();
    if text == "~" || text.starts_with("~/") || text.starts_with("~\\") {
        if let Some(home) = home_dir() {
            let rest = text
                .trim_start_matches('~')
                .trim_start_matches(['/', '\\'])
                .to_string();
            text = if rest.is_empty() {
                home.display().to_string()
            } else {
                home.join(rest).display().to_string()
            };
        }
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '%' {
            if let Some(end) = chars[index + 1..].iter().position(|c| *c == '%') {
                let name: String = chars[index + 1..index + 1 + end].iter().collect();
                if is_env_name(&name) {
                    if let Ok(value) = std::env::var(&name) {
                        out.push_str(&value);
                        index += end + 2;
                        continue;
                    }
                }
            }
        } else if chars[index] == '$' {
            let (name, next) = if chars.get(index + 1) == Some(&'{') {
                match chars[index + 2..].iter().position(|c| *c == '}') {
                    Some(end) => (
                        chars[index + 2..index + 2 + end].iter().collect::<String>(),
                        index + 3 + end,
                    ),
                    None => (String::new(), index + 1),
                }
            } else {
                let mut end = index + 1;
                while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_')
                {
                    end += 1;
                }
                (chars[index + 1..end].iter().collect::<String>(), end)
            };
            if is_env_name(&name) {
                if let Ok(value) = std::env::var(&name) {
                    out.push_str(&value);
                    index = next;
                    continue;
                }
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    std::path::PathBuf::from(out)
}

fn is_env_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    rand::thread_rng().fill_bytes(&mut buf);
    buf
}

pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(data);
    let out = mac.finalize().into_bytes();
    let mut result = [0u8; 32];
    result.copy_from_slice(&out);
    result
}

/// A best-effort device fingerprint. Real hardware binding is provided by the
/// platform keystore (DPAPI / Keychain / Secret Service); this value only makes
/// accidental copies of the config directory obvious.
pub fn device_fingerprint() -> String {
    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown-host".into());
    let username = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown-user".into());
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    sha256_hex(format!("{hostname}|{username}|{os}|{arch}").as_bytes())
}

#[cfg(windows)]
fn run_powershell(script: &str) -> Option<String> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Hardware-derived identity (WMI UUID / IOPlatformUUID / DMI product UUID).
/// Unlike `device_fingerprint`, renaming the host or user does not change it.
#[cfg(windows)]
pub fn hardware_fingerprint() -> String {
    let script = "$u=(Get-CimInstance Win32_ComputerSystemProduct -ErrorAction SilentlyContinue).UUID; \
                  $b=(Get-CimInstance Win32_BaseBoard -ErrorAction SilentlyContinue).SerialNumber; \
                  $d=(Get-CimInstance Win32_DiskDrive -ErrorAction SilentlyContinue | Select-Object -First 1).SerialNumber; \
                  Write-Output ($u+'|'+$b+'|'+$d)";
    match run_powershell(script) {
        Some(raw) if !raw.is_empty() => sha256_hex(raw.as_bytes()),
        _ => device_fingerprint(),
    }
}

#[cfg(target_os = "macos")]
pub fn hardware_fingerprint() -> String {
    if let Ok(output) = std::process::Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        if let Some(uuid) = text
            .lines()
            .find(|line| line.contains("IOPlatformUUID"))
            .and_then(|line| line.split('"').nth(3))
        {
            if !uuid.is_empty() {
                return sha256_hex(uuid.as_bytes());
            }
        }
    }
    device_fingerprint()
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn hardware_fingerprint() -> String {
    let machine = std::fs::read_to_string("/etc/machine-id").unwrap_or_default();
    let dmi = std::fs::read_to_string("/sys/class/dmi/id/product_uuid").unwrap_or_default();
    let raw = format!("{}|{}", machine.trim(), dmi.trim());
    if raw == "|" {
        device_fingerprint()
    } else {
        sha256_hex(raw.as_bytes())
    }
}

pub fn redact(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 {
        return "****".into();
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars
        .iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}...{tail}")
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    Ok(())
}

/// Home directory without pulling in the `dirs` crate (which would drag in a
/// large Windows-only dependency tree).
pub fn home_dir() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            return Some(std::path::PathBuf::from(profile));
        }
        let drive = std::env::var_os("HOMEDRIVE")?;
        let path = std::env::var_os("HOMEPATH")?;
        let mut joined = std::path::PathBuf::from(drive);
        joined.push(path);
        Some(joined)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(std::path::PathBuf::from)
    }
}

pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, data)?;
    // On Windows, rename fails if the destination exists; remove first.
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("30s").unwrap(), 30);
        assert_eq!(parse_duration("15m").unwrap(), 900);
        assert_eq!(parse_duration("2h").unwrap(), 7200);
        assert_eq!(parse_duration("1d").unwrap(), 86400);
        assert!(parse_duration("15x").is_err());
        assert!(parse_duration("").is_err());
    }

    #[test]
    fn redacts_short_and_long_values() {
        assert_eq!(redact("abc"), "****");
        assert_eq!(redact("postgres://user:password@host/db"), "post...t/db");
    }
}
