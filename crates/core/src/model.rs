use crate::error::{Error, Result};
use crate::util::{device_fingerprint, now_ms};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    Env,
    Token,
    Json,
}

impl Default for SecretKind {
    fn default() -> Self {
        SecretKind::Token
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SecretRecord {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: SecretKind,
    #[serde(default)]
    pub honeypot: bool,
    pub created_at: i64,
    pub rotated_at: i64,
    /// Bumped on every rotation or secret-level revocation. Tickets are signed
    /// with a key derived from this value, so old tickets stop verifying.
    #[serde(default)]
    pub epoch: u64,
    #[serde(default)]
    pub note: Option<String>,
}

impl SecretRecord {
    pub fn new(name: impl Into<String>, kind: SecretKind, honeypot: bool) -> Self {
        let ts = now_ms();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            kind,
            honeypot,
            created_at: ts,
            rotated_at: ts,
            epoch: 0,
            note: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentBinding {
    pub agent: String,
    pub config_path: String,
    pub servers: Vec<String>,
    pub bound_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub default_ttl_secs: u64,
    pub default_max_uses: u32,
    pub auto_revoke_on_honeypot: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_ttl_secs: 900,
            default_max_uses: 1,
            auto_revoke_on_honeypot: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub schema: u32,
    pub device_fingerprint: String,
    /// Hardware-derived fingerprint (WMI UUID / IOPlatformUUID / DMI UUID).
    #[serde(default)]
    pub hardware_fp: Option<String>,
    pub created_at: i64,
    pub secrets: Vec<SecretRecord>,
    pub agents: Vec<AgentBinding>,
    pub settings: Settings,
}

impl Config {
    pub fn fresh() -> Self {
        Self {
            schema: SCHEMA_VERSION,
            device_fingerprint: device_fingerprint(),
            // Computed lazily by `init --rebind`; broker startup must not block
            // on WMI / TPM queries.
            hardware_fp: None,
            created_at: now_ms(),
            secrets: Vec::new(),
            agents: Vec::new(),
            settings: Settings::default(),
        }
    }

    pub fn with_fingerprint(fingerprint: String) -> Self {
        let mut cfg = Self::fresh();
        cfg.device_fingerprint = fingerprint;
        cfg
    }

    pub fn check_device(&self) -> Result<()> {
        let current = device_fingerprint();
        if self.device_fingerprint == current {
            return Ok(());
        }
        Err(Error::Denied(format!(
            "device fingerprint mismatch (config={}, current={}). \
             Run `agentvault init --rebind` if this machine is trusted.",
            short(&self.device_fingerprint),
            short(&current)
        )))
    }

    /// Slower than `check_device`: this reads WMI/DMI hardware identifiers.
    /// Call it once when the broker starts, not on every CLI command.
    pub fn check_hardware(&self) -> Result<()> {
        let Some(expected) = &self.hardware_fp else {
            return Ok(());
        };
        let current = crate::util::hardware_fingerprint();
        if expected == &current {
            return Ok(());
        }
        Err(Error::Denied(format!(
            "hardware fingerprint mismatch (config={}, current={}). \
             The config directory was copied from another machine. \
             Run `agentvault init --rebind` if this is intentional.",
            short(expected),
            short(&current)
        )))
    }

    pub fn rebind(&mut self) {
        self.device_fingerprint = device_fingerprint();
        self.hardware_fp = Some(crate::util::hardware_fingerprint());
    }

    pub fn secret(&self, name: &str) -> Option<&SecretRecord> {
        self.secrets.iter().find(|s| s.name == name)
    }

    pub fn secret_mut(&mut self, name: &str) -> Option<&mut SecretRecord> {
        self.secrets.iter_mut().find(|s| s.name == name)
    }

    pub fn upsert_secret(&mut self, record: SecretRecord) {
        match self.secrets.iter_mut().find(|s| s.name == record.name) {
            Some(slot) => *slot = record,
            None => self.secrets.push(record),
        }
    }
}

fn short(value: &str) -> String {
    value.chars().take(8).collect()
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct RevokedStore {
    /// Global epoch. Bumping it invalidates every ticket signed before the bump.
    #[serde(default)]
    pub epoch: u64,
    #[serde(default)]
    pub jtis: HashSet<String>,
    #[serde(default)]
    pub secrets: HashSet<String>,
    #[serde(default)]
    pub updated_at: i64,
}

impl RevokedStore {
    pub fn revoke_ticket(&mut self, jti: &str) {
        self.jtis.insert(jti.to_string());
        self.updated_at = now_ms();
    }

    pub fn revoke_secret(&mut self, name: &str) {
        self.secrets.insert(name.to_string());
        self.updated_at = now_ms();
    }

    pub fn revoke_all(&mut self) {
        self.epoch = self.epoch.saturating_add(1);
        self.jtis.clear();
        self.secrets.clear();
        self.updated_at = now_ms();
    }

    pub fn is_ticket_revoked(&self, jti: &str) -> bool {
        self.jtis.contains(jti)
    }

    pub fn is_secret_revoked(&self, name: &str) -> bool {
        self.secrets.contains(name)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Honeypot {
    pub id: String,
    pub name: String,
    pub token: String,
    pub created_at: i64,
    #[serde(default)]
    pub triggered_at: Option<i64>,
    #[serde(default)]
    pub hits: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct HoneypotStore {
    #[serde(default)]
    pub items: Vec<Honeypot>,
}

impl HoneypotStore {
    pub fn add(&mut self, name: &str, token: String) -> Honeypot {
        let item = Honeypot {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            token,
            created_at: now_ms(),
            triggered_at: None,
            hits: 0,
        };
        self.items.push(item.clone());
        item
    }

    pub fn match_token(&mut self, candidate: &str) -> Option<Honeypot> {
        let ts = now_ms();
        for item in self.items.iter_mut() {
            if item.token == candidate {
                item.hits = item.hits.saturating_add(1);
                item.triggered_at = Some(ts);
                return Some(item.clone());
            }
        }
        None
    }

    pub fn remove(&mut self, id_or_name: &str) -> bool {
        let before = self.items.len();
        self.items
            .retain(|h| h.id != id_or_name && h.name != id_or_name);
        self.items.len() != before
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TicketKind {
    /// One-shot ticket. Can be resolved but cannot mint children.
    #[default]
    Leaf,
    /// Task-level ticket. Cannot resolve directly; mints short leaf tickets
    /// and can be renewed until its hard limit.
    Parent,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TicketClaims {
    pub v: u8,
    pub jti: String,
    pub sub: String,
    pub sid: String,
    pub secret: String,
    pub scope: String,
    pub iat: i64,
    pub exp: i64,
    pub epoch: u64,
    /// Global revocation epoch used to derive the signing key. Distinct from
    /// `epoch` (the per-secret rotation epoch), so diagnostics can tell a
    /// globally revoked ticket from one signed with the wrong key.
    #[serde(default)]
    pub gepoch: u64,
    pub max_uses: u32,
    /// Defaults to leaf so tickets issued before this field existed still work.
    #[serde(default)]
    pub kind: TicketKind,
    /// For leaf tickets: the parent task ticket that minted them.
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub renewable: bool,
    /// Absolute ceiling for sliding renewal. 0 means "same as exp".
    #[serde(default)]
    pub hard_exp: i64,
    #[serde(default)]
    pub renewals: u32,
}

impl Default for TicketClaims {
    fn default() -> Self {
        Self {
            v: 1,
            jti: String::new(),
            sub: String::new(),
            sid: String::new(),
            secret: String::new(),
            scope: "read".into(),
            iat: 0,
            exp: 0,
            epoch: 0,
            gepoch: 0,
            max_uses: 1,
            kind: TicketKind::Leaf,
            parent: None,
            renewable: false,
            hard_exp: 0,
            renewals: 0,
        }
    }
}

impl TicketClaims {
    pub fn is_expired(&self, now: i64) -> bool {
        now >= self.exp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revoke_all_bumps_epoch() {
        let mut store = RevokedStore::default();
        store.revoke_ticket("t1");
        store.revoke_all();
        assert_eq!(store.epoch, 1);
        assert!(store.jtis.is_empty());
        assert!(!store.is_ticket_revoked("t1"));
    }

    #[test]
    fn honeypot_matches_exact_token_only() {
        let mut store = HoneypotStore::default();
        store.add("prod-db", "canary-123".into());
        assert!(store.match_token("canary-123").is_some());
        assert!(store.match_token("canary-12").is_none());
        assert_eq!(store.items[0].hits, 1);
    }
}
