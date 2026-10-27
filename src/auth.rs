//! Account bootstrap, Argon2 password verification and expiring hashed session tokens.

use crate::{
    AppState,
    config::Config,
    error::{AppError, Result},
};
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Json,
    extract::{Query, Request, State},
    http::{HeaderMap, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// Authenticated account identity inserted into request extensions.
#[derive(Clone, Debug)]
pub struct User {
    /// Stable identifier of this record.
    pub id: String,
    /// Account username.
    pub username: String,
    /// Current persisted administrator flag, refreshed when authenticating each request.
    pub is_admin: bool,
}
fn hash_token(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

/// Create the initial administrator with an Argon2 hash; existing accounts are preserved.
///
/// # Errors
/// Fails on a missing/short initial password, invalid username, hashing or database errors.
pub async fn bootstrap(pool: &SqlitePool, config: &Config) -> anyhow::Result<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        return Ok(());
    }
    let password = if let Some(path) = &config.admin_password_file {
        tokio::fs::read_to_string(path)
            .await?
            .trim_end_matches(['\r', '\n'])
            .to_owned()
    } else {
        std::env::var("ADMIN_PASSWORD").unwrap_or_default()
    };
    anyhow::ensure!(
        password.len() >= 12,
        "first run requires ADMIN_PASSWORD_FILE or ADMIN_PASSWORD (at least 12 characters)"
    );
    anyhow::ensure!(
        !config.admin_user.trim().is_empty(),
        "admin username must not be empty"
    );
    let hash = tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|h| h.to_string())
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    })
    .await??;
    sqlx::query("INSERT INTO users(id, username, password_hash,is_admin) VALUES (?,?,?,1)")
        .bind(uuid::Uuid::new_v4().simple().to_string())
        .bind(&config.admin_user)
        .bind(hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Native or Jellyfin login credentials; must never be logged.
#[derive(Deserialize)]
pub struct Credentials {
    /// Account username.
    #[serde(alias = "Username")]
    pub username: String,
    /// Plaintext password supplied for verification; never log or persist it.
    #[serde(alias = "Pw", alias = "Password")]
    pub password: String,
}
/// Verify credentials under login budgets and return the account plus a new expiring token.
pub async fn authenticate(state: &AppState, credentials: Credentials) -> Result<(User, String)> {
    // Bound expensive password hashing and brute force attempts. Global budget is deliberate
    // for this single-household MVP; it does not trust spoofable proxy IP headers.
    let mut window = state.login_window.lock().await;
    if window.0.elapsed() >= Duration::from_secs(60) {
        *window = (Instant::now(), 0);
    }
    if window.1 >= 30 {
        let remaining = Duration::from_secs(60).saturating_sub(window.0.elapsed());
        return Err(AppError::RateLimited {
            retry_after_seconds: remaining.as_secs().saturating_add(1).min(60),
        });
    }
    window.1 += 1;
    drop(window);
    let permit = state
        .login_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::Busy)?;
    if credentials.username.len() > 256 || credentials.password.len() > 1024 {
        return Err(AppError::Unauthorized);
    }
    let row: Option<(String, String, String, bool)> =
        sqlx::query_as("SELECT id, username, password_hash,is_admin FROM users WHERE username=?")
            .bind(credentials.username)
            .fetch_optional(&state.pool)
            .await?;
    // Use a real existing hash for missing usernames, preserving equivalent Argon2 cost.
    let hash: String = match &row {
        Some(row) => row.2.clone(),
        None => {
            sqlx::query_scalar("SELECT password_hash FROM users LIMIT 1")
                .fetch_one(&state.pool)
                .await?
        }
    };
    let (sender, receiver) = tokio::sync::oneshot::channel();
    // Disk I/O can exhaust Tokio's blocking pool; login has its own two-thread budget.
    std::thread::Builder::new()
        .name("password-verification".into())
        .spawn(move || {
            let _permit = permit;
            let valid = PasswordHash::new(&hash).is_ok_and(|h| {
                Argon2::default()
                    .verify_password(credentials.password.as_bytes(), &h)
                    .is_ok()
            });
            let _ = sender.send(valid);
        })?;
    let valid = receiver.await.map_err(|e| AppError::Internal(e.into()))?;
    let (id, username, _, is_admin) = row.filter(|_| valid).ok_or(AppError::Unauthorized)?;
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sqlx::query("DELETE FROM auth_tokens WHERE expires_at <= unixepoch()")
        .execute(&state.pool)
        .await?;
    sqlx::query(
        "INSERT INTO auth_tokens(token_hash,user_id,expires_at) VALUES (?,?,unixepoch()+604800)",
    )
    .bind(hash_token(&token))
    .bind(&id)
    .execute(&state.pool)
    .await?;
    Ok((
        User {
            id,
            username,
            is_admin,
        },
        token,
    ))
}
/// Format a strict HttpOnly session cookie with lifetime in seconds and optional Secure flag.
pub fn cookie(state: &AppState, token: &str, age: u64) -> String {
    format!(
        "media_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={age}{}",
        if state.config.secure_cookie {
            "; Secure"
        } else {
            ""
        }
    )
}
/// Authenticate native JSON credentials and set the browser session cookie.
pub async fn login(
    State(state): State<AppState>,
    Json(credentials): Json<Credentials>,
) -> Result<Response> {
    let (user, token) = authenticate(&state, credentials).await?;
    Ok((
        [(header::SET_COOKIE, cookie(&state, &token, 604800))],
        Json(json!({"user": {"id": user.id, "username": user.username, "is_admin":user.is_admin}})),
    )
        .into_response())
}

fn header_token(headers: &HeaderMap) -> Option<String> {
    if let Some(token) = headers.get("x-emby-token").and_then(|v| v.to_str().ok()) {
        return Some(token.to_owned());
    }
    for name in ["authorization", "x-emby-authorization"] {
        if let Some(value) = headers.get(name).and_then(|v| v.to_str().ok()) {
            if let Some(token) = value.strip_prefix("Bearer ") {
                return Some(token.to_owned());
            }
            for part in value.split(',') {
                let part = part
                    .trim()
                    .strip_prefix("MediaBrowser ")
                    .unwrap_or(part.trim());
                if let Some((key, token)) = part.split_once('=')
                    && key.trim().eq_ignore_ascii_case("token")
                {
                    return Some(token.trim().trim_matches('"').to_owned());
                }
            }
        }
    }
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .find_map(|s| s.trim().strip_prefix("media_session=").map(str::to_owned))
        })
}
/// Validate a session from supported headers, cookie or read-only media query parameters.
pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    let token = header_token(req.headers())
        .or_else(|| {
            // Query tokens are needed by native media elements and Jellyfin clients. Never trace queries.
            if req.method() != axum::http::Method::GET && req.method() != axum::http::Method::HEAD {
                return None;
            }
            Query::<HashMap<String, String>>::try_from_uri(req.uri())
                .ok()
                .and_then(|q| {
                    q.0.get("api_key")
                        .or_else(|| q.0.get("AccessToken"))
                        .cloned()
                })
        })
        .ok_or(AppError::Unauthorized)?;
    if token.len() > 256 {
        return Err(AppError::Unauthorized);
    }
    let row: Option<(String, String, bool)> = sqlx::query_as("SELECT u.id,u.username,u.is_admin FROM users u JOIN auth_tokens t ON t.user_id=u.id WHERE t.token_hash=? AND t.expires_at>unixepoch()")
        .bind(hash_token(&token)).fetch_optional(&state.pool).await?;
    let (id, username, is_admin) = row.ok_or(AppError::Unauthorized)?;
    req.extensions_mut().insert(User {
        id,
        username,
        is_admin,
    });
    req.extensions_mut().insert(TokenHash(hash_token(&token)));
    req.extensions_mut().insert(MediaToken(token));
    Ok(next.run(req).await)
}
/// SHA-256 session-token digest used to revoke the current session.
#[derive(Clone)]
pub struct TokenHash(pub String);
/// Request-scoped token for authenticated media URLs; never serialized or logged.
#[derive(Clone)]
pub struct MediaToken(pub String);
/// Revoke the current session digest and expire its browser cookie.
pub async fn logout(
    State(state): State<AppState>,
    axum::Extension(token): axum::Extension<TokenHash>,
) -> Result<Response> {
    sqlx::query("DELETE FROM auth_tokens WHERE token_hash=?")
        .bind(token.0)
        .execute(&state.pool)
        .await?;
    Ok((
        [(header::SET_COOKIE, cookie(&state, "", 0))],
        axum::http::StatusCode::NO_CONTENT,
    )
        .into_response())
}
