
pub mod jellyfin {
    
    pub mod dto;
    pub mod navigation;
    pub mod negotiation;
    use crate::{
        AppState,
        api::v1::{Progress, save_progress},
        auth::{self, Credentials, User},
        db,
        error::{AppError, Result},
        streaming,
    };
    use axum::{
        Extension, Json, Router,
        extract::{Path, Query, State},
        http::{StatusCode, header},
        response::{IntoResponse, Response},
        routing::{get, post},
    };
    use navigation::ItemQuery;
    use serde::Deserialize;
    use serde_json::{Value, json};
    
    pub fn public_router() -> Router<AppState> {
        Router::new()
            .route("/System/Info/Public", get(info))
            .route("/Users/AuthenticateByName", post(login))
            .route("/Users/Public", get(|| async { Json(json!([])) }))
    }
    pub fn protected_router() -> Router<AppState> {
        Router::new()
            .route("/System/Info", get(info))
            .route("/Users/Me", get(me))
            .route("/Users/{id}", get(user))
            .route("/UserViews", get(root_views))
            .route("/Users/{id}/Views", get(views))
            .route("/Shows/{id}/Seasons", get(seasons))
            .route("/Shows/{id}/Episodes", get(episodes))
            .route(
                "/Sessions/Capabilities/Full",
                post(|| async { StatusCode::NO_CONTENT }),
            )
            .route(
                "/Sessions/Capabilities",
                post(|| async { StatusCode::NO_CONTENT }),
            )
            .route("/Users/{id}/Items", get(user_items))
            .route("/Users/{user_id}/Items/{id}", get(user_item))
            .route("/Items", get(items))
            .route("/Items/{id}", get(item))
            .route(
                "/Items/{id}/PlaybackInfo",
                get(playback_info).post(playback_info),
            )
            .route("/Items/{id}/Images/Primary", get(streaming::poster))
            .route("/Items/{id}/Images/Backdrop", get(streaming::backdrop))
            .route("/Items/{id}/Images/Backdrop/0", get(streaming::backdrop))
            .route("/Items/{id}/Images/Primary/0", get(streaming::poster))
            .route(
                "/Videos/{id}/{source}/Subtitles/{index}/Stream.vtt",
                get(streaming::subtitles::jellyfin),
            )
            .route("/Videos/{id}/master.m3u8", get(streaming::hls::playlist))
            .route("/Videos/{id}/hls/{segment}", get(streaming::hls::segment))
            .route("/Videos/{id}/stream", get(streaming::stream))
            .route("/Videos/{id}/stream.mp4", get(streaming::stream))
            .route("/Sessions/Playing", post(playing))
            .route("/Sessions/Playing/Progress", post(playing))
            .route("/Sessions/Playing/Stopped", post(stopped))
            .route("/Sessions/Logout", post(auth::logout))
    }
    async fn info(State(state): State<AppState>) -> Json<Value> {
        Json(
            json!({"Id":*state.server_id,"ServerName":"Medienserver","Version":"10.10.7","ServerVersion":env!("CARGO_PKG_VERSION"),"ProductName":"Medienserver","OperatingSystem":std::env::consts::OS,"StartupWizardCompleted":true}),
        )
    }
    async fn login(
        State(state): State<AppState>,
        Json(credentials): Json<Credentials>,
    ) -> Result<Response> {
        let (user, token) = auth::authenticate(&state, credentials).await?;
        Ok(([(header::SET_COOKIE,auth::cookie(&state,&token,604800))], Json(json!({"User":dto::user(&user,&state.server_id),"AccessToken":token,"ServerId":*state.server_id,"SessionInfo":{"Id":uuid::Uuid::new_v4().simple().to_string(),"UserId":user.id,"UserName":user.username,"ServerId":*state.server_id}}))).into_response())
    }
    fn check_user(id: &str, user: &User) -> Result<()> {
        if id == user.id {
            Ok(())
        } else {
            Err(AppError::NotFound)
        }
    }
    async fn me(State(state): State<AppState>, Extension(user): Extension<User>) -> Json<Value> {
        Json(dto::user(&user, &state.server_id))
    }
    async fn user(
        State(state): State<AppState>,
        Extension(user): Extension<User>,
        Path(id): Path<String>,
    ) -> Result<Json<Value>> {
        check_user(&id, &user)?;
        Ok(Json(dto::user(&user, &state.server_id)))
    }
    async fn views(
        State(state): State<AppState>,
        Extension(user): Extension<User>,
        Path(id): Path<String>,
    ) -> Result<Json<Value>> {
        check_user(&id, &user)?;
        Ok(Json(navigation::views(&state)))
    }
    async fn root_views(
        State(state): State<AppState>,
        Extension(user): Extension<User>,
        Query(q): Query<ItemQuery>,
    ) -> Result<Json<Value>> {
        if let Some(id) = q.user_id {
            check_user(&id, &user)?;
        }
        Ok(Json(navigation::views(&state)))
    }
    async fn list(state: &AppState, user: &User, q: ItemQuery) -> Result<Json<dto::ItemsResponse>> {
        navigation::list(state, user, q).await.map(Json)
    }
    async fn seasons(
        State(state): State<AppState>,
        Extension(user): Extension<User>,
        Path(id): Path<String>,
        Query(mut q): Query<ItemQuery>,
    ) -> Result<Json<dto::ItemsResponse>> {
        q.parent_id = Some(id);
        q.include_item_types = Some("Season".into());
        list(&state, &user, q).await
    }
    async fn episodes(
        State(state): State<AppState>,
        Extension(user): Extension<User>,
        Path(id): Path<String>,
        Query(mut q): Query<ItemQuery>,
    ) -> Result<Json<dto::ItemsResponse>> {
        q.parent_id = Some(id);
        q.include_item_types = Some("Episode".into());
        q.sort_by
            .get_or_insert("ParentIndexNumber,IndexNumber".into());
        list(&state, &user, q).await
    }
    async fn items(
        State(state): State<AppState>,
        Extension(user): Extension<User>,
        Query(q): Query<ItemQuery>,
    ) -> Result<Json<dto::ItemsResponse>> {
        list(&state, &user, q).await
    }
    async fn user_items(
        State(state): State<AppState>,
        Extension(user): Extension<User>,
        Path(id): Path<String>,
        Query(q): Query<ItemQuery>,
    ) -> Result<Json<dto::ItemsResponse>> {
        check_user(&id, &user)?;
        list(&state, &user, q).await
    }
    async fn get_item(state: &AppState, user: &User, id: &str) -> Result<Json<Value>> {
        let item = match db::item(&state.pool, id).await {
            Ok(item) => item,
            Err(AppError::NotFound) => return navigation::get_folder(state, id).await.map(Json),
            Err(e) => return Err(e),
        };
        let position: Option<i64> = sqlx::query_scalar(
            "SELECT position_ticks FROM playback_sessions WHERE user_id=? AND media_item_id=?",
        )
        .bind(&user.id)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
        Ok(Json(dto::item(
            &item,
            &state.server_id,
            position.unwrap_or(0),
        )))
    }
    async fn item(
        State(state): State<AppState>,
        Extension(user): Extension<User>,
        Path(id): Path<String>,
    ) -> Result<Json<Value>> {
        get_item(&state, &user, &id).await
    }
    async fn user_item(
        State(state): State<AppState>,
        Extension(user): Extension<User>,
        Path((uid, id)): Path<(String, String)>,
    ) -> Result<Json<Value>> {
        check_user(&uid, &user)?;
        get_item(&state, &user, &id).await
    }
    async fn playback_info(
        State(state): State<AppState>,
        Path(id): Path<String>,
        Extension(token): Extension<auth::MediaToken>,
        Query(query): Query<negotiation::PlaybackRequest>,
        body: Option<Json<negotiation::PlaybackRequest>>,
    ) -> Result<Json<Value>> {
        let item = db::item(&state.pool, &id).await?;
        let mut request = body.map(|j| j.0).unwrap_or_default();
        request.audio_stream_index = request.audio_stream_index.or(query.audio_stream_index);
        request.subtitle_stream_index = request
            .subtitle_stream_index
            .or(query.subtitle_stream_index);
        request.start_time_ticks = request.start_time_ticks.or(query.start_time_ticks);
        request.max_streaming_bitrate = request
            .max_streaming_bitrate
            .or(query.max_streaming_bitrate);
        let mut source = negotiation::source(&item, &request)?;
        let encoded: String =
            reqwest::Url::parse_with_params("http://localhost", &[("api_key", &token.0)])
                .unwrap()
                .query()
                .unwrap()
                .into();
        for key in ["TranscodingUrl", "DirectStreamUrl"] {
            if let Some(url) = source[key].as_str() {
                source[key] = json!(format!(
                    "{url}{}{encoded}",
                    if url.contains('?') { "&" } else { "?" }
                ));
            }
        }
        if let Some(streams) = source["MediaStreams"].as_array_mut() {
            for stream in streams {
                if let Some(url) = stream["DeliveryUrl"].as_str() {
                    stream["DeliveryUrl"] = json!(format!("{url}?{encoded}"));
                }
            }
        }
        let playable = source["SupportsDirectPlay"] == true
            || source["SupportsDirectStream"] == true
            || source["SupportsTranscoding"] == true;
        let mut result =
            json!({"MediaSources":[source],"PlaySessionId":uuid::Uuid::new_v4().simple().to_string()});
        if !playable {
            result["ErrorCode"] = json!("NoCompatibleStream");
        }
        Ok(Json(result))
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Playing {
        item_id: String,
        #[serde(default)]
        position_ticks: i64,
        #[serde(default)]
        is_paused: bool,
    }
    async fn playing(
        State(state): State<AppState>,
        Extension(user): Extension<User>,
        Json(p): Json<Playing>,
    ) -> Result<StatusCode> {
        save_progress(
            &state,
            &user,
            Progress {
                profile_id: None,
                completed: None,
                item_id: p.item_id,
                position_ticks: p.position_ticks,
                is_paused: p.is_paused,
                stopped: false,
            },
        )
        .await?;
        Ok(StatusCode::NO_CONTENT)
    }
    async fn stopped(
        State(state): State<AppState>,
        Extension(user): Extension<User>,
        Json(p): Json<Playing>,
    ) -> Result<StatusCode> {
        save_progress(
            &state,
            &user,
            Progress {
                profile_id: None,
                completed: None,
                item_id: p.item_id,
                position_ticks: p.position_ticks,
                is_paused: true,
                stopped: true,
            },
        )
        .await?;
        Ok(StatusCode::NO_CONTENT)
    }
}
pub mod v1;
use axum::{extract::Request, middleware::Next, response::Response};
pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("referrer-policy", "no-referrer".parse().unwrap());
    headers.insert("x-frame-options", "DENY".parse().unwrap());
    headers
        .entry("cache-control")
        .or_insert("no-store".parse().unwrap());
    response
}
