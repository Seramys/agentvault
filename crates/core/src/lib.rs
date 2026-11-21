//! Core domain types, token signing and local store for agentvault.

pub mod error;
pub mod model;
pub mod paths;
pub mod store;
pub mod token;
pub mod util;

pub use error::{Error, Result};
pub use model::{
    AgentBinding, Config, Honeypot, HoneypotStore, RevokedStore, SecretKind, SecretRecord,
    Settings, TicketClaims, TicketKind,
};
pub use paths::{default_data_dir, Root, RootSource};
pub use store::{read_json, write_json};
pub use token::TokenIssuer;
pub use util::{
    device_fingerprint, expand_path, hardware_fingerprint, home_dir, now_ms, parse_duration,
    random_hex, redact,
};
