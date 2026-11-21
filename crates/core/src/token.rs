use crate::error::{Error, Result};
use crate::model::TicketClaims;
use crate::util::{hmac_sha256, now_ms};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hkdf::Hkdf;
use sha2::Sha256;

pub const TOKEN_PREFIX: &str = "av1";

pub struct TokenIssuer {
    key: [u8; 32],
}

impl TokenIssuer {
    /// Derive the signing key from the master key and the current global epoch.
    /// Bumping the epoch (revoke --all / rotate) invalidates every old ticket.
    pub fn derive(master: &[u8; 32], epoch: u64) -> Self {
        let hk = Hkdf::<Sha256>::new(Some(b"agentvault-ticket"), master);
        let mut key = [0u8; 32];
        let info = format!("epoch:{epoch}");
        hk.expand(info.as_bytes(), &mut key)
            .expect("32 bytes is a valid hkdf output length");
        Self { key }
    }

    pub fn admin_token(master: &[u8; 32]) -> String {
        let hk = Hkdf::<Sha256>::new(Some(b"agentvault-admin"), master);
        let mut key = [0u8; 32];
        hk.expand(b"local-broker", &mut key)
            .expect("32 bytes is a valid hkdf output length");
        hex::encode(key)
    }

    pub fn issue(&self, claims: &TicketClaims) -> Result<String> {
        let payload = serde_json::to_vec(claims)?;
        let payload_b64 = URL_SAFE_NO_PAD.encode(&payload);
        let signing_input = format!("{TOKEN_PREFIX}.{payload_b64}");
        let sig = hmac_sha256(&self.key, signing_input.as_bytes());
        Ok(format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(sig)))
    }

    /// Decode the payload without checking the signature. Only use this to
    /// produce useful diagnostics (for example "globally revoked"); never to
    /// authorize an action.
    pub fn peek(token: &str) -> Option<TicketClaims> {
        let mut parts = token.split('.');
        let prefix = parts.next()?;
        let payload_b64 = parts.next()?;
        let _sig = parts.next()?;
        if parts.next().is_some() || prefix != TOKEN_PREFIX {
            return None;
        }
        let payload = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
        serde_json::from_slice(&payload).ok()
    }

    pub fn verify(&self, token: &str) -> Result<TicketClaims> {
        let mut parts = token.split('.');
        let prefix = parts.next().unwrap_or_default();
        let payload_b64 = parts.next().unwrap_or_default();
        let sig_b64 = parts.next().unwrap_or_default();
        if parts.next().is_some() {
            return Err(Error::Token("too many segments".into()));
        }
        if prefix != TOKEN_PREFIX {
            return Err(Error::Token(format!("unsupported version `{prefix}`")));
        }
        let expected = hmac_sha256(
            &self.key,
            format!("{TOKEN_PREFIX}.{payload_b64}").as_bytes(),
        );
        let provided = URL_SAFE_NO_PAD
            .decode(sig_b64)
            .map_err(|_| Error::Token("bad signature encoding".into()))?;
        if !constant_time_eq(&expected, &provided) {
            return Err(Error::Token("signature mismatch".into()));
        }
        let payload = URL_SAFE_NO_PAD
            .decode(payload_b64)
            .map_err(|_| Error::Token("bad payload encoding".into()))?;
        let claims: TicketClaims = serde_json::from_slice(&payload)?;
        if claims.v != 1 {
            return Err(Error::Token(format!(
                "unsupported claims version {}",
                claims.v
            )));
        }
        if claims.is_expired(now_ms()) {
            return Err(Error::Token("ticket expired".into()));
        }
        Ok(claims)
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::now_ms;

    fn claims(exp_offset_ms: i64) -> TicketClaims {
        let now = now_ms();
        TicketClaims {
            v: 1,
            jti: "jti-1".into(),
            sub: "claude-code".into(),
            sid: "sid-1".into(),
            secret: "postgres/prod".into(),
            scope: "read".into(),
            iat: now,
            exp: now + exp_offset_ms,
            epoch: 0,
            max_uses: 1,
            ..Default::default()
        }
    }

    #[test]
    fn issue_and_verify() {
        let master = [7u8; 32];
        let issuer = TokenIssuer::derive(&master, 0);
        let token = issuer.issue(&claims(60_000)).unwrap();
        let parsed = issuer.verify(&token).unwrap();
        assert_eq!(parsed.jti, "jti-1");
        assert_eq!(parsed.secret, "postgres/prod");
    }

    #[test]
    fn expired_ticket_rejected() {
        let master = [7u8; 32];
        let issuer = TokenIssuer::derive(&master, 0);
        let token = issuer.issue(&claims(-1_000)).unwrap();
        assert!(issuer.verify(&token).is_err());
    }

    #[test]
    fn tampered_payload_rejected() {
        let master = [7u8; 32];
        let issuer = TokenIssuer::derive(&master, 0);
        let token = issuer.issue(&claims(60_000)).unwrap();
        let tampered = format!("{}x", token);
        assert!(issuer.verify(&tampered).is_err());
    }

    #[test]
    fn epoch_bump_invalidates_old_ticket() {
        let master = [7u8; 32];
        let old = TokenIssuer::derive(&master, 0);
        let token = old.issue(&claims(60_000)).unwrap();
        let new = TokenIssuer::derive(&master, 1);
        assert!(new.verify(&token).is_err());
    }
}
