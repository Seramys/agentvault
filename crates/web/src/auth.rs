//! Local accounts, roles, tenants and sessions for the web console.
//!
//! Both state files live under `AGENTVAULT_HOME` and are written with private
//! permissions. Passwords are stored as argon2id hashes and session tokens as
//! SHA-256 hashes, so neither file exposes a usable credential.

use agentvault_core::util::random_hex;
use agentvault_core::{store, Error, Result, Root};
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const DEFAULT_TENANT: &str = "default";
pub const SESSION_PREFIX: &str = "avs_";

const SESSION_TTL_MS: i64 = 12 * 3600 * 1000;
const MIN_PASSWORD_CHARS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Admin,
    Viewer,
}

impl Role {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "owner" => Ok(Role::Owner),
            "admin" => Ok(Role::Admin),
            "viewer" => Ok(Role::Viewer),
            other => Err(Error::Other(format!("unknown role: {other}"))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Viewer => "viewer",
        }
    }

    /// Viewers are read-only; owner and admin may mutate.
    pub fn can_write(self) -> bool {
        !matches!(self, Role::Viewer)
    }
}

/// Caller resolved by the auth middleware.
#[derive(Clone, Debug)]
pub struct Identity {
    pub username: String,
    pub role: Role,
    pub tenant: String,
    #[allow(dead_code)]
    pub session: Option<String>,
}

impl Identity {
    /// The bootstrap token from `web.token` acts as a local owner.
    pub fn bootstrap() -> Self {
        Self {
            username: "local".into(),
            role: Role::Owner,
            tenant: DEFAULT_TENANT.into(),
            session: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub username: String,
    pub role: Role,
    pub tenant: String,
    pub created_at: i64,
    pub password_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub username: String,
    pub role: Role,
    pub tenant: String,
    pub expires_at: i64,
}

#[derive(Default, Serialize, Deserialize)]
struct AccountsFile {
    #[serde(default)]
    accounts: Vec<Account>,
}

#[derive(Default, Serialize, Deserialize)]
struct SessionsFile {
    #[serde(default)]
    sessions: BTreeMap<String, Session>,
}

pub fn accounts_path(root: &Root) -> PathBuf {
    root.dir.join("web-accounts.json")
}

pub fn sessions_path(root: &Root) -> PathBuf {
    root.dir.join("web-sessions.json")
}

pub fn load_accounts(root: &Root) -> Result<Vec<Account>> {
    let path = accounts_path(root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file: AccountsFile = store::read_json(&path)?;
    Ok(file.accounts)
}

fn save_accounts(root: &Root, accounts: &[Account]) -> Result<()> {
    let file = AccountsFile {
        accounts: accounts.to_vec(),
    };
    let text = serde_json::to_string_pretty(&file)?;
    agentvault_keystore::write_private(&accounts_path(root), &text)
}

fn load_sessions(root: &Root) -> Result<SessionsFile> {
    let path = sessions_path(root);
    if !path.exists() {
        return Ok(SessionsFile::default());
    }
    store::read_json(&path)
}

fn save_sessions(root: &Root, file: &SessionsFile) -> Result<()> {
    let text = serde_json::to_string_pretty(file)?;
    agentvault_keystore::write_private(&sessions_path(root), &text)
}

pub fn account_count(root: &Root) -> usize {
    load_accounts(root)
        .map(|accounts| accounts.len())
        .unwrap_or(0)
}

pub fn create_account(
    root: &Root,
    username: &str,
    password: &str,
    role: Role,
    tenant: Option<&str>,
) -> Result<Account> {
    let username = username.trim();
    if username.is_empty() {
        return Err(Error::Other("username is required".into()));
    }
    if username.chars().any(char::is_whitespace) || username.contains('/') {
        return Err(Error::Other(
            "username must not contain whitespace or '/'".into(),
        ));
    }
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(Error::Other(format!(
            "password must be at least {MIN_PASSWORD_CHARS} characters"
        )));
    }
    let tenant = tenant
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_TENANT);

    let mut accounts = load_accounts(root)?;
    if accounts.iter().any(|account| account.username == username) {
        return Err(Error::Other(format!("account {username} already exists")));
    }
    let account = Account {
        username: username.to_string(),
        role,
        tenant: tenant.to_string(),
        created_at: agentvault_core::now_ms(),
        password_hash: hash_password(password)?,
    };
    accounts.push(account.clone());
    save_accounts(root, &accounts)?;
    Ok(account)
}

pub fn remove_account(root: &Root, username: &str) -> Result<()> {
    let mut accounts = load_accounts(root)?;
    let index = accounts
        .iter()
        .position(|account| account.username == username)
        .ok_or_else(|| Error::NotFound(format!("account {username}")))?;
    let owners = accounts
        .iter()
        .filter(|account| account.role == Role::Owner)
        .count();
    if accounts[index].role == Role::Owner && owners <= 1 {
        return Err(Error::Other("cannot remove the last owner account".into()));
    }
    accounts.remove(index);
    save_accounts(root, &accounts)?;

    let mut sessions = load_sessions(root)?;
    sessions
        .sessions
        .retain(|_, session| session.username != username);
    save_sessions(root, &sessions)
}

pub fn login(root: &Root, username: &str, password: &str) -> Result<Option<(String, Session)>> {
    let username = username.trim();
    let accounts = load_accounts(root)?;
    let account = match accounts.iter().find(|account| account.username == username) {
        Some(account) => account,
        None => return Ok(None),
    };
    if !verify_password(&account.password_hash, password) {
        return Ok(None);
    }

    let now = agentvault_core::now_ms();
    let token = format!("{SESSION_PREFIX}{}", random_hex(32));
    let session = Session {
        username: account.username.clone(),
        role: account.role,
        tenant: account.tenant.clone(),
        expires_at: now + SESSION_TTL_MS,
    };
    let mut sessions = load_sessions(root)?;
    sessions.sessions.retain(|_, item| item.expires_at > now);
    sessions
        .sessions
        .insert(token_hash(&token), session.clone());
    save_sessions(root, &sessions)?;
    Ok(Some((token, session)))
}

pub fn lookup(root: &Root, token: &str) -> Result<Option<Session>> {
    if !token.starts_with(SESSION_PREFIX) {
        return Ok(None);
    }
    let sessions = load_sessions(root)?;
    let now = agentvault_core::now_ms();
    Ok(sessions
        .sessions
        .get(&token_hash(token))
        .filter(|session| session.expires_at > now)
        .cloned())
}

pub fn logout(root: &Root, token: &str) -> Result<bool> {
    let mut sessions = load_sessions(root)?;
    let removed = sessions.sessions.remove(&token_hash(token)).is_some();
    if removed {
        save_sessions(root, &sessions)?;
    }
    Ok(removed)
}

fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| Error::Other(format!("cannot hash password: {error}")))
}

fn verify_password(hash: &str, password: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

fn token_hash(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
