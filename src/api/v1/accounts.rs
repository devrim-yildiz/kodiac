//! Account administration and private, account-scoped viewing preferences.
use super::settings;
use crate::{
    AppState,
    auth::User,
    error::{AppError, Result},
};
use argon2::{
    Argon2, PasswordHasher,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Build private preference and administrator account routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/preferences",
            get(preferences).put(save_preferences),
        )
        .route("/api/v1/users", get(users).post(create))
        .route("/api/v1/users/{id}/role", axum::routing::put(role))
}

#[derive(Deserialize, Serialize)]
/// Persisted viewing preferences for one authenticated account.
pub struct Preferences {
    /// ISO language code preferred for audio tracks.
    pub audio_language_preference: String,
    /// Subtitle selection policy: off, forced or always.
    pub subtitle_mode: String,
    /// Enabled Home rows in display order.
    pub home_sections: Vec<String>,
}
/// Load account preferences, inheriting server playback defaults for new accounts.
pub async fn load(s: &AppState, user: &User) -> Result<Preferences> {
    let stored: Option<String> =
        sqlx::query_scalar("SELECT values_json FROM user_preferences WHERE user_id=?")
            .bind(&user.id)
            .fetch_optional(&s.pool)
            .await?;
    if let Some(stored) = stored {
        return serde_json::from_str(&stored).map_err(|e| AppError::Internal(e.into()));
    }
    let defaults = settings::values(s).await?;
    Ok(Preferences {
        audio_language_preference: defaults
            .get("audio_language_preference")
            .cloned()
            .unwrap_or("eng".into()),
        subtitle_mode: defaults
            .get("subtitle_mode")
            .cloned()
            .unwrap_or("off".into()),
        home_sections: ["continue", "collections", "recommended", "top"]
            .map(str::to_owned)
            .to_vec(),
    })
}
async fn preferences(
    State(s): State<AppState>,
    Extension(u): Extension<User>,
) -> Result<Json<Preferences>> {
    Ok(Json(load(&s, &u).await?))
}
async fn save_preferences(
    State(s): State<AppState>,
    Extension(u): Extension<User>,
    Json(p): Json<Preferences>,
) -> Result<Json<Preferences>> {
    if !["eng", "deu", "fra", "spa", "jpn", "ita"].contains(&p.audio_language_preference.as_str())
        || !["off", "forced", "always"].contains(&p.subtitle_mode.as_str())
        || p.home_sections.len() > 6
        || p.home_sections.iter().enumerate().any(|(i, k)| {
            ![
                "continue",
                "collections",
                "recommended",
                "top",
                "series",
                "recent",
            ]
            .contains(&k.as_str())
                || p.home_sections[..i].contains(k)
        })
    {
        return Err(AppError::BadRequest("Invalid viewing preferences".into()));
    }
    let value = serde_json::to_string(&p).map_err(|e| AppError::Internal(e.into()))?;
    sqlx::query("INSERT INTO user_preferences(user_id,values_json) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET values_json=excluded.values_json")
        .bind(u.id).bind(value).execute(&s.pool).await?;
    Ok(Json(p))
}
async fn users(State(s): State<AppState>, Extension(u): Extension<User>) -> Result<Json<Value>> {
    settings::admin(&s, &u).await?;
    let rows: Vec<(String, String, bool)> =
        sqlx::query_as("SELECT id,username,is_admin FROM users ORDER BY username COLLATE NOCASE")
            .fetch_all(&s.pool)
            .await?;
    Ok(Json(
        json!({"items": rows.into_iter().map(|(id,username,is_admin)|json!({"id":id,"username":username,"is_admin":is_admin})).collect::<Vec<_>>()}),
    ))
}
#[derive(Deserialize)]
struct NewUser {
    username: String,
    password: String,
    is_admin: bool,
}
async fn create(
    State(s): State<AppState>,
    Extension(u): Extension<User>,
    Json(input): Json<NewUser>,
) -> Result<(StatusCode, Json<Value>)> {
    settings::admin(&s, &u).await?;
    let name = input.username.trim();
    if name.is_empty()
        || name.chars().count() > 80
        || name.chars().any(char::is_control)
        || input.password.chars().count() < 12
        || input.password.len() > 1024
    {
        return Err(AppError::BadRequest(
            "Use a username of 1–80 characters and a password of at least 12 characters (maximum 1 KB)".into(),
        ));
    }
    let permit = s
        .login_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::Busy)?;
    let hash = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        Argon2::default()
            .hash_password(input.password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|h| h.to_string())
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .map_err(AppError::Internal)?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    sqlx::query("INSERT INTO users(id,username,password_hash,is_admin) VALUES(?,?,?,?)")
        .bind(&id)
        .bind(name)
        .bind(hash)
        .bind(input.is_admin)
        .execute(&s.pool)
        .await
        .map_err(|e| {
            if e.as_database_error()
                .is_some_and(|e| e.is_unique_violation())
            {
                AppError::BadRequest("Username already exists".into())
            } else {
                e.into()
            }
        })?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id":id,"username":name,"is_admin":input.is_admin})),
    ))
}
#[derive(Deserialize)]
struct Role {
    is_admin: bool,
}
async fn role(
    State(s): State<AppState>,
    Extension(u): Extension<User>,
    Path(id): Path<String>,
    Json(role): Json<Role>,
) -> Result<StatusCode> {
    settings::admin(&s, &u).await?;
    // Keeping one's own role prevents removing the final administrator, even concurrently.
    if id == u.id {
        return Err(AppError::BadRequest(
            "You cannot change your own role".into(),
        ));
    }
    let changed = sqlx::query("UPDATE users SET is_admin=? WHERE id=? AND (?=1 OR is_admin=0 OR (SELECT count(*) FROM users WHERE is_admin=1)>1)").bind(role.is_admin).bind(id).bind(role.is_admin).execute(&s.pool).await?;
    if changed.rows_affected() == 0 {
        return Err(AppError::BadRequest(
            "Account not found or the last administrator must be retained".into(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}
