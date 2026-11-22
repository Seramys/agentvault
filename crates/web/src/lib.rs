//! Local web console for agentvault.
//!
//! axum + SSE + rust-embed. The React build (`webapp/dist`) is embedded at
//! compile time, so `agentvault web` is a single binary. `--dist` still works
//! for development against a live Vite output.

use agentvault_adapters::{AgentKind, McpConfig};
use agentvault_audit as audit;
use agentvault_broker::client;
use agentvault_core::store;
use agentvault_core::util::random_hex;
use agentvault_core::{Error, Result, Root, SecretKind, TicketClaims, TicketKind};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use futures_util::stream;

use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;
use std::time::Duration;

mod auth;

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

struct Ctx {
    root: Root,
    token: String,
    dist: Option<PathBuf>,
    backend: String,
}

#[derive(Deserialize, Default)]
struct TicketFile {
    #[serde(default)]
    tickets: BTreeMap<String, TicketState>,
}

#[derive(Deserialize)]
struct TicketState {
    claims: TicketClaims,
    #[serde(default)]
    uses_left: u32,
}

pub fn serve(root: Root, port: u16, dist: Option<PathBuf>) -> Result<()> {
    let host = bind_host();
    if !is_loopback_host(&host) {
        let allow_remote = std::env::var("AGENTVAULT_WEB_ALLOW_REMOTE")
            .map(|value| value.trim() == "1")
            .unwrap_or(false);
        if !allow_remote {
            return Err(Error::Other(format!(
                "refusing to bind {host}: set AGENTVAULT_WEB_ALLOW_REMOTE=1 to expose the console"
            )));
        }
        if auth::account_count(&root) == 0 {
            return Err(Error::Other(
                "remote bind requires at least one account; start on 127.0.0.1 and create an owner account first"
                    .into(),
            ));
        }
    }
    client::ensure_running(&root)?;
    let backend = agentvault_keystore::Keystore::open(&root.dir)
        .backend_name()
        .to_string();
    let token = random_hex(16);
    let token_path = root.dir.join("web.token");
    let _ = agentvault_keystore::write_private(&token_path, &token);
    let ctx = Arc::new(Ctx {
        root,
        token: token.clone(),
        dist: dist.or_else(|| std::env::var_os("AGENTVAULT_WEB_DIST").map(PathBuf::from)),
        backend,
    });

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Other(format!("cannot start async runtime: {e}")))?;

    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind((host.as_str(), port))
            .await
            .map_err(|e| Error::Other(format!("cannot bind {host}:{port}: {e}")))?;
        let actual = listener.local_addr()?;
        println!("agentvault web: http://{actual}/");
        println!("web token file: {}", token_path.display());
        println!("press Ctrl+C to stop");
        axum::serve(listener, router(ctx)).await?;
        Ok::<(), Error>(())
    })
}

fn bind_host() -> String {
    std::env::var("AGENTVAULT_WEB_BIND")
        .map(|value| value.trim().to_string())
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

fn is_loopback_host(host: &str) -> bool {
    let trimmed = host.trim().trim_start_matches('[').trim_end_matches(']');
    if trimmed.eq_ignore_ascii_case("localhost") {
        return true;
    }
    trimmed
        .parse::<std::net::IpAddr>()
        .map(|address| address.is_loopback())
        .unwrap_or(false)
}

fn router(ctx: Arc<Ctx>) -> Router {
    let api = Router::new()
        .route("/status", get(h_status))
        .route("/overview", get(h_overview))
        .route("/secrets", get(h_secrets).post(h_add_secret))
        .route("/secrets/:name/rotate", post(h_rotate))
        .route("/secrets/:name", delete(h_remove_secret))
        .route("/tickets", get(h_tickets).post(h_issue))
        .route("/tickets/:id/renew", post(h_renew))
        .route("/tickets/:id", delete(h_revoke_ticket))
        .route("/revoke-all", post(h_revoke_all))
        .route("/agents", get(h_agents))
        .route("/agents/candidates", get(h_candidates))
        .route("/agents/plan", post(h_agent_plan))
        .route("/agents/init", post(h_agents_init))
        .route("/audit", get(h_audit))
        .route("/honeypots", get(h_honeypots).post(h_honeypot_add))
        .route("/honeypots/:id/trigger", post(h_honeypot_trigger))
        .route("/honeypots/:id", delete(h_honeypot_delete))
        .layer(middleware::from_fn_with_state(ctx.clone(), auth_middleware));

    let auth_api = Router::new()
        .route("/auth/me", get(h_me))
        .route("/auth/accounts", get(h_accounts).post(h_add_account))
        .route("/auth/accounts/:username", delete(h_delete_account))
        .layer(middleware::from_fn_with_state(ctx.clone(), auth_middleware));

    let public_api = Router::new()
        .route("/auth/login", post(h_login))
        .route("/auth/logout", post(h_logout));

    Router::new()
        .route("/api/events", get(h_sse))
        .nest("/api", api.merge(auth_api).merge(public_api))
        .fallback(static_handler)
        .with_state(ctx)
}

/* ------------------------------ auth ------------------------------ */

fn bearer(headers: &HeaderMap) -> &str {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or_default()
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "ok": false, "error": "missing or invalid session token" })),
    )
        .into_response()
}

async fn auth_middleware(
    State(ctx): State<Arc<Ctx>>,
    headers: HeaderMap,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    let provided = bearer(&headers);
    let identity = if provided == ctx.token {
        Some(auth::Identity::bootstrap())
    } else {
        match auth::lookup(&ctx.root, provided) {
            Ok(Some(session)) => Some(auth::Identity {
                username: session.username,
                role: session.role,
                tenant: session.tenant,
                session: Some(provided.to_string()),
            }),
            Ok(None) => None,
            Err(error) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "ok": false, "error": error.to_string() })),
                )
                    .into_response()
            }
        }
    };
    let Some(identity) = identity else {
        return unauthorized();
    };

    let method = request.method().clone();
    if !identity.role.can_write()
        && method != axum::http::Method::GET
        && method != axum::http::Method::HEAD
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "ok": false, "error": "viewer accounts are read-only" })),
        )
            .into_response();
    }

    request.extensions_mut().insert(identity);
    next.run(request).await
}

fn api_ok(result: Result<Value>) -> Response {
    match result {
        Ok(value) => Json(json!({ "ok": true, "data": value })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": error.to_string() })),
        )
            .into_response(),
    }
}

/* ------------------------------ handlers ------------------------------ */

async fn h_status(State(ctx): State<Arc<Ctx>>) -> Response {
    api_ok(api_status(&ctx))
}

async fn h_overview(State(ctx): State<Arc<Ctx>>) -> Response {
    api_ok(api_overview(&ctx))
}

async fn h_secrets(State(ctx): State<Arc<Ctx>>) -> Response {
    api_ok(api_secrets(&ctx))
}

async fn h_add_secret(State(ctx): State<Arc<Ctx>>, Json(body): Json<Value>) -> Response {
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    api_ok(api_add_secret(&ctx, &bytes))
}

async fn h_rotate(State(ctx): State<Arc<Ctx>>, Path(name): Path<String>) -> Response {
    api_ok(api_rotate(&ctx, &name))
}

async fn h_remove_secret(State(ctx): State<Arc<Ctx>>, Path(name): Path<String>) -> Response {
    api_ok(api_remove_secret(&ctx, &name))
}

async fn h_tickets(State(ctx): State<Arc<Ctx>>) -> Response {
    api_ok(api_tickets(&ctx))
}

async fn h_issue(State(ctx): State<Arc<Ctx>>, Json(body): Json<Value>) -> Response {
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    api_ok(api_issue(&ctx, &bytes))
}

async fn h_candidates() -> Response {
    api_ok(api_candidates())
}

async fn h_agent_plan(State(ctx): State<Arc<Ctx>>, Json(body): Json<Value>) -> Response {
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    api_ok(api_agent_plan(&ctx, &bytes))
}

async fn h_renew(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>) -> Response {
    api_ok(api_renew(&ctx, &id))
}

async fn h_revoke_ticket(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>) -> Response {
    api_ok(api_revoke_ticket(&ctx, &id))
}

async fn h_revoke_all(State(ctx): State<Arc<Ctx>>, Json(body): Json<Value>) -> Response {
    api_ok(api_revoke_all(&ctx, &body))
}

async fn h_agents(State(ctx): State<Arc<Ctx>>) -> Response {
    api_ok(api_agents(&ctx))
}

async fn h_agents_init(State(ctx): State<Arc<Ctx>>, Json(body): Json<Value>) -> Response {
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    api_ok(api_agents_init(&ctx, &bytes))
}

async fn h_audit(
    State(ctx): State<Arc<Ctx>>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let limit = query
        .get("limit")
        .and_then(|value| value.parse().ok())
        .unwrap_or(100usize);
    let event = query.get("event").cloned();
    let since = query
        .get("since")
        .and_then(|value| value.parse::<i64>().ok());
    match audit::tail(&ctx.root, 10_000) {
        Ok(events) => {
            let filtered: Vec<agentvault_audit::AuditEvent> = events
                .into_iter()
                .filter(|item| {
                    event
                        .as_deref()
                        .map(|prefix| item.event.contains(prefix))
                        .unwrap_or(true)
                })
                .filter(|item| since.map(|since| item.ts >= since).unwrap_or(true))
                .collect();
            let start = filtered.len().saturating_sub(limit);
            api_ok(Ok(
                serde_json::to_value(&filtered[start..]).unwrap_or(Value::Null)
            ))
        }
        Err(error) => api_ok(Err(error)),
    }
}

async fn h_honeypots(State(ctx): State<Arc<Ctx>>) -> Response {
    api_ok(api_honeypots(&ctx))
}

async fn h_honeypot_add(State(ctx): State<Arc<Ctx>>, Json(body): Json<Value>) -> Response {
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    api_ok(api_honeypot_add(&ctx, &bytes))
}

async fn h_honeypot_trigger(
    State(ctx): State<Arc<Ctx>>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    api_ok(api_honeypot_trigger(&ctx, &id, &body))
}

async fn h_honeypot_delete(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>) -> Response {
    api_ok(api_honeypot_delete(&ctx, &id))
}

/* ------------------------------ auth handlers ------------------------------ */

fn account_json(account: &auth::Account) -> Value {
    json!({
        "username": account.username,
        "role": account.role.as_str(),
        "tenant": account.tenant,
        "created_at": account.created_at,
    })
}

fn require_owner(identity: &auth::Identity) -> Option<Response> {
    if identity.role == auth::Role::Owner {
        None
    } else {
        Some(
            (
                StatusCode::FORBIDDEN,
                Json(json!({ "ok": false, "error": "owner role required" })),
            )
                .into_response(),
        )
    }
}

async fn h_login(State(ctx): State<Arc<Ctx>>, Json(body): Json<Value>) -> Response {
    let username = body
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let password = body
        .get("password")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if username.trim().is_empty() || password.is_empty() {
        return unauthorized();
    }
    match auth::login(&ctx.root, username, password) {
        Ok(Some((token, session))) => Json(json!({
            "ok": true,
            "data": {
                "token": token,
                "role": session.role.as_str(),
                "tenant": session.tenant,
                "expires_at": session.expires_at,
            }
        }))
        .into_response(),
        Ok(None) => unauthorized(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn h_logout(State(ctx): State<Arc<Ctx>>, headers: HeaderMap) -> Response {
    let provided = bearer(&headers);
    if provided.starts_with(auth::SESSION_PREFIX) {
        let _ = auth::logout(&ctx.root, provided);
    }
    Json(json!({ "ok": true, "data": { "ok": true } })).into_response()
}

async fn h_me(Extension(identity): Extension<auth::Identity>) -> Response {
    Json(json!({
        "ok": true,
        "data": {
            "username": identity.username,
            "role": identity.role.as_str(),
            "tenant": identity.tenant,
        }
    }))
    .into_response()
}

async fn h_accounts(
    State(ctx): State<Arc<Ctx>>,
    Extension(identity): Extension<auth::Identity>,
) -> Response {
    if let Some(response) = require_owner(&identity) {
        return response;
    }
    match auth::load_accounts(&ctx.root) {
        Ok(accounts) => {
            let items: Vec<Value> = accounts.iter().map(account_json).collect();
            Json(json!({ "ok": true, "data": items })).into_response()
        }
        Err(error) => api_ok(Err(error)),
    }
}

async fn h_add_account(
    State(ctx): State<Arc<Ctx>>,
    Extension(identity): Extension<auth::Identity>,
    Json(body): Json<Value>,
) -> Response {
    if let Some(response) = require_owner(&identity) {
        return response;
    }
    let username = body
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let password = body
        .get("password")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let role = match body.get("role").and_then(Value::as_str) {
        Some(value) => match auth::Role::parse(value) {
            Ok(role) => role,
            Err(error) => return api_ok(Err(error)),
        },
        None => auth::Role::Viewer,
    };
    let tenant = body.get("tenant").and_then(Value::as_str);
    match auth::create_account(&ctx.root, username, password, role, tenant) {
        Ok(account) => Json(json!({ "ok": true, "data": account_json(&account) })).into_response(),
        Err(error) => api_ok(Err(error)),
    }
}

async fn h_delete_account(
    State(ctx): State<Arc<Ctx>>,
    Extension(identity): Extension<auth::Identity>,
    Path(username): Path<String>,
) -> Response {
    if let Some(response) = require_owner(&identity) {
        return response;
    }
    match auth::remove_account(&ctx.root, &username) {
        Ok(()) => Json(json!({ "ok": true, "data": { "removed": username } })).into_response(),
        Err(error) => api_ok(Err(error)),
    }
}

async fn h_sse(State(ctx): State<Arc<Ctx>>, headers: HeaderMap) -> Response {
    let provided = bearer(&headers);
    let authorized =
        provided == ctx.token || matches!(auth::lookup(&ctx.root, provided), Ok(Some(_)));
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let root = ctx.root.clone();
    let stream = stream::unfold(
        (root, audit_signature(&ctx.root)),
        |(root, last)| async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let current = audit_signature(&root);
            let event = if current != last {
                Event::default().event("refresh").data("{}")
            } else {
                Event::default().comment("ping")
            };
            Some((Ok::<Event, Infallible>(event), (root, current)))
        },
    );
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

async fn static_handler(State(ctx): State<Arc<Ctx>>, uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(dist) = &ctx.dist {
        if let Some(response) = read_dist(dist, path) {
            return response;
        }
    }
    if let Some((_, bytes)) = EMBEDDED.iter().find(|(name, _)| *name == path) {
        return asset_response(path, bytes.to_vec());
    }
    if let Some((_, bytes)) = EMBEDDED.iter().find(|(name, _)| *name == "index.html") {
        return asset_response("index.html", bytes.to_vec());
    }
    (
        StatusCode::NOT_FOUND,
        "console assets not built; run `npm run build` in webapp/",
    )
        .into_response()
}

fn read_dist(dist: &FsPath, path: &str) -> Option<Response> {
    let full = dist.join(path);
    if let Ok(bytes) = std::fs::read(&full) {
        return Some(asset_response(path, bytes));
    }
    if !path.contains('.') {
        if let Ok(bytes) = std::fs::read(dist.join("index.html")) {
            return Some(asset_response("index.html", bytes));
        }
    }
    None
}

fn asset_response(path: &str, bytes: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    ([(header::CONTENT_TYPE, mime.as_ref())], bytes).into_response()
}

/* ------------------------------ API ------------------------------ */

fn api_status(ctx: &Ctx) -> Result<Value> {
    let cfg = store::load_config(&ctx.root)?;
    let revoked = store::load_revoked(&ctx.root).unwrap_or_default();
    let ping = client::ping(&ctx.root).ok();
    Ok(json!({
        "backend": ctx.backend,
        "device": cfg.device_fingerprint.chars().take(8).collect::<String>(),
        "hardware": cfg.hardware_fp.as_deref().unwrap_or("-").chars().take(8).collect::<String>(),
        "epoch": revoked.epoch,
        "broker": ping.is_some(),
        "secrets": cfg.secrets.len(),
        "agents": cfg.agents.len(),
    }))
}

fn api_secrets(ctx: &Ctx) -> Result<Value> {
    let cfg = store::load_config(&ctx.root)?;
    let secrets: Vec<Value> = cfg
        .secrets
        .iter()
        .map(|secret| {
            let agents: Vec<String> = cfg
                .agents
                .iter()
                .filter(|agent| agent.servers.iter().any(|server| server == &secret.name))
                .map(|agent| agent.agent.clone())
                .collect();
            json!({
                "name": secret.name,
                "kind": secret.kind,
                "epoch": secret.epoch,
                "rotated_at": secret.rotated_at,
                "created_at": secret.created_at,
                "honeypot": secret.honeypot,
                "agents": agents,
                "note": secret.note,
            })
        })
        .collect();
    Ok(json!(secrets))
}

fn api_rotate(ctx: &Ctx, name: &str) -> Result<Value> {
    client::request(&ctx.root, "rotate", json!({ "secret": name }))
}

fn api_remove_secret(ctx: &Ctx, name: &str) -> Result<Value> {
    client::request(&ctx.root, "secret_remove", json!({ "name": name }))
}

fn api_add_secret(ctx: &Ctx, body: &[u8]) -> Result<Value> {
    let request: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let name = request
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Other("name is required".into()))?;
    let value = request
        .get("value")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Other("value is required".into()))?;
    let kind = request
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("token");
    client::request(
        &ctx.root,
        "secret_add",
        json!({ "name": name, "value": value, "kind": kind }),
    )
}

fn api_tickets(ctx: &Ctx) -> Result<Value> {
    let path = ctx.root.tickets();
    let file: TicketFile = if path.exists() {
        store::read_json(&path).unwrap_or_default()
    } else {
        TicketFile::default()
    };
    let now = agentvault_core::now_ms();
    let mut tickets: Vec<Value> = file
        .tickets
        .values()
        .map(|state| {
            let claims = &state.claims;
            let total = (claims.exp - claims.iat).max(1);
            json!({
                "id": claims.jti,
                "kind": claims.kind,
                "secret": claims.secret,
                "sub": claims.sub,
                "parent": claims.parent,
                "exp": claims.exp,
                "iat": claims.iat,
                "total": total,
                "uses": state.uses_left,
                "renewals": claims.renewals,
                "scope": claims.scope,
                "status": if claims.exp <= now { "expired" } else { "active" },
            })
        })
        .collect();
    tickets.sort_by(|a, b| {
        let ka = a.get("kind").and_then(Value::as_str).unwrap_or("");
        let kb = b.get("kind").and_then(Value::as_str).unwrap_or("");
        let pa = if ka == "parent" { 0 } else { 1 };
        let pb = if kb == "parent" { 0 } else { 1 };
        pa.cmp(&pb)
    });
    Ok(json!(tickets))
}

fn api_issue(ctx: &Ctx, body: &[u8]) -> Result<Value> {
    let request: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let secret = request
        .get("secret")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Other("secret is required".into()))?;
    let ttl = request.get("ttl").and_then(Value::as_str).unwrap_or("15m");
    let scope = request
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("read");
    let max_uses = request.get("max_uses").and_then(Value::as_u64).unwrap_or(1);
    let mut payload = json!({
        "secret": secret,
        "sub": "web",
        "ttl": ttl,
        "scope": scope,
        "max_uses": max_uses,
    });
    if request
        .get("task")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        payload["task"] = json!(true);
        payload["hard_ttl"] = request
            .get("hard_ttl")
            .cloned()
            .unwrap_or_else(|| json!("1h"));
    }
    client::request(&ctx.root, "issue", payload)
}

fn api_candidates() -> Result<Value> {
    let kinds = [
        AgentKind::Claude,
        AgentKind::Cursor,
        AgentKind::Codex,
        AgentKind::Gemini,
        AgentKind::Windsurf,
        AgentKind::Cline,
        AgentKind::Qoder,
        AgentKind::Generic,
    ];
    let agents: Vec<Value> = kinds
        .iter()
        .map(|kind| {
            let paths: Vec<Value> = agentvault_adapters::candidate_paths(*kind)
                .into_iter()
                .map(|path| {
                    let resolved = agentvault_adapters::resolve_target(&path);
                    json!({
                        "path": resolved.display().to_string(),
                        "exists": resolved.exists(),
                    })
                })
                .collect();
            json!({
                "id": kind.as_str(),
                "paths": paths,
                "supports_toml": kind.supports_toml(),
            })
        })
        .collect();
    Ok(json!(agents))
}

fn api_agent_plan(_ctx: &Ctx, body: &[u8]) -> Result<Value> {
    let request: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let agent = request
        .get("agent")
        .and_then(Value::as_str)
        .unwrap_or("generic");
    let kind = AgentKind::parse(agent).unwrap_or(AgentKind::Generic);
    let path = request
        .get("path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or_else(|| agentvault_adapters::first_existing(kind))
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_default()
                .join(".mcp.json")
        });
    let path = agentvault_adapters::resolve_target(&path);
    if !path.exists() {
        return Err(Error::NotFound(format!("config {}", path.display())));
    }
    if path.extension().and_then(|extension| extension.to_str()) == Some("toml") {
        let codex = agentvault_adapters::codex::CodexConfig::load(&path)?;
        let changes: Vec<Value> = codex
            .plan()
            .iter()
            .map(|change| {
                json!({
                    "server": change.server,
                    "before": change.before,
                    "migrated_env": change.migrated_env,
                    "remaining_env": change.remaining_env,
                })
            })
            .collect();
        return Ok(json!({
            "path": path.display().to_string(),
            "format": "toml",
            "changes": changes,
            "skipped": [],
        }));
    }
    let config = McpConfig::load(&path)?;
    let plan = agentvault_adapters::plan(&config);
    let changes: Vec<Value> = plan
        .changes
        .iter()
        .map(|change| {
            json!({
                "server": change.server,
                "before": change.before,
                "after": change.after,
                "migrated_env": change.migrated_env,
                "kept_env": change.kept_env,
                "remaining_env": change.remaining_env,
            })
        })
        .collect();
    Ok(json!({
        "path": path.display().to_string(),
        "format": "json",
        "changes": changes,
        "skipped": plan.skipped,
    }))
}

fn require_confirm(body: &Value) -> Result<()> {
    if body
        .get("confirm")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err(Error::Other("confirm=true is required".into()))
    }
}

fn api_renew(ctx: &Ctx, id: &str) -> Result<Value> {
    client::request(&ctx.root, "renew", json!({ "ticket": id }))
}

fn api_revoke_ticket(ctx: &Ctx, id: &str) -> Result<Value> {
    client::request(&ctx.root, "revoke", json!({ "ticket": id }))
}

fn api_revoke_all(ctx: &Ctx, body: &Value) -> Result<Value> {
    require_confirm(body)?;
    client::request(&ctx.root, "revoke", json!({ "all": true }))
}

fn api_agents(ctx: &Ctx) -> Result<Value> {
    let cfg = store::load_config(&ctx.root)?;
    let kinds = [
        AgentKind::Claude,
        AgentKind::Cursor,
        AgentKind::Codex,
        AgentKind::Gemini,
        AgentKind::Windsurf,
        AgentKind::Cline,
    ];
    let agents: Vec<Value> = kinds
        .iter()
        .map(|kind| {
            let bound = cfg.agents.iter().find(|agent| agent.agent == kind.as_str());
            let path = bound
                .map(|agent| agent.config_path.clone())
                .or_else(|| {
                    agentvault_adapters::first_existing(*kind)
                        .map(|path| path.display().to_string())
                })
                .unwrap_or_default();
            json!({
                "id": kind.as_str(),
                "path": path,
                "wrapped": bound.is_some(),
                "servers": bound.map(|agent| agent.servers.len()).unwrap_or(0),
                "supports_toml": kind.supports_toml(),
            })
        })
        .collect();
    Ok(json!(agents))
}

fn api_agents_init(ctx: &Ctx, body: &[u8]) -> Result<Value> {
    let launcher = launcher_path();
    let request: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    require_confirm(&request)?;
    let agent = request
        .get("agent")
        .and_then(Value::as_str)
        .unwrap_or("generic");
    let kind = AgentKind::parse(agent).unwrap_or(AgentKind::Generic);
    let path = request
        .get("path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or_else(|| agentvault_adapters::first_existing(kind))
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_default()
                .join(".mcp.json")
        });
    let path = agentvault_adapters::resolve_target(&path);
    if !path.exists() {
        return Err(Error::NotFound(format!("config {}", path.display())));
    }

    if path.extension().and_then(|extension| extension.to_str()) == Some("toml") {
        let mut codex = agentvault_adapters::codex::CodexConfig::load(&path)?;
        let plan = codex.plan();
        for change in &plan {
            for (key, value) in &change.migrated_env {
                let name = format!("{}/{}", change.server, key);
                client::request(
                    &ctx.root,
                    "secret_add",
                    json!({ "name": name, "value": value, "kind": "env" }),
                )?;
            }
        }
        let backup = agentvault_backup::create(&path, &ctx.root)?;
        let changes = codex.apply_with_launcher(&launcher, Some(ctx.root.dir.as_path()));
        codex.save(&path)?;
        let reloaded = agentvault_adapters::codex::CodexConfig::load(&path)?;
        let failed = changes.iter().any(|change| {
            reloaded
                .servers
                .iter()
                .find(|server| server.name == change.server)
                .map(|server| !agentvault_adapters::codex::CodexConfig::is_wrapped(server))
                .unwrap_or(true)
        });
        if failed {
            let _ = agentvault_backup::restore(&backup, &path, &ctx.root);
            return Err(Error::Other(
                "Codex config did not contain wrapped servers after rewrite".into(),
            ));
        }
        println!("wrote:        {}", path.display());
        println!("backup:       {}", backup.display());
        let servers: Vec<String> = changes.iter().map(|change| change.server.clone()).collect();
        client::request(
            &ctx.root,
            "bind_agent",
            json!({
                "agent": kind.as_str(),
                "config_path": path.display().to_string(),
                "servers": servers,
            }),
        )?;
        return Ok(json!({
            "wrapped": changes.len(),
            "path": path.display().to_string(),
            "backup": backup.display().to_string(),
        }));
    }

    let mut mcp = McpConfig::load(&path)?;
    let plan = agentvault_adapters::plan(&mcp);
    for change in &plan.changes {
        for key in &change.migrated_env {
            let name = format!("{}/{}", change.server, key);
            if let Some(value) = mcp
                .mcp_servers
                .get(&change.server)
                .and_then(|server| server.env.get(key))
            {
                client::request(
                    &ctx.root,
                    "secret_add",
                    json!({ "name": name, "value": value, "kind": "env" }),
                )?;
            }
        }
    }
    let backup = agentvault_backup::create(&path, &ctx.root)?;
    let changes =
        agentvault_adapters::apply_with_launcher(&mut mcp, &launcher, Some(ctx.root.dir.as_path()));
    mcp.save(&path)?;
    let names: Vec<String> = changes.iter().map(|change| change.server.clone()).collect();
    if let Err(error) = agentvault_adapters::verify_wrapped(&path, &names) {
        let _ = agentvault_backup::restore(&backup, &path, &ctx.root);
        return Err(error);
    }
    println!("wrote:        {}", path.display());
    println!("backup:       {}", backup.display());
    let servers: Vec<String> = changes.iter().map(|change| change.server.clone()).collect();
    client::request(
        &ctx.root,
        "bind_agent",
        json!({
            "agent": kind.as_str(),
            "config_path": path.display().to_string(),
            "servers": servers,
        }),
    )?;
    Ok(json!({
        "wrapped": changes.len(),
        "path": path.display().to_string(),
        "backup": backup.display().to_string(),
    }))
}

fn launcher_path() -> String {
    std::env::current_exe()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "agentvault".to_string())
}

fn api_overview(ctx: &Ctx) -> Result<Value> {
    let cfg = store::load_config(&ctx.root)?;
    let events = audit::tail(&ctx.root, 2000)?;
    let now = agentvault_core::now_ms();
    let since = now - 24 * 3600 * 1000;
    let recent: Vec<&agentvault_audit::AuditEvent> =
        events.iter().filter(|event| event.ts >= since).collect();

    let issued = recent
        .iter()
        .filter(|event| event.event == audit::EV_TICKET_ISSUE)
        .count() as i64;
    let resolved = recent
        .iter()
        .filter(|event| event.event == audit::EV_TICKET_RESOLVE)
        .count() as i64;
    let denied = recent
        .iter()
        .filter(|event| event.result == "denied" || event.event == audit::EV_HONEYPOT_TRIGGERED)
        .count() as i64;

    let mut trend_issued = vec![0i64; 24];
    let mut trend_resolved = vec![0i64; 24];
    let mut heat = [0i64; 24];
    for event in &recent {
        let index = (((event.ts - since) / (3600 * 1000)) as usize).min(23);
        if event.event == audit::EV_TICKET_ISSUE {
            trend_issued[index] += 1;
        }
        if event.event == audit::EV_TICKET_RESOLVE {
            trend_resolved[index] += 1;
        }
        let hour = (((event.ts % 86_400_000) / 3_600_000) as usize).min(23);
        heat[hour] += 1;
    }
    let max_heat = heat.iter().copied().max().unwrap_or(1).max(1);
    let heat_levels: Vec<u8> = heat
        .iter()
        .map(|value| {
            if *value == 0 {
                0
            } else {
                ((*value as f64 / max_heat as f64) * 4.0).ceil() as u8
            }
        })
        .map(|value| value.min(4))
        .collect();

    let total = cfg.secrets.len().max(1);
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for secret in &cfg.secrets {
        let key = match &secret.kind {
            SecretKind::Env => "env",
            SecretKind::Token => "token",
            SecretKind::Json => "json",
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    let kinds: Vec<Value> = ["env", "token", "json"]
        .iter()
        .map(|key| {
            let count = *counts.get(key).unwrap_or(&0);
            json!({
                "label": key.to_uppercase(),
                "value": (count * 100 / total) as i64,
                "count": count,
            })
        })
        .collect();

    let mut usage: BTreeMap<String, i64> = BTreeMap::new();
    for event in &recent {
        if event.event == audit::EV_TICKET_RESOLVE {
            *usage.entry(event.actor.clone()).or_insert(0) += 1;
        }
    }
    let usage_total: i64 = usage.values().copied().sum::<i64>().max(1);
    let mut usage_list: Vec<Value> = usage
        .iter()
        .map(|(label, count)| json!({ "label": label, "value": count * 100 / usage_total }))
        .collect();
    usage_list.sort_by(|a, b| {
        b.get("value")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .cmp(&a.get("value").and_then(Value::as_i64).unwrap_or(0))
    });
    usage_list.truncate(4);

    let live = api_tickets(ctx)?
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|ticket| ticket.get("status").and_then(Value::as_str) == Some("active"))
                .count()
        })
        .unwrap_or(0);

    Ok(json!({
        "secrets": cfg.secrets.len(),
        "live": live,
        "issued": issued,
        "resolved": resolved,
        "denied": denied,
        "trend": { "issued": trend_issued, "resolved": trend_resolved },
        "kinds": kinds,
        "usage": usage_list,
        "heat": heat_levels,
        "epoch": store::load_revoked(&ctx.root).map(|revoked| revoked.epoch).unwrap_or(0),
    }))
}

fn api_honeypots(ctx: &Ctx) -> Result<Value> {
    let honeypots = store::load_honeypots(&ctx.root).unwrap_or_default();
    let items: Vec<Value> = honeypots
        .items
        .iter()
        .map(|honeypot| {
            json!({
                "id": honeypot.id,
                "name": honeypot.name,
                "token": honeypot.token,
                "hits": honeypot.hits,
                "status": if honeypot.triggered_at.is_some() { "triggered" } else { "armed" },
                "created_at": honeypot.created_at,
                "triggered_at": honeypot.triggered_at,
            })
        })
        .collect();
    Ok(json!(items))
}

fn api_honeypot_add(ctx: &Ctx, body: &[u8]) -> Result<Value> {
    let request: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let name = request
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Other("name is required".into()))?;
    client::request(&ctx.root, "honeypot_add", json!({ "name": name }))
}

fn api_honeypot_trigger(ctx: &Ctx, id: &str, body: &Value) -> Result<Value> {
    require_confirm(body)?;
    let honeypots = store::load_honeypots(&ctx.root).unwrap_or_default();
    let item = honeypots
        .items
        .iter()
        .find(|honeypot| honeypot.id == id || honeypot.name == id)
        .ok_or_else(|| Error::NotFound(format!("honeypot {id}")))?;
    let token = item.token.clone();
    client::request(&ctx.root, "honeypot_check", json!({ "token": token }))
}

fn api_honeypot_delete(ctx: &Ctx, id: &str) -> Result<Value> {
    let mut honeypots = store::load_honeypots(&ctx.root).unwrap_or_default();
    if !honeypots.remove(id) {
        return Err(Error::NotFound(format!("honeypot {id}")));
    }
    store::save_honeypots(&ctx.root, &honeypots)?;
    Ok(json!({ "removed": id }))
}

fn audit_signature(root: &Root) -> (u64, u64) {
    let path = root.audit_log();
    std::fs::metadata(path)
        .map(|metadata| {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as u64)
                .unwrap_or(0);
            (metadata.len(), modified)
        })
        .unwrap_or((0, 0))
}

/// Helper for tests and typed callers.
pub fn read_tickets(root: &Root) -> Result<Vec<(String, TicketKind, i64, u32)>> {
    let path = root.tickets();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file: TicketFile = store::read_json(&path)?;
    Ok(file
        .tickets
        .values()
        .map(|state| {
            (
                state.claims.jti.clone(),
                state.claims.kind.clone(),
                state.claims.exp,
                state.uses_left,
            )
        })
        .collect())
}
