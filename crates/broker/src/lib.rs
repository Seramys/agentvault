//! Layer 1: local broker. It owns the long-lived secrets, signs short-lived
//! tickets, resolves them exactly once, rotates and revokes.

pub mod client;
pub mod server;

use agentvault_audit::{self as audit, AuditEvent};
use agentvault_core::store;
use agentvault_core::util::{now_ms, parse_duration, random_hex};
use agentvault_core::{
    AgentBinding, Config, Error, Honeypot, HoneypotStore, Result, RevokedStore, Root, SecretKind,
    SecretRecord, TicketClaims, TicketKind, TokenIssuer,
};
use agentvault_keystore::Keystore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use std::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TicketState {
    pub claims: TicketClaims,
    pub uses_left: u32,
}

#[derive(Serialize, Deserialize, Default)]
struct TicketFile {
    #[serde(default)]
    tickets: BTreeMap<String, TicketState>,
}

pub struct Broker {
    pub root: Root,
    pub keystore: Keystore,
    master: [u8; 32],
    admin: String,
    cfg: Mutex<Config>,
    revoked: Mutex<RevokedStore>,
    tickets: Mutex<BTreeMap<String, TicketState>>,
    honeypots: Mutex<HoneypotStore>,
    /// Decrypted secret values, loaded once at startup so a tool call never
    /// has to spawn PowerShell again.
    secrets: Mutex<BTreeMap<String, String>>,
}

impl Broker {
    pub fn open(root: Root) -> Result<Self> {
        root.ensure()?;
        let cfg = store::ensure_config(&root)?;
        cfg.check_hardware()?;

        let keystore = Keystore::open(&root.dir);
        let master = keystore.master_key()?;
        let admin = TokenIssuer::admin_token(&master);
        agentvault_keystore::write_private(&root.admin_token(), &admin)?;

        let names: Vec<String> = cfg.secrets.iter().map(|s| s.name.clone()).collect();
        let secrets = keystore.load_all(&names).unwrap_or_default();

        let revoked = store::load_revoked(&root).unwrap_or_default();
        let honeypots = store::load_honeypots(&root).unwrap_or_default();
        let tickets = load_tickets(&root).unwrap_or_default();

        Ok(Self {
            root,
            keystore,
            master,
            admin,
            cfg: Mutex::new(cfg),
            revoked: Mutex::new(revoked),
            tickets: Mutex::new(tickets),
            honeypots: Mutex::new(honeypots),
            secrets: Mutex::new(secrets),
        })
    }

    pub fn admin_token(&self) -> &str {
        &self.admin
    }

    pub fn backend(&self) -> &'static str {
        self.keystore.backend_name()
    }

    fn cfg(&self) -> std::sync::MutexGuard<'_, Config> {
        self.cfg.lock().expect("config mutex poisoned")
    }

    fn revoked(&self) -> std::sync::MutexGuard<'_, RevokedStore> {
        self.revoked.lock().expect("revoked mutex poisoned")
    }

    fn tickets(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, TicketState>> {
        self.tickets.lock().expect("tickets mutex poisoned")
    }

    fn honeypots(&self) -> std::sync::MutexGuard<'_, HoneypotStore> {
        self.honeypots.lock().expect("honeypots mutex poisoned")
    }

    fn secrets(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, String>> {
        self.secrets.lock().expect("secrets mutex poisoned")
    }

    fn persist_tickets(&self) -> Result<()> {
        let file = TicketFile {
            tickets: self.tickets().clone(),
        };
        store::write_json(&self.root.tickets(), &file)
    }

    pub fn issue(
        &self,
        secret_name: &str,
        sub: &str,
        ttl: Option<&str>,
        scope: &str,
        max_uses: Option<u32>,
        task: bool,
        hard_ttl: Option<&str>,
    ) -> Result<Value> {
        let cfg = self.cfg();
        if self.revoked().is_secret_revoked(secret_name) {
            return Err(Error::Denied(format!(
                "secret `{secret_name}` is revoked; rotate it or run `agentvault revoke --all` first"
            )));
        }
        let record = cfg
            .secret(secret_name)
            .ok_or_else(|| Error::NotFound(format!("secret `{secret_name}`")))?
            .clone();
        if record.honeypot {
            return Err(Error::Denied("honeypot secrets cannot be issued".into()));
        }
        drop(cfg);

        let ttl_secs = match ttl {
            Some(t) => parse_duration(t)?,
            None => self.cfg().settings.default_ttl_secs,
        };
        let now = now_ms();
        let exp = now + (ttl_secs as i64) * 1000;

        // A task ticket is a parent that cannot be resolved directly: it mints
        // short leaf tickets and can be renewed until its hard limit.
        let (kind, uses, exp, hard_exp, renewable) = if task {
            let hard_secs = match hard_ttl {
                Some(h) => parse_duration(h)?,
                None => 8 * 60 * 60,
            };
            let hard_exp = now + (hard_secs as i64) * 1000;
            // A task ticket must never live past its hard limit, even if the
            // caller asked for a longer sliding TTL.
            (TicketKind::Parent, 0u32, exp.min(hard_exp), hard_exp, true)
        } else {
            let uses = max_uses
                .unwrap_or_else(|| self.cfg().settings.default_max_uses)
                .max(1);
            (TicketKind::Leaf, uses, exp, exp, false)
        };

        let global_epoch = self.revoked().epoch;
        let claims = TicketClaims {
            v: 1,
            jti: uuid::Uuid::new_v4().to_string(),
            sub: sub.to_string(),
            sid: record.id.clone(),
            secret: record.name.clone(),
            scope: scope.to_string(),
            iat: now,
            exp,
            epoch: record.epoch,
            gepoch: global_epoch,
            max_uses: uses,
            kind: kind.clone(),
            parent: None,
            renewable,
            hard_exp,
            renewals: 0,
        };
        let issuer = TokenIssuer::derive(&self.master, global_epoch);
        let token = issuer.issue(&claims)?;

        {
            let mut tickets = self.tickets();
            tickets.insert(
                claims.jti.clone(),
                TicketState {
                    claims: claims.clone(),
                    uses_left: uses,
                },
            );
        }
        self.persist_tickets()?;

        audit::append(
            &self.root,
            &AuditEvent::new(audit::EV_TICKET_ISSUE, sub, "ok")
                .secret(&record.name)
                .ticket(&claims.jti)
                .meta(json!({
                    "ttl_secs": ttl_secs,
                    "max_uses": uses,
                    "scope": scope,
                    "kind": kind,
                    "hard_exp": hard_exp,
                })),
        )?;

        Ok(json!({
            "ticket": token,
            "jti": claims.jti,
            "exp": claims.exp,
            "ttl_secs": ttl_secs,
            "max_uses": uses,
            "scope": scope,
            "kind": kind,
            "hard_exp": hard_exp,
            "renewable": renewable,
        }))
    }

    pub fn resolve(&self, token: &str) -> Result<Value> {
        let global_epoch = self.revoked().epoch;
        let issuer = TokenIssuer::derive(&self.master, global_epoch);
        let claims = match issuer.verify(token) {
            Ok(claims) => claims,
            Err(error) => {
                let signature_mismatch = matches!(
                    &error,
                    Error::Token(message) if message == "signature mismatch"
                );
                if signature_mismatch {
                    if let Some(peeked) = TokenIssuer::peek(token) {
                        if peeked.gepoch < global_epoch {
                            audit::append(
                                &self.root,
                                &AuditEvent::new(audit::EV_TICKET_RESOLVE, "cli", "revoked")
                                    .secret(&peeked.secret)
                                    .ticket(&peeked.jti),
                            )?;
                            return Err(Error::Denied(
                                "ticket was invalidated by a global revocation (revoke --all)"
                                    .into(),
                            ));
                        }
                    }
                }
                return Err(error);
            }
        };
        if claims.kind == TicketKind::Parent {
            return Err(Error::Denied(
                "parent task tickets cannot resolve; issue a child ticket".into(),
            ));
        }
        let now = now_ms();

        if self.revoked().is_ticket_revoked(&claims.jti) {
            audit::append(
                &self.root,
                &AuditEvent::new(audit::EV_TICKET_RESOLVE, &claims.sub, "revoked")
                    .secret(&claims.secret)
                    .ticket(&claims.jti),
            )?;
            return Err(Error::Denied("ticket revoked".into()));
        }
        if self.revoked().is_secret_revoked(&claims.secret) {
            return Err(Error::Denied("secret revoked".into()));
        }

        let record = {
            let cfg = self.cfg();
            cfg.secret(&claims.secret)
                .ok_or_else(|| Error::NotFound(format!("secret `{}`", claims.secret)))?
                .clone()
        };
        if record.epoch != claims.epoch {
            return Err(Error::Denied("ticket predates a secret rotation".into()));
        }
        if record.honeypot {
            return Err(Error::Denied("honeypot secret".into()));
        }

        let uses_left = {
            let mut tickets = self.tickets();
            let state = tickets
                .get_mut(&claims.jti)
                .ok_or_else(|| Error::Denied("unknown ticket".into()))?;
            if state.uses_left == 0 {
                return Err(Error::Denied("ticket exhausted".into()));
            }
            state.uses_left -= 1;
            let left = state.uses_left;
            if left == 0 {
                let mut revoked = self.revoked();
                revoked.revoke_ticket(&claims.jti);
                store::save_revoked(&self.root, &revoked)?;
            }
            left
        };
        self.persist_tickets()?;

        let value = {
            let cached = self.secrets().get(&claims.secret).cloned();
            match cached {
                Some(value) => value,
                None => {
                    let Some(value) = self.keystore.get(&claims.secret)? else {
                        return Err(Error::NotFound(format!(
                            "stored value for `{}`",
                            claims.secret
                        )));
                    };
                    self.secrets().insert(claims.secret.clone(), value.clone());
                    value
                }
            }
        };

        // Honeypot tripwire: a canary value handed to an agent must never be
        // used for a real call. If it is, revoke everything immediately.
        let trip = {
            let mut honeypots = self.honeypots();
            honeypots.match_token(&value)
        };
        if let Some(hit) = trip {
            store::save_honeypots(&self.root, &self.honeypots())?;
            self.revoke_all()?;
            audit::append(
                &self.root,
                &AuditEvent::new(audit::EV_HONEYPOT_TRIGGERED, &claims.sub, "blocked")
                    .secret(&claims.secret)
                    .ticket(&claims.jti)
                    .meta(json!({ "honeypot": hit.name, "hits": hit.hits })),
            )?;
            return Err(Error::Denied(
                "honeypot token used; all tickets revoked".into(),
            ));
        }

        audit::append(
            &self.root,
            &AuditEvent::new(audit::EV_TICKET_RESOLVE, &claims.sub, "ok")
                .secret(&claims.secret)
                .ticket(&claims.jti)
                .meta(json!({ "uses_left": uses_left, "age_ms": now - claims.iat })),
        )?;

        Ok(json!({
            "value": value,
            "secret": claims.secret,
            "uses_left": uses_left,
            "exp": claims.exp,
        }))
    }

    /// Mint a short leaf ticket from a parent task ticket. This is how a long
    /// agent task keeps working: the parent lives for the task, each tool call
    /// gets a fresh leaf that is consumed exactly once.
    pub fn child(
        &self,
        parent_token: &str,
        ttl: Option<&str>,
        sub: Option<&str>,
        max_uses: Option<u32>,
    ) -> Result<Value> {
        let global_epoch = self.revoked().epoch;
        let issuer = TokenIssuer::derive(&self.master, global_epoch);
        let parent = issuer.verify(parent_token)?;
        if parent.kind != TicketKind::Parent {
            return Err(Error::Denied("not a parent task ticket".into()));
        }
        if self.revoked().is_ticket_revoked(&parent.jti)
            || self.revoked().is_secret_revoked(&parent.secret)
        {
            return Err(Error::Denied("parent ticket revoked".into()));
        }
        if parent.hard_exp > 0 && now_ms() >= parent.hard_exp {
            return Err(Error::Denied("parent task hard limit reached".into()));
        }
        {
            let tickets = self.tickets();
            if !tickets.contains_key(&parent.jti) {
                return Err(Error::Denied("unknown parent ticket".into()));
            }
        }

        let record = {
            let cfg = self.cfg();
            cfg.secret(&parent.secret)
                .ok_or_else(|| Error::NotFound(format!("secret `{}`", parent.secret)))?
                .clone()
        };
        if record.epoch != parent.epoch {
            return Err(Error::Denied("parent predates a secret rotation".into()));
        }
        if record.honeypot {
            return Err(Error::Denied("honeypot secret".into()));
        }

        let ttl_secs = match ttl {
            Some(t) => parse_duration(t)?,
            None => 60,
        };
        let uses = max_uses.unwrap_or(1).max(1);
        let now = now_ms();
        let ceiling = if parent.hard_exp > 0 {
            parent.hard_exp
        } else {
            parent.exp
        };
        let exp = (now + (ttl_secs as i64) * 1000).min(ceiling);
        if exp <= now {
            return Err(Error::Denied("parent task hard limit reached".into()));
        }
        let actual_ttl_secs = ((exp - now) / 1000).max(0) as u64;
        let claims = TicketClaims {
            v: 1,
            jti: uuid::Uuid::new_v4().to_string(),
            sub: sub.unwrap_or(&parent.sub).to_string(),
            sid: record.id.clone(),
            secret: record.name.clone(),
            scope: parent.scope.clone(),
            iat: now,
            exp,
            epoch: record.epoch,
            gepoch: global_epoch,
            max_uses: uses,
            kind: TicketKind::Leaf,
            parent: Some(parent.jti.clone()),
            renewable: false,
            hard_exp: 0,
            renewals: 0,
        };
        let token = issuer.issue(&claims)?;
        {
            let mut tickets = self.tickets();
            tickets.insert(
                claims.jti.clone(),
                TicketState {
                    claims: claims.clone(),
                    uses_left: uses,
                },
            );
        }
        self.persist_tickets()?;
        audit::append(
            &self.root,
            &AuditEvent::new(audit::EV_TICKET_ISSUE, &claims.sub, "ok")
                .secret(&record.name)
                .ticket(&claims.jti)
                .meta(json!({ "kind": "leaf", "parent": parent.jti, "ttl_secs": actual_ttl_secs })),
        )?;

        Ok(json!({
            "ticket": token,
            "jti": claims.jti,
            "exp": claims.exp,
            "ttl_secs": actual_ttl_secs,
            "requested_ttl_secs": ttl_secs,
            "capped_by_parent": actual_ttl_secs < ttl_secs,
            "max_uses": uses,
            "parent": parent.jti,
            "kind": "leaf",
        }))
    }

    /// Slide a parent ticket's expiry forward, capped by its hard limit.
    pub fn renew(&self, token: &str, ttl: Option<&str>) -> Result<Value> {
        let global_epoch = self.revoked().epoch;
        let issuer = TokenIssuer::derive(&self.master, global_epoch);
        let claims = issuer.verify(token)?;
        if claims.kind != TicketKind::Parent || !claims.renewable {
            return Err(Error::Denied(
                "only renewable task tickets can be renewed".into(),
            ));
        }
        if self.revoked().is_ticket_revoked(&claims.jti)
            || self.revoked().is_secret_revoked(&claims.secret)
        {
            return Err(Error::Denied("parent ticket revoked".into()));
        }

        let ttl_secs = match ttl {
            Some(t) => parse_duration(t)?,
            None => self.cfg().settings.default_ttl_secs,
        };
        let now = now_ms();

        let (new_claims, hard_exp, at_limit) = {
            let mut tickets = self.tickets();
            let state = tickets
                .get_mut(&claims.jti)
                .ok_or_else(|| Error::Denied("unknown parent ticket".into()))?;
            let hard = if state.claims.hard_exp > 0 {
                state.claims.hard_exp
            } else {
                state.claims.exp
            };
            let proposed = now + (ttl_secs as i64) * 1000;
            let mut new_exp = proposed.min(hard);
            if new_exp <= now {
                return Err(Error::Denied("task hard limit reached".into()));
            }
            if state.claims.exp > hard {
                // A ticket created before the hard limit was enforced: clamp
                // it down to the hard limit even if this renewal is shorter.
                new_exp = hard;
            } else if new_exp <= state.claims.exp {
                return Ok(json!({
                    "ticket": token,
                    "exp": state.claims.exp,
                    "hard_exp": hard,
                    "renewals": state.claims.renewals,
                    "note": "not shortened; existing expiry is later",
                }));
            }
            let mut updated = state.claims.clone();
            updated.exp = new_exp;
            updated.gepoch = global_epoch;
            updated.renewals = updated.renewals.saturating_add(1);
            state.claims = updated.clone();
            (updated, hard, false)
        };
        if at_limit {
            return Ok(json!({ "ticket": token, "exp": new_claims.exp }));
        }

        let renewed = issuer.issue(&new_claims)?;
        self.persist_tickets()?;
        audit::append(
            &self.root,
            &AuditEvent::new(audit::EV_TICKET_ISSUE, &new_claims.sub, "renewed")
                .secret(&new_claims.secret)
                .ticket(&new_claims.jti)
                .meta(json!({ "exp": new_claims.exp, "hard_exp": hard_exp, "renewals": new_claims.renewals })),
        )?;

        Ok(json!({
            "ticket": renewed,
            "jti": new_claims.jti,
            "exp": new_claims.exp,
            "hard_exp": hard_exp,
            "renewals": new_claims.renewals,
        }))
    }

    pub fn revoke_ticket(&self, jti: &str) -> Result<Value> {
        // Operators often have the token (printed by `issue`) rather than the
        // bare jti. Normalize it so `--ticket av1...` actually revokes.
        let jti = TokenIssuer::peek(jti)
            .map(|claims| claims.jti)
            .unwrap_or_else(|| jti.to_string());
        let jti = jti.as_str();
        let children: Vec<String> = {
            let tickets = self.tickets();
            tickets
                .values()
                .filter(|s| s.claims.parent.as_deref() == Some(jti))
                .map(|s| s.claims.jti.clone())
                .collect()
        };
        {
            let mut revoked = self.revoked();
            revoked.revoke_ticket(jti);
            for child in &children {
                revoked.revoke_ticket(child);
            }
            store::save_revoked(&self.root, &revoked)?;
        }
        {
            let mut tickets = self.tickets();
            tickets.remove(jti);
            for child in &children {
                tickets.remove(child);
            }
        }
        self.persist_tickets()?;
        audit::append(
            &self.root,
            &AuditEvent::new(audit::EV_TICKET_REVOKE, "cli", "ok")
                .ticket(jti)
                .meta(json!({ "children_revoked": children.len() })),
        )?;
        Ok(json!({ "revoked": jti, "children_revoked": children }))
    }

    pub fn revoke_secret(&self, name: &str) -> Result<Value> {
        {
            let mut revoked = self.revoked();
            revoked.revoke_secret(name);
            store::save_revoked(&self.root, &revoked)?;
        }
        {
            let mut cfg = self.cfg();
            if let Some(record) = cfg.secret_mut(name) {
                record.epoch = record.epoch.saturating_add(1);
                record.rotated_at = now_ms();
            }
            store::save_config(&self.root, &cfg)?;
        }
        // Drop every in-memory ticket for this secret.
        {
            let mut tickets = self.tickets();
            tickets.retain(|_, state| state.claims.secret != name);
        }
        self.persist_tickets()?;
        audit::append(
            &self.root,
            &AuditEvent::new(audit::EV_TICKET_REVOKE, "cli", "ok").secret(name),
        )?;
        Ok(json!({ "revoked_secret": name }))
    }

    pub fn revoke_all(&self) -> Result<Value> {
        {
            let mut revoked = self.revoked();
            revoked.revoke_all();
            store::save_revoked(&self.root, &revoked)?;
        }
        {
            let mut tickets = self.tickets();
            tickets.clear();
        }
        self.persist_tickets()?;
        audit::append(
            &self.root,
            &AuditEvent::new(audit::EV_REVOKE_ALL, "cli", "ok"),
        )?;
        Ok(json!({ "revoked": "all" }))
    }

    pub fn rotate(&self, name: &str) -> Result<Value> {
        let record = {
            let cfg = self.cfg();
            cfg.secret(name)
                .ok_or_else(|| Error::NotFound(format!("secret `{name}`")))?
                .clone()
        };
        let new_value = self.keystore.rotate(name)?;
        self.secrets().insert(name.to_string(), new_value.clone());
        {
            // Rotation is the recovery path for a secret-level revocation.
            let mut revoked = self.revoked();
            if revoked.secrets.remove(name) {
                revoked.updated_at = now_ms();
                store::save_revoked(&self.root, &revoked)?;
            }
        }
        {
            let mut cfg = self.cfg();
            if let Some(record) = cfg.secret_mut(name) {
                record.epoch = record.epoch.saturating_add(1);
                record.rotated_at = now_ms();
            }
            store::save_config(&self.root, &cfg)?;
        }
        {
            let mut tickets = self.tickets();
            tickets.retain(|_, state| state.claims.secret != name);
        }
        self.persist_tickets()?;
        audit::append(
            &self.root,
            &AuditEvent::new(audit::EV_SECRET_ROTATE, "cli", "ok")
                .secret(name)
                .meta(json!({ "previous_epoch": record.epoch })),
        )?;
        Ok(json!({
            "secret": name,
            "new_value": new_value,
            "previous_epoch": record.epoch,
            "note": "shown once; store it in the downstream system now",
        }))
    }

    pub fn bind_agent(
        &self,
        agent: &str,
        config_path: &str,
        servers: Vec<String>,
    ) -> Result<Value> {
        let mut cfg = self.cfg();
        cfg.agents.retain(|a| a.config_path != config_path);
        cfg.agents.push(AgentBinding {
            agent: agent.to_string(),
            config_path: config_path.to_string(),
            servers,
            bound_at: now_ms(),
        });
        store::save_config(&self.root, &cfg)?;
        Ok(json!({ "bound": config_path }))
    }

    pub fn add_secret(
        &self,
        name: &str,
        value: &str,
        kind: SecretKind,
        honeypot: bool,
    ) -> Result<Value> {
        if name.trim().is_empty() {
            return Err(Error::Other("secret name must not be empty".into()));
        }
        if name.len() > 255 || name.chars().any(char::is_control) {
            return Err(Error::Other(
                "secret name must be <=255 bytes and contain no control characters".into(),
            ));
        }
        if self.revoked().is_secret_revoked(name) {
            return Err(Error::Denied(format!(
                "secret `{name}` is revoked; rotate it or run `agentvault revoke --all` first"
            )));
        }
        let previous = self.secrets().get(name).cloned();
        self.keystore.set(name, value)?;
        self.secrets().insert(name.to_string(), value.to_string());
        {
            let mut cfg = self.cfg();
            let now = now_ms();
            match cfg.secret_mut(name) {
                // Re-adding an existing name updates the value but must not
                // reset its epoch/id: old tickets would otherwise become
                // valid again after a rotation.
                Some(record) => {
                    record.kind = kind;
                    record.honeypot = honeypot;
                    record.rotated_at = now;
                }
                None => cfg.upsert_secret(SecretRecord::new(name, kind, honeypot)),
            }
            store::save_config(&self.root, &cfg)?;
        }
        audit::append(
            &self.root,
            &AuditEvent::new(audit::EV_SECRET_ADD, "cli", "ok").secret(name),
        )?;
        Ok(json!({
            "secret": name,
            "stored": true,
            "replaced": previous.is_some(),
            "value_changed": previous
                .as_deref()
                .map(|old| old != value)
                .unwrap_or(false),
        }))
    }

    pub fn remove_secret(&self, name: &str) -> Result<Value> {
        self.keystore.delete(name)?;
        self.secrets().remove(name);
        {
            let mut cfg = self.cfg();
            cfg.secrets.retain(|s| s.name != name);
            store::save_config(&self.root, &cfg)?;
        }
        audit::append(
            &self.root,
            &AuditEvent::new(audit::EV_SECRET_REMOVE, "cli", "ok").secret(name),
        )?;
        Ok(json!({ "removed": name }))
    }

    pub fn unrevoke_secret(&self, name: &str) -> Result<Value> {
        let removed = {
            let mut revoked = self.revoked();
            let removed = revoked.secrets.remove(name);
            if removed {
                revoked.updated_at = now_ms();
                store::save_revoked(&self.root, &revoked)?;
            }
            removed
        };
        audit::append(
            &self.root,
            &AuditEvent::new("secret.unrevoke", "cli", "ok").secret(name),
        )?;
        Ok(json!({ "secret": name, "unrevoked": removed }))
    }

    pub fn status(&self) -> Result<Value> {
        let cfg = self.cfg();
        let secrets: Vec<Value> = cfg
            .secrets
            .iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "id": s.id,
                    "kind": s.kind,
                    "honeypot": s.honeypot,
                    "epoch": s.epoch,
                    "rotated_at": s.rotated_at,
                    "revoked": self.revoked().is_secret_revoked(&s.name),
                })
            })
            .collect();
        let now = now_ms();
        let agents: Vec<Value> = cfg
            .agents
            .iter()
            .map(|agent| {
                json!({
                    "agent": agent.agent,
                    "config_path": agent.config_path,
                    "servers": agent.servers,
                    "bound_at": agent.bound_at,
                })
            })
            .collect();
        let tickets: Vec<Value> = self
            .tickets()
            .values()
            .filter(|t| {
                t.claims.exp > now && (t.claims.kind == TicketKind::Parent || t.uses_left > 0)
            })
            .map(|t| {
                json!({
                    "jti": t.claims.jti,
                    "sub": t.claims.sub,
                    "secret": t.claims.secret,
                    "scope": t.claims.scope,
                    "kind": t.claims.kind,
                    "parent": t.claims.parent,
                    "exp": t.claims.exp,
                    "hard_exp": t.claims.hard_exp,
                    "renewals": t.claims.renewals,
                    "remaining_ms": t.claims.exp - now,
                    "uses_left": t.uses_left,
                })
            })
            .collect();
        Ok(json!({
            "home": self.root.dir.display().to_string(),
            "backend": self.backend(),
            "device": cfg.device_fingerprint.chars().take(8).collect::<String>(),
            "hardware": cfg
                .hardware_fp
                .as_deref()
                .unwrap_or("-")
                .chars()
                .take(8)
                .collect::<String>(),

            "secrets": secrets,
            "tickets": tickets,
            "agents": agents,
            "global_epoch": self.revoked().epoch,
            "honeypots": self.honeypots().items.len(),
        }))
    }

    pub fn audit_tail(&self, limit: usize) -> Result<Value> {
        let events = audit::tail(&self.root, limit)?;
        Ok(serde_json::to_value(events)?)
    }

    pub fn honeypot_add(&self, name: &str) -> Result<Value> {
        let token = format!("canary_{}", random_hex(16));
        let item: Honeypot = {
            let mut store = self.honeypots();
            let item = store.add(name, token.clone());
            store::save_honeypots(&self.root, &store)?;
            item
        };
        Ok(json!({
            "honeypot": item.name,
            "token": token,
            "usage": "put this value in a .env or config that an agent should never read; any real use trips the wire",
        }))
    }

    pub fn honeypot_list(&self) -> Result<Value> {
        Ok(json!(self.honeypots().items))
    }

    pub fn honeypot_check(&self, token: &str) -> Result<Value> {
        let hit = {
            let mut store = self.honeypots();
            let hit = store.match_token(token);
            if hit.is_some() {
                store::save_honeypots(&self.root, &store)?;
            }
            hit
        };
        match hit {
            Some(item) => {
                self.revoke_all()?;
                audit::append(
                    &self.root,
                    &AuditEvent::new(audit::EV_HONEYPOT_TRIGGERED, "cli", "blocked")
                        .secret(&item.name)
                        .meta(json!({ "hits": item.hits })),
                )?;
                Ok(json!({ "hit": true, "honeypot": item.name, "revoked_all": true }))
            }
            None => Ok(json!({ "hit": false })),
        }
    }

    /// Sweep expired tickets so `status` stays honest and the file does not grow
    /// without bound.
    pub fn sweep(&self) -> Result<usize> {
        let now = now_ms();
        let mut tickets = self.tickets();
        let before = tickets.len();
        tickets.retain(|_, state| {
            state.claims.exp > now
                && (state.claims.kind == TicketKind::Parent || state.uses_left > 0)
        });
        let removed = before - tickets.len();
        drop(tickets);
        if removed > 0 {
            self.persist_tickets()?;
        }
        Ok(removed)
    }
}

fn load_tickets(root: &Root) -> Result<BTreeMap<String, TicketState>> {
    let path = root.tickets();
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let file: TicketFile = store::read_json(&path)?;
    Ok(file.tickets)
}
