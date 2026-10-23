use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use kodiac::{AppState, auth, config::Config, db, router, scanner};
use serde_json::{Value, json};
use std::{
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
};
use tempfile::TempDir;
use tower::ServiceExt;

struct Fixture {
    _dir: TempDir,
    state: AppState,
    app: Router,
    token: String,
    user: String,
    item: String,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("media");
        tokio::fs::create_dir(&media).await.unwrap();
        tokio::fs::write(media.join("sample.mp4"), b"0123456789")
            .await
            .unwrap();
        tokio::fs::write(
            media.join("sample.nfo"),
            b"<movie><title>Sample &amp; Test</title><year>2025</year><runtime>2</runtime></movie>",
        )
        .await
        .unwrap();
        let password = dir.path().join("password.secret");
        tokio::fs::write(&password, "a-test-password-only")
            .await
            .unwrap();
        let config = Config {
            media_path: tokio::fs::canonicalize(media).await.unwrap(),
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 0,
            db_path: dir.path().join("test.db"),
            admin_user: "admin".into(),
            admin_password_file: Some(password),
            ffmpeg: PathBuf::from("ffmpeg"),
            ffprobe: PathBuf::from("/nonexistent/ffprobe"),
            max_transcodes: 1,
            scan_interval_seconds: 0,
            transcode_max_seconds: 60,
            tmdb_token: None,
            tmdb_language: "en-US".into(),
            metadata_path: dir.path().join("metadata"),
            hw_accel: "software".into(),
            vaapi_device: "/dev/dri/renderD128".into(),
            secure_cookie: false,
            cors_origins: vec![],
        };
        let pool = db::connect(&config).await.unwrap();
        auth::bootstrap(&pool, &config).await.unwrap();
        let state = AppState::new(pool, config).await.unwrap();
        scanner::scan(&state).await.unwrap();
        let app = router(state.clone()).unwrap();
        let response = app
            .clone()
            .oneshot(
                Request::post("/Users/AuthenticateByName")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"Username":"admin","Pw":"a-test-password-only"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let login: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        let item: String = sqlx::query_scalar("SELECT id FROM media_items LIMIT 1")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        Self {
            _dir: dir,
            state,
            app,
            token: login["AccessToken"].as_str().unwrap().into(),
            user: login["User"]["Id"].as_str().unwrap().into(),
            item,
        }
    }
    async fn request(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Value,
    ) -> axum::response::Response {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header("x-emby-token", &self.token)
            .header("content-type", "application/json");
        for (name, value) in headers {
            req = req.header(*name, *value);
        }
        self.app
            .clone()
            .oneshot(
                req.body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
            )
            .await
            .unwrap()
    }
}
async fn value(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
}

#[tokio::test]
async fn jellyfin_playback_enable_flags_respect_query_and_body() {
    let f = Fixture::new().await;
    let url = format!(
        "/Items/{}/PlaybackInfo?EnableDirectPlay=false&EnableDirectStream=false&EnableTranscoding=false",
        f.item
    );
    let response = f
        .app
        .clone()
        .oneshot(
            Request::get(&url)
                .header("x-emby-token", &f.token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let info = value(response).await;
    for flag in [
        "SupportsDirectPlay",
        "SupportsDirectStream",
        "SupportsTranscoding",
    ] {
        assert_eq!(info["MediaSources"][0][flag], false);
    }
    assert_eq!(info["ErrorCode"], "NoCompatibleStream");
    let info = value(
        f.request("POST", &url, &[], json!({"EnableDirectPlay":true}))
            .await,
    )
    .await;
    assert_eq!(info["MediaSources"][0]["SupportsDirectPlay"], true);
    assert_eq!(info["MediaSources"][0]["SupportsDirectStream"], false);
    assert!(info["MediaSources"][0]["DirectStreamUrl"].is_null());

    sqlx::query("UPDATE media_items SET streams_json=?,video_codec='hevc',audio_codec='aac' WHERE id=?")
        .bind(json!([{"index":0,"codec_type":"video","codec_name":"hevc"},{"index":1,"codec_type":"audio","codec_name":"aac"}]).to_string())
        .bind(&f.item).execute(&f.state.pool).await.unwrap();
    let profile = json!({"TranscodingProfiles":[{"Type":"Video","Container":"mp4","Protocol":"http","VideoCodec":"h264","AudioCodec":"aac"}]});
    let info = value(
        f.request("POST", &url, &[], json!({"DeviceProfile":profile}))
            .await,
    )
    .await;
    assert_eq!(info["MediaSources"][0]["SupportsTranscoding"], false);
    let info = value(
        f.request(
            "POST",
            &url,
            &[],
            json!({"DeviceProfile":profile,"EnableTranscoding":true}),
        )
        .await,
    )
    .await;
    assert_eq!(info["MediaSources"][0]["SupportsTranscoding"], true);
}

#[tokio::test]
async fn jellyfin_watched_state_uses_only_the_default_profile() {
    let f = Fixture::new().await;
    sqlx::query("INSERT INTO profiles(id,user_id,name) VALUES('other',?,'Other')")
        .bind(&f.user)
        .execute(&f.state.pool)
        .await
        .unwrap();
    for (profile, completed, expected) in [
        ("other", true, false),
        (f.user.as_str(), true, true),
        (f.user.as_str(), false, false),
    ] {
        assert_eq!(f.request("POST","/api/v1/playback",&[],json!({"item_id":f.item,"profile_id":profile,"position_ticks":500,"completed":completed})).await.status(),StatusCode::NO_CONTENT);
        for path in [
            format!("/Items/{}", f.item),
            format!("/Users/{}/Items/{}", f.user, f.item),
        ] {
            let item = value(f.request("GET", &path, &[], Value::Null).await).await;
            assert_eq!(item["UserData"]["Played"], expected);
            assert_eq!(item["UserData"]["PlayCount"], i32::from(expected));
        }
        for path in ["/Items".to_owned(), format!("/Users/{}/Items", f.user)] {
            let page = value(f.request("GET", &path, &[], Value::Null).await).await;
            assert_eq!(page["Items"][0]["UserData"]["Played"], expected);
        }
    }
}

#[tokio::test]
async fn authentication_discovery_and_jellyfin_catalog() {
    let f = Fixture::new().await;
    for path in [
        "/api/v1/items".to_owned(),
        format!("/Videos/{}/stream", f.item),
        "/System/Info".to_owned(),
    ] {
        let response = f
            .app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let public = f
        .app
        .clone()
        .oneshot(
            Request::get("/System/Info/Public")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(public.status(), StatusCode::OK);
    let response = f
        .request("GET", &format!("/Users/{}/Items", f.user), &[], Value::Null)
        .await;
    let catalog = value(response).await;
    assert_eq!(catalog["TotalRecordCount"], 1);
    assert_eq!(catalog["Items"][0]["Name"], "Sample & Test");
    assert!(catalog["Items"][0].get("path").is_none());
    assert_eq!(
        f.request("GET", "/Users/other/Items", &[], Value::Null)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let info = value(
        f.request(
            "POST",
            &format!("/Items/{}/PlaybackInfo", f.item),
            &[],
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(info["MediaSources"][0]["SupportsTranscoding"], false);
    let login = f
        .app
        .clone()
        .oneshot(
            Request::post("/api/v1/login")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"admin","password":"bad"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::UNAUTHORIZED);
    let bearer = f
        .app
        .clone()
        .oneshot(
            Request::get("/api/v1/me")
                .header(
                    "authorization",
                    format!("MediaBrowser Client=\"Test\", Token=\"{}\"", f.token),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bearer.status(), StatusCode::OK);
    assert_eq!(
        f.request("POST", "/api/v1/logout", &[], Value::Null)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.request("GET", "/api/v1/me", &[], Value::Null)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn byte_ranges_head_and_validators() {
    let f = Fixture::new().await;
    let path = format!("/api/v1/items/{}/stream", f.item);
    let whole = f.request("GET", &path, &[], Value::Null).await;
    assert_eq!(whole.status(), StatusCode::OK);
    assert_eq!(whole.headers()["content-length"], "10");
    let etag = whole.headers()["etag"].to_str().unwrap().to_string();
    assert_eq!(
        &to_bytes(whole.into_body(), 1024).await.unwrap()[..],
        b"0123456789"
    );
    for (range, expected, content) in [
        ("bytes=0-0", "bytes 0-0/10", "0"),
        ("bytes=4-", "bytes 4-9/10", "456789"),
        ("bytes=-3", "bytes 7-9/10", "789"),
        ("bytes=8-999", "bytes 8-9/10", "89"),
    ] {
        let response = f
            .request("GET", &path, &[("range", range)], Value::Null)
            .await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()["content-range"], expected);
        assert_eq!(
            &to_bytes(response.into_body(), 1024).await.unwrap()[..],
            content.as_bytes()
        );
    }
    let bad = f
        .request("GET", &path, &[("range", "bytes=10-")], Value::Null)
        .await;
    assert_eq!(bad.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(bad.headers()["content-range"], "bytes */10");
    let head = f
        .request("HEAD", &path, &[("range", "bytes=0-1")], Value::Null)
        .await;
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers()["content-length"], "10");
    assert!(to_bytes(head.into_body(), 1024).await.unwrap().is_empty());
    assert_eq!(
        f.request("GET", &path, &[("if-none-match", &etag)], Value::Null)
            .await
            .status(),
        StatusCode::NOT_MODIFIED
    );
    assert_eq!(
        f.request(
            "GET",
            &path,
            &[("range", "bytes=1-2"), ("if-range", "\"old\"")],
            Value::Null
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        f.request(
            "GET",
            &path,
            &[("range", "bytes=1-2"), ("if-range", &etag)],
            Value::Null
        )
        .await
        .status(),
        StatusCode::PARTIAL_CONTENT
    );
    let query = f
        .app
        .clone()
        .oneshot(
            Request::get(format!("{path}?api_key={}", f.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(query.status(), StatusCode::OK);
}

#[tokio::test]
async fn scanner_updates_preserve_id_and_progress_and_mark_missing() {
    let f = Fixture::new().await;
    let response = f
        .request(
            "POST",
            "/api/v1/playback",
            &[],
            json!({"item_id":f.item,"position_ticks":123456,"is_paused":true}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    tokio::fs::write(f.state.config.media_path.join("sample.nfo"),"<episodedetails><title>Pilot</title><showtitle>Test Show</showtitle><season>1</season><episode>1</episode></episodedetails>").await.unwrap();
    scanner::scan(&f.state).await.unwrap();
    let item = db::item(&f.state.pool, &f.item).await.unwrap();
    assert_eq!(item.title, "Pilot");
    assert_eq!(item.kind, "episode");
    let playback = value(f.request("GET", "/api/v1/playback", &[], Value::Null).await).await;
    assert_eq!(playback["items"][0]["position_ticks"], 123456);
    let series = value(f.request("GET", "/api/v1/series", &[], Value::Null).await).await;
    assert_eq!(series["items"][0]["name"], "Test Show");
    tokio::fs::remove_file(f.state.config.media_path.join("sample.mp4"))
        .await
        .unwrap();
    scanner::scan(&f.state).await.unwrap();
    assert!(db::item(&f.state.pool, &f.item).await.is_err());
    let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(journal, "wal");
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/playback",
            &[],
            json!({"item_id":f.item,"position_ticks":-1})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn empty_files_invalid_modes_static_assets_and_migrations() {
    let f = Fixture::new().await;
    let path = format!("/api/v1/items/{}/stream", f.item);
    assert_eq!(
        f.request("GET", &format!("{path}?mode=unknown"), &[], Value::Null)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        f.request(
            "GET",
            &format!("{path}?mode=transcode&start_seconds=-1"),
            &[],
            Value::Null
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    tokio::fs::write(f.state.config.media_path.join("sample.mp4"), [])
        .await
        .unwrap();
    assert_eq!(
        f.request("GET", &path, &[], Value::Null).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        f.request("GET", &path, &[("range", "bytes=0-")], Value::Null)
            .await
            .status(),
        StatusCode::RANGE_NOT_SATISFIABLE
    );
    let response = f.request("GET", "/", &[], Value::Null).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("content-security-policy"));
    let html = String::from_utf8(
        to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("id=\"root\""));
    let module = regex::Regex::new(r#"src="(/assets/[^"]+\.js)""#).unwrap();
    let asset = module
        .captures(&html)
        .expect("Vite module must be embedded")
        .get(1)
        .unwrap()
        .as_str();
    let javascript = f.request("GET", asset, &[], Value::Null).await;
    assert_eq!(javascript.status(), StatusCode::OK);
    assert!(
        javascript.headers()["content-type"]
            .to_str()
            .unwrap()
            .contains("javascript")
    );
    assert!(
        javascript.headers()["cache-control"]
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    for source_path in ["/src/App.tsx", "/package.json", "/app.js"] {
        assert_eq!(
            f.request("GET", source_path, &[], Value::Null)
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }

    assert_eq!(
        f.request("GET", "/missing.js", &[], Value::Null)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    sqlx::migrate!().run(&f.state.pool).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_cannot_escape_media_root() {
    let f = Fixture::new().await;
    let outside = f._dir.path().join("outside.mp4");
    tokio::fs::write(&outside, "private").await.unwrap();
    std::os::unix::fs::symlink(&outside, f.state.config.media_path.join("escape.mp4")).unwrap();
    scanner::scan(&f.state).await.unwrap();
    let (_, count) = db::catalog(&f.state.pool, None, "", 0, 100).await.unwrap();
    assert_eq!(count, 1);
    tokio::fs::remove_file(f.state.config.media_path.join("sample.mp4"))
        .await
        .unwrap();
    std::os::unix::fs::symlink(&outside, f.state.config.media_path.join("sample.mp4")).unwrap();
    assert_eq!(
        f.request(
            "GET",
            &format!("/Videos/{}/stream", f.item),
            &[],
            Value::Null
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn expired_tokens_and_throttling() {
    let f = Fixture::new().await;
    sqlx::query("UPDATE auth_tokens SET expires_at=0")
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(
        f.request("GET", "/api/v1/me", &[], Value::Null)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    *f.state.login_window.lock().await = (
        std::time::Instant::now() - std::time::Duration::from_secs(40),
        30,
    );
    let response = f
        .app
        .clone()
        .oneshot(
            Request::post("/api/v1/login")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"admin","password":"bad"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry: u64 = response.headers()["retry-after"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=20).contains(&retry));

    f.state.login_window.lock().await.1 = 0;
    let _slots = f.state.login_slots.acquire_many(2).await.unwrap();
    let response = f
        .request(
            "POST",
            "/api/v1/login",
            &[],
            json!({"username":"admin","password":"bad"}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!response.headers().contains_key("retry-after"));
}

#[tokio::test]
#[ignore = "requires ffmpeg and ffprobe on PATH"]
async fn real_ffmpeg_remux_transcode_failure_and_disconnect() {
    let mut f = Fixture::new().await;
    std::sync::Arc::make_mut(&mut f.state.config).ffprobe = PathBuf::from("ffprobe");
    let path = f.state.config.media_path.join("sample.mp4");
    let status = tokio::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440",
            "-t",
            "3",
            "-c:v",
            "libx264",
            "-g",
            "24",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-movflags",
            "+faststart",
        ])
        .arg(&path)
        .status()
        .await
        .unwrap();
    assert!(status.success());
    scanner::scan(&f.state).await.unwrap();
    f.app = router(f.state.clone()).unwrap();
    for mode in ["remux", "transcode"] {
        let response = f
            .request(
                "GET",
                &format!("/Videos/{}/stream?mode={mode}", f.item),
                &[],
                Value::Null,
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["accept-ranges"], "none");
        let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(&bytes[4..8], b"ftyp");
        let output = f._dir.path().join(format!("{mode}.mp4"));
        tokio::fs::write(&output, bytes).await.unwrap();
        let status = tokio::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(output)
            .args(["-f", "null", "-"])
            .status()
            .await
            .unwrap();
        assert!(status.success());
    }
    // A remux seek maps the normalized pipe timestamps back to the original keyframe.
    let response = kodiac::streaming::transcode::serve(&f.state, &path, false, 2.4)
        .await
        .unwrap();
    let origin: f64 = response.headers()["x-media-start-seconds"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap();
    let seek_output = f._dir.path().join("seek-remux.mp4");
    tokio::fs::write(&seek_output, bytes).await.unwrap();
    let probe = tokio::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "packet=pts_time",
            "-of",
            "json",
        ])
        .arg(&seek_output)
        .output()
        .await
        .unwrap();
    assert!(probe.status.success());
    let packets: Value = serde_json::from_slice(&probe.stdout).unwrap();
    let first_pts: f64 = packets["packets"][0]["pts_time"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        (1.99..2.1).contains(&(first_pts + origin)),
        "keyframe timestamp lost: {first_pts} + {origin}"
    );
    let response = kodiac::streaming::transcode::serve(&f.state, &path, true, 0.0)
        .await
        .unwrap();
    assert_eq!(f.state.transcodes.available_permits(), 0);
    assert!(
        kodiac::streaming::transcode::serve(&f.state, &path, true, 0.0)
            .await
            .is_err()
    );
    drop(response);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while f.state.transcodes.available_permits() == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    tokio::fs::write(&path, b"not a video").await.unwrap();
    assert!(
        kodiac::streaming::transcode::serve(&f.state, &path, true, 0.0)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn failed_scan_retains_catalog_and_concurrent_scan_is_busy() {
    let f = Fixture::new().await;
    let guard = f.state.scan_lock.lock().await;
    assert_eq!(
        f.request("POST", "/api/v1/scan", &[], Value::Null)
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(guard);
    // Reproduce stale progress left by a failed scan after indexing a file.
    *f.state.scan_status.lock().await = json!({"running":true,"indexed":1});
    tokio::fs::rename(
        &f.state.config.media_path,
        f._dir.path().join("temporarily-offline"),
    )
    .await
    .unwrap();
    assert!(scanner::scan(&f.state).await.is_err());
    assert_eq!(f.state.scan_status.lock().await["running"], false);
    assert!(db::item(&f.state.pool, &f.item).await.is_ok());
    tokio::fs::rename(
        f._dir.path().join("temporarily-offline"),
        &f.state.config.media_path,
    )
    .await
    .unwrap();
    assert_eq!(
        f.request("POST", "/api/v1/settings/rescan", &[], Value::Null)
            .await
            .status(),
        StatusCode::ACCEPTED
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(guard) = f.state.scan_lock.try_lock() {
                drop(guard);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(f.state.scan_status.lock().await["running"], false);
    f.state.shutdown.cancel();
    assert!(scanner::scan(&f.state).await.is_err());
    assert_eq!(f.state.scan_status.lock().await["running"], false);
}

#[tokio::test]
async fn changing_the_configured_root_hides_previous_library() {
    let mut f = Fixture::new().await;
    let new_root = f._dir.path().join("new-root");
    tokio::fs::create_dir(&new_root).await.unwrap();
    std::sync::Arc::make_mut(&mut f.state.config).media_path =
        tokio::fs::canonicalize(&new_root).await.unwrap();
    scanner::scan(&f.state).await.unwrap();
    assert!(db::item(&f.state.pool, &f.item).await.is_err());
    f.app = router(f.state.clone()).unwrap();
    let views = value(
        f.request("GET", &format!("/Users/{}/Views", f.user), &[], Value::Null)
            .await,
    )
    .await;
    assert_eq!(views["TotalRecordCount"], 2);
}

#[tokio::test]
async fn nfo_rescans_replace_local_links_atomically() {
    let f = Fixture::new().await;
    let nfo = f.state.config.media_path.join("sample.nfo");
    for (actor, director, collection) in [("Alice", "First", "Old"), ("Bob", "Second", "New")] {
        tokio::fs::write(&nfo, format!("<movie><title>{actor}</title><actor><name>{actor}</name></actor><director>{director}</director><set><name>{collection}</name></set></movie>")).await.unwrap();
        scanner::scan(&f.state).await.unwrap();
        let credits: Vec<(String, String)> =
            sqlx::query_as("SELECT name,role FROM item_people WHERE item_id=? ORDER BY role")
                .bind(&f.item)
                .fetch_all(&f.state.pool)
                .await
                .unwrap();
        assert_eq!(
            credits,
            vec![
                (actor.into(), "Actor".into()),
                (director.into(), "Director".into())
            ]
        );
        let collections: Vec<String> = sqlx::query_scalar("SELECT title FROM collections c JOIN collection_movies m ON m.collection_id=c.id WHERE m.movie_id=?")
            .bind(&f.item).fetch_all(&f.state.pool).await.unwrap();
        assert_eq!(collections, [collection]);
    }
    // A failure after deleting old links must also roll back the title and new links.
    sqlx::query("CREATE TRIGGER reject_credit BEFORE INSERT ON item_people BEGIN SELECT RAISE(ABORT,'fixture failure'); END")
        .execute(&f.state.pool).await.unwrap();
    tokio::fs::write(&nfo, "<movie><title>Rejected</title><actor><name>Rejected</name></actor><set><name>Rejected</name></set></movie>").await.unwrap();
    assert_eq!(scanner::scan(&f.state).await.unwrap().failed, 1);
    assert_eq!(db::item(&f.state.pool, &f.item).await.unwrap().title, "Bob");
    let credits: Vec<String> =
        sqlx::query_scalar("SELECT name FROM item_people WHERE item_id=? ORDER BY name")
            .bind(&f.item)
            .fetch_all(&f.state.pool)
            .await
            .unwrap();
    assert_eq!(credits, ["Bob", "Second"]);
    let collections: Vec<String> = sqlx::query_scalar("SELECT title FROM collections c JOIN collection_movies m ON m.collection_id=c.id WHERE m.movie_id=?")
        .bind(&f.item).fetch_all(&f.state.pool).await.unwrap();
    assert_eq!(collections, ["New"]);
    sqlx::query("DROP TRIGGER reject_credit")
        .execute(&f.state.pool)
        .await
        .unwrap();

    // Missing or invalid NFO is not authoritative and must preserve existing links.
    tokio::fs::remove_file(&nfo).await.unwrap();
    scanner::scan(&f.state).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM item_people WHERE item_id=?")
        .bind(&f.item)
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    tokio::fs::write(&nfo, "<movie><year>invalid</year></movie>")
        .await
        .unwrap();
    scanner::scan(&f.state).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM item_people WHERE item_id=?")
        .bind(&f.item)
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(count, 2);

    sqlx::query("INSERT INTO collections(id,title) VALUES('tmdb:fixture','Remote')")
        .execute(&f.state.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO collection_movies(collection_id,movie_id) VALUES('tmdb:fixture',?)")
        .bind(&f.item)
        .execute(&f.state.pool)
        .await
        .unwrap();
    tokio::fs::write(&nfo, "<movie><title>Empty credits</title></movie>")
        .await
        .unwrap();
    scanner::scan(&f.state).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM item_people WHERE item_id=?")
        .bind(&f.item)
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let collections: Vec<String> =
        sqlx::query_scalar("SELECT collection_id FROM collection_movies WHERE movie_id=?")
            .bind(&f.item)
            .fetch_all(&f.state.pool)
            .await
            .unwrap();
    assert_eq!(collections, ["tmdb:fixture"]);
}

#[tokio::test]
async fn diagnostics_count_extracted_artwork_with_either_separator() {
    let f = Fixture::new().await;
    for path in [
        "/cache/extracted/frame.jpg",
        r"C:\cache\extracted\frame.jpg",
    ] {
        sqlx::query("UPDATE media_items SET poster_path=?,backdrop_path=NULL WHERE id=?")
            .bind(path)
            .bind(&f.item)
            .execute(&f.state.pool)
            .await
            .unwrap();
        let stats = value(
            f.request("GET", "/api/v1/settings/stats", &[], Value::Null)
                .await,
        )
        .await;
        assert_eq!(stats["extracted_count"], 1);
        sqlx::query("UPDATE media_items SET backdrop_path=poster_path,poster_path=NULL WHERE id=?")
            .bind(&f.item)
            .execute(&f.state.pool)
            .await
            .unwrap();
        let stats = value(
            f.request("GET", "/api/v1/settings/stats", &[], Value::Null)
                .await,
        )
        .await;
        assert_eq!(stats["extracted_count"], 1);
    }
}

#[tokio::test]
async fn beta_navigation_profiles_images_and_progress() {
    let f = Fixture::new().await;
    let streams = json!([
        {"index":0,"codec_type":"video","codec_name":"hevc","width":1920,"height":1080,"pix_fmt":"yuv420p10le"},
        {"index":2,"codec_type":"audio","codec_name":"aac","channels":2,"tags":{"language":"deu"},"disposition":{"default":1}},
        {"index":5,"codec_type":"audio","codec_name":"ac3","channels":6,"tags":{"language":"eng"}},
        {"index":7,"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"eng"},"disposition":{"forced":1}}
    ]);
    sqlx::query("UPDATE media_items SET streams_json=?,kind='episode',series_name='Alien: Earth',season=1,episode=5,container='mkv',video_codec='hevc',audio_codec='aac'").bind(streams.to_string()).execute(&f.state.pool).await.unwrap();
    let views = value(f.request("GET", "/UserViews", &[], Value::Null).await).await;
    assert_eq!(views["Items"][0]["Name"], "Movies");
    assert_eq!(views["Items"][1]["Name"], "Series");
    let series = value(
        f.request(
            "GET",
            &format!("/Users/{}/Items?IncludeItemTypes=Series&Limit=1", f.user),
            &[],
            Value::Null,
        )
        .await,
    )
    .await;
    let id = series["Items"][0]["Id"].as_str().unwrap();
    let seasons = value(
        f.request("GET", &format!("/Shows/{id}/Seasons"), &[], Value::Null)
            .await,
    )
    .await;
    assert_eq!(seasons["Items"][0]["Type"], "Season");
    let eps = value(
        f.request("GET", &format!("/Shows/{id}/Episodes"), &[], Value::Null)
            .await,
    )
    .await;
    assert_eq!(eps["Items"][0]["IndexNumber"], 5);
    let empty=value(f.request("GET",&format!("/Users/{}/Items?IncludeItemTypes=Movie,Episode&StartIndex=1&Limit=5&SortBy=Name&SortOrder=Descending",f.user),&[],Value::Null).await).await;
    assert_eq!(empty["TotalRecordCount"], 1);
    assert_eq!(empty["Items"].as_array().unwrap().len(), 0);
    let profile = json!({"AudioStreamIndex":5,"DeviceProfile":{"DirectPlayProfiles":[{"Type":"Video","Container":"mp4","VideoCodec":"h264","AudioCodec":"aac"}],"TranscodingProfiles":[{"Type":"Video","Container":"ts","Protocol":"hls","VideoCodec":"h264","AudioCodec":"aac"}]}});
    let info = value(
        f.request(
            "POST",
            &format!("/Items/{}/PlaybackInfo", f.item),
            &[],
            profile,
        )
        .await,
    )
    .await;
    let source = &info["MediaSources"][0];
    assert_eq!(source["SupportsDirectPlay"], false);
    assert_eq!(source["SupportsTranscoding"], true);
    assert_eq!(source["DefaultAudioStreamIndex"], 5);
    assert_eq!(source["MediaStreams"][1]["Language"], "deu");
    assert_eq!(source["MediaStreams"][3]["IsForced"], true);
    assert!(
        source["TranscodingUrl"]
            .as_str()
            .unwrap()
            .contains("audio_index=1")
    );
    assert_eq!(source["TranscodingSubProtocol"], "hls");
    let playlist = f
        .request(
            "GET",
            source["TranscodingUrl"].as_str().unwrap(),
            &[],
            Value::Null,
        )
        .await;
    assert_eq!(playlist.status(), StatusCode::OK);
    let text = String::from_utf8(
        to_bytes(playlist.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(text.starts_with("#EXTM3U"));
    assert!(text.contains("#EXT-X-ENDLIST"));
    let direct=value(f.request("POST",&format!("/Items/{}/PlaybackInfo",f.item),&[],json!({"DeviceProfile":{"DirectPlayProfiles":[{"Type":"Video","Container":"mkv","VideoCodec":"hevc","AudioCodec":"aac"}]}})).await).await;
    assert_eq!(direct["MediaSources"][0]["SupportsDirectPlay"], true);
    for (event, ticks) in [
        ("Playing", 100),
        ("Playing/Progress", 200),
        ("Playing/Stopped", 300),
    ] {
        assert_eq!(
            f.request(
                "POST",
                &format!("/Sessions/{event}"),
                &[],
                json!({"ItemId":f.item,"PositionTicks":ticks})
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
    }
    let progress = value(
        f.request(
            "GET",
            &format!("/Users/{}/Items/{}", f.user, f.item),
            &[],
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(progress["UserData"]["PlaybackPositionTicks"], 300);
    let image = f.state.config.media_path.join("backdrop.jpg");
    tokio::fs::write(&image, [0xff, 0xd8, 0xff, 0xd9])
        .await
        .unwrap();
    sqlx::query("UPDATE media_items SET backdrop_path=?")
        .bind(image.to_str().unwrap())
        .execute(&f.state.pool)
        .await
        .unwrap();
    let response = f
        .request(
            "GET",
            &format!("/Items/{}/Images/Backdrop", f.item),
            &[],
            Value::Null,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/jpeg");
}

#[tokio::test]
#[ignore = "requires real ffmpeg/ffprobe"]
async fn beta_real_multi_audio_subtitles_and_hls() {
    let mut f = Fixture::new().await;
    std::sync::Arc::make_mut(&mut f.state.config).ffprobe = "ffprobe".into();
    let media = &f.state.config.media_path;
    let sub = media.join("Alien.Earth.S01E05.eng.srt");
    tokio::fs::write(&sub, "1\n00:00:00,200 --> 00:00:01,800\nHello beta\n")
        .await
        .unwrap();
    let path = media.join("Alien.Earth.S01E05.mkv");
    let status = tokio::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:sample_rate=48000",
            "-i",
        ])
        .arg(&sub)
        .args([
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-map",
            "3:s",
            "-t",
            "8",
            "-c:v",
            "libx264",
            "-g",
            "24",
            "-c:a",
            "aac",
            "-c:s",
            "srt",
            "-metadata:s:a:0",
            "language=deu",
            "-metadata:s:a:1",
            "language=eng",
        ])
        .arg(&path)
        .status()
        .await
        .unwrap();
    assert!(status.success());
    scanner::scan(&f.state).await.unwrap();
    f.app = router(f.state.clone()).unwrap();
    let item: db::MediaItem = sqlx::query_as("SELECT * FROM media_items WHERE path=?")
        .bind(path.to_str().unwrap())
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    let raw: Vec<Value> = serde_json::from_str(&item.streams_json).unwrap();
    assert_eq!(raw.iter().filter(|s| s["codec_type"] == "audio").count(), 2);
    assert_eq!(item.series_name.as_deref(), Some("Alien Earth"));
    let subs: Vec<_> = raw
        .iter()
        .filter(|s| s["codec_type"] == "subtitle")
        .collect();
    assert_eq!(subs.len(), 2);
    for sub in subs {
        let response = f
            .request(
                "GET",
                &format!("/api/v1/videos/{}/subtitles/{}.vtt", item.id, sub["index"]),
                &[],
                Value::Null,
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.starts_with("WEBVTT"));
        assert!(text.contains("Hello beta"));
    }
    assert_eq!(
        f.request(
            "GET",
            &format!("/Videos/{}/stream?audio_index=99", item.id),
            &[],
            Value::Null
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let response = f
        .request(
            "GET",
            &format!("/Videos/{}/stream?mode=remux&audio_index=1", item.id),
            &[],
            Value::Null,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap();
    let out = f._dir.path().join("selected.mp4");
    tokio::fs::write(&out, bytes).await.unwrap();
    let probe = tokio::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(&out)
        .output()
        .await
        .unwrap();
    let probe: Value = serde_json::from_slice(&probe.stdout).unwrap();
    let audio: Vec<_> = probe["streams"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["codec_type"] == "audio")
        .collect();
    assert_eq!(audio.len(), 1);
    // Verify selected content by decoding PCM and estimating zero-crossing frequency.
    let pcm = tokio::process::Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&out)
        .args([
            "-map", "0:a:0", "-t", "1", "-ac", "1", "-ar", "48000", "-f", "s16le", "pipe:1",
        ])
        .output()
        .await
        .unwrap();
    let samples: Vec<_> = pcm
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    let crossings = samples.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count();
    assert!(
        (850..910).contains(&crossings),
        "wrong audio: {crossings} Hz"
    );
    for n in [0, 1] {
        let response = f
            .request(
                "GET",
                &format!("/Videos/{}/hls/{n}.ts?audio_index=1", item.id),
                &[],
                Value::Null,
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(bytes[0], 0x47);
        let out = f._dir.path().join(format!("segment{n}.ts"));
        tokio::fs::write(&out, bytes).await.unwrap();
        let decode = tokio::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&out)
            .args(["-f", "null", "-"])
            .status()
            .await
            .unwrap();
        assert!(decode.success());
    }
}

#[tokio::test]
async fn tmdb_cached_episode_metadata_and_nfo_precedence() {
    let f = Fixture::new().await;
    let mut state = f.state.clone();
    std::sync::Arc::make_mut(&mut state.config).tmdb_token =
        Some("test-token-not-a-real-credential".into());
    let query = vec![
        ("query", "Alien Earth".to_string()),
        ("include_adult", "false".to_string()),
    ];
    for (route, query, response) in [
        (
            "search/tv",
            query,
            json!({"results":[{"id":123,"name":"Alien: Earth"}]}),
        ),
        (
            "tv/123",
            vec![("append_to_response", "credits,videos".to_string())],
            json!({"name":"Alien: Earth","first_air_date":"2025-08-12","overview":"Series overview"}),
        ),
        ("tv/123/season/1", vec![], json!({"name":"Season 1"})),
        (
            "tv/123/season/1/episode/5",
            vec![],
            json!({"name":"Official episode name","overview":"Episode overview"}),
        ),
    ] {
        let key = serde_json::to_string(&(route, query, "en-US")).unwrap();
        sqlx::query("INSERT INTO metadata_cache(cache_key,response_json) VALUES (?,?)")
            .bind(key)
            .bind(response.to_string())
            .execute(&state.pool)
            .await
            .unwrap();
    }
    let path = state.config.media_path.join("Alien.Earth.S01E05.1080p.mkv");
    tokio::fs::write(&path, b"test").await.unwrap();
    scanner::scan(&state).await.unwrap();
    // Scanning is local-only; only the episode job is eligible in this fixture.
    sqlx::query("UPDATE tmdb_jobs SET completed_at=unixepoch(),next_attempt_at=unixepoch()+2592000 WHERE media_item_id NOT IN(SELECT id FROM media_items WHERE path=?)").bind(path.to_str().unwrap()).execute(&state.pool).await.unwrap();
    let local: String = sqlx::query_scalar("SELECT title FROM media_items WHERE path=?")
        .bind(path.to_str().unwrap())
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_ne!(local, "Official episode name");
    assert!(scanner::tmdb_worker::run_once(&state).await.unwrap());
    let item: db::MediaItem = sqlx::query_as("SELECT * FROM media_items WHERE path=?")
        .bind(path.to_str().unwrap())
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(item.series_name.as_deref(), Some("Alien Earth"));
    let show_title: String = sqlx::query_scalar("SELECT title FROM shows")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(show_title, "Alien: Earth");
    assert_eq!(item.title, "Official episode name");
    assert_eq!(item.plot, "Episode overview");
    scanner::scan(&state).await.unwrap();
    assert_eq!(
        db::item(&state.pool, &item.id).await.unwrap().title,
        "Official episode name"
    );
    tokio::fs::write(path.with_extension("nfo"),"<episodedetails><title>Local title</title><showtitle>Local show</showtitle><season>1</season><episode>5</episode></episodedetails>").await.unwrap();
    scanner::scan(&state).await.unwrap();
    let item = db::item(&state.pool, &item.id).await.unwrap();
    assert_eq!(item.title, "Local title");
    assert_eq!(item.series_name.as_deref(), Some("Local show"));
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "requires macOS VideoToolbox hardware and FFmpeg"]
async fn real_videotoolbox_and_initialization_fallback() {
    use std::os::unix::fs::PermissionsExt;
    let mut f = Fixture::new().await;
    let path = f.state.config.media_path.join("hardware.mp4");
    let status = tokio::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=640x360:rate=24",
            "-t",
            "2",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&path)
        .status()
        .await
        .unwrap();
    assert!(status.success());
    std::sync::Arc::make_mut(&mut f.state.config).hw_accel = "videotoolbox".into();
    let response = kodiac::streaming::transcode::serve(&f.state, &path, true, 0.0)
        .await
        .unwrap();
    assert_eq!(
        response.headers()["x-transcode-backend"],
        "videotoolbox",
        "hardware test must actually use the GPU"
    );
    let data = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap();
    let out = f._dir.path().join("vt.mp4");
    tokio::fs::write(&out, data).await.unwrap();
    assert!(
        tokio::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(out)
            .args(["-f", "null", "-"])
            .status()
            .await
            .unwrap()
            .success()
    );
    let wrapper = f._dir.path().join("ffmpeg-fail-hardware");
    // The hardware branch emits a misleading MP4 header before failing. No such
    // bytes may leak into the successful software response.
    tokio::fs::write(&wrapper,"#!/bin/sh\ncase \" $* \" in\n*' -encoders '*) echo ' V h264_videotoolbox'; exit 0;;\n*' h264_videotoolbox '*) printf '\\000\\000\\000\\020ftypisom0000'; exit 1;;\nesac\nexec ffmpeg \"$@\"\n").await.unwrap();
    tokio::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700))
        .await
        .unwrap();
    std::sync::Arc::make_mut(&mut f.state.config).ffmpeg = wrapper;
    let response = kodiac::streaming::transcode::serve(&f.state, &path, true, 0.0)
        .await
        .unwrap();
    assert_eq!(response.headers()["x-transcode-backend"], "software");
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(&bytes[4..8], b"ftyp");
    assert_eq!(bytes.windows(4).filter(|s| *s == b"ftyp").count(), 1);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(f.state.transcodes.available_permits(), 1);
}

#[tokio::test]
async fn relational_catalog_profiles_and_continue_watching_are_isolated() {
    let f = Fixture::new().await;
    let movies = value(f.request("GET", "/api/v1/movies", &[], Value::Null).await).await;
    assert_eq!(movies["total"], 1);
    assert_eq!(movies["items"][0]["id"], f.item);
    assert!(movies["items"][0].get("path").is_none());
    assert_eq!(
        f.request(
            "GET",
            &format!("/api/v1/movies/{}", f.item),
            &[],
            Value::Null
        )
        .await
        .status(),
        StatusCode::OK
    );
    let mut ids = Vec::new();
    for name in [
        "Alien.Earth.S00E01.mkv",
        "Alien.Earth.S01E02.mkv",
        "Alien.Earth.S01E01.mkv",
    ] {
        tokio::fs::write(f.state.config.media_path.join(name), b"fixture")
            .await
            .unwrap();
    }
    scanner::scan(&f.state).await.unwrap();
    let shows = value(
        f.request("GET", "/api/v1/shows?limit=1", &[], Value::Null)
            .await,
    )
    .await;
    assert_eq!(shows["total"], 1);
    assert_eq!(shows["items"][0]["episode_count"], 3);
    let show = shows["items"][0]["id"].as_str().unwrap();
    let seasons = value(
        f.request(
            "GET",
            &format!("/api/v1/shows/{show}/seasons"),
            &[],
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(seasons["total"], 2);
    assert_eq!(seasons["items"][0]["season_number"], 0);
    for season in seasons["items"].as_array().unwrap() {
        let season = season["id"].as_str().unwrap();
        let eps = value(
            f.request(
                "GET",
                &format!("/api/v1/seasons/{season}/episodes"),
                &[],
                Value::Null,
            )
            .await,
        )
        .await;
        for ep in eps["items"].as_array().unwrap() {
            ids.push(ep["id"].as_str().unwrap().to_owned());
        }
    }
    assert_eq!(ids.len(), 3);
    let guests = value(
        f.request("POST", "/api/v1/profiles", &[], json!({"name":"Guests"}))
            .await,
    )
    .await;
    let guest = guests["id"].as_str().unwrap();
    assert_ne!(guest, f.user);
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/playback",
            &[],
            json!({"item_id":f.item,"position_ticks":200,"profile_id":guest})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/playback",
            &[],
            json!({"item_id":ids[0],"position_ticks":300})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let default = value(
        f.request("GET", "/api/v1/continue-watching", &[], Value::Null)
            .await,
    )
    .await;
    assert_eq!(default["total"], 1);
    assert_eq!(default["items"][0]["kind"], "episode");
    let custom = value(
        f.request(
            "GET",
            &format!("/api/v1/continue-watching?profile_id={guest}"),
            &[],
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(custom["total"], 1);
    assert_eq!(custom["items"][0]["kind"], "movie");
    // End-of-title and explicit completion both leave Continue Watching.
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/playback",
            &[],
            json!({"item_id":f.item,"position_ticks":1_190_000_000_i64,"profile_id":guest})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let custom = value(
        f.request(
            "GET",
            &format!("/api/v1/continue-watching?profile_id={guest}"),
            &[],
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(custom["total"], 0);
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/playback",
            &[],
            json!({"item_id":ids[0],"position_ticks":300,"completed":true})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        value(
            f.request("GET", "/api/v1/continue-watching", &[], Value::Null)
                .await
        )
        .await["total"],
        0
    );
    // Foreign account profiles must never be read, updated, deleted or written.
    sqlx::query(
        "INSERT INTO users(id,username,password_hash) VALUES('foreign','Foreign','fixture')",
    )
    .execute(&f.state.pool)
    .await
    .unwrap();
    for (method, path, body) in [
        (
            "GET",
            "/api/v1/continue-watching?profile_id=foreign",
            Value::Null,
        ),
        ("GET", "/api/v1/profiles/foreign", Value::Null),
        (
            "PATCH",
            "/api/v1/profiles/foreign",
            json!({"name":"Stolen"}),
        ),
        ("DELETE", "/api/v1/profiles/foreign", Value::Null),
        (
            "POST",
            "/api/v1/playback",
            json!({"item_id":f.item,"position_ticks":1,"profile_id":"foreign"}),
        ),
    ] {
        assert_eq!(
            f.request(method, path, &[], body).await.status(),
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        f.request("POST", "/api/v1/profiles", &[], json!({"name":" "}))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        f.request("POST", "/api/v1/profiles", &[], json!({"name":"Guests"}))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        f.request(
            "PATCH",
            &format!("/api/v1/profiles/{guest}"),
            &[],
            json!({"name":"Visitors"})
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        f.request(
            "DELETE",
            &format!("/api/v1/profiles/{}", f.user),
            &[],
            Value::Null
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        f.request(
            "DELETE",
            &format!("/api/v1/profiles/{guest}"),
            &[],
            Value::Null
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let remaining: i64 =
        sqlx::query_scalar("SELECT count(*) FROM playback_history WHERE profile_id=?")
            .bind(guest)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
    assert_eq!(remaining, 0);
    for url in [
        "/api/v1/shows/missing/seasons",
        "/api/v1/seasons/missing/episodes",
        "/api/v1/movies/missing",
    ] {
        assert_eq!(
            f.request("GET", url, &[], Value::Null).await.status(),
            StatusCode::NOT_FOUND
        );
    }
    let page = value(
        f.request("GET", "/api/v1/movies?offset=10&limit=1", &[], Value::Null)
            .await,
    )
    .await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn cinematic_settings_are_admin_only_masked_atomic_and_dynamic() {
    let f = Fixture::new().await;
    for mode in [
        "auto",
        "software",
        "vaapi",
        "nvenc",
        "qsv",
        "amf",
        "videotoolbox",
    ] {
        assert_eq!(
            f.request("POST", "/api/v1/settings", &[], json!({"hw_accel":mode}))
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            kodiac::api::v1::settings::effective(&f.state)
                .await
                .unwrap()
                .config
                .hw_accel,
            mode
        );
    }
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/settings",
            &[],
            json!({"hw_accel":"unknown"})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let response=f.request("POST","/api/v1/settings",&[],json!({"tmdb_api_key":"private-fixture-key","hw_accel":"software","audio_language_preference":"deu"})).await;
    assert_eq!(response.status(), StatusCode::OK);
    let masked = value(response).await;
    assert!(!masked.to_string().contains("private-fixture-key"));
    assert_eq!(masked["tmdb_configured"], true);
    let effective = kodiac::api::v1::settings::effective(&f.state)
        .await
        .unwrap();
    assert_eq!(
        effective.config.tmdb_token.as_deref(),
        Some("private-fixture-key")
    );
    assert_eq!(
        value(
            f.request("GET", "/api/v1/playback-defaults", &[], Value::Null)
                .await
        )
        .await["audio_language_preference"],
        "deu"
    );
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/settings",
            &[],
            json!({"audio_language_preference":"eng","unexpected":"no"})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        value(
            f.request("GET", "/api/v1/playback-defaults", &[], Value::Null)
                .await
        )
        .await["audio_language_preference"],
        "deu"
    );
    sqlx::query("UPDATE users SET is_admin=0 WHERE id=?")
        .bind(&f.user)
        .execute(&f.state.pool)
        .await
        .unwrap();
    for path in ["/api/v1/settings", "/api/v1/settings/stats"] {
        assert_eq!(
            f.request("GET", path, &[], Value::Null).await.status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        f.request("POST", "/api/v1/settings/rescan", &[], json!({}))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.request("POST", "/api/v1/scan", &[], Value::Null)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn cinematic_hub_search_people_and_next_episode_are_profile_scoped() {
    let f = Fixture::new().await;
    sqlx::query("UPDATE media_items SET rating=8.4 WHERE id=?")
        .bind(&f.item)
        .execute(&f.state.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO people(name,bio) VALUES('Jane Director','Local biography')")
        .execute(&f.state.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_people(item_id,name,role) VALUES(?,'Jane Director','Director')")
        .bind(&f.item)
        .execute(&f.state.pool)
        .await
        .unwrap();
    f.request(
        "POST",
        "/api/v1/playback",
        &[],
        json!({"item_id":f.item,"position_ticks":150000000}),
    )
    .await;
    let response = f.request("GET", "/api/v1/hub/home", &[], Value::Null).await;
    assert_eq!(response.status(), StatusCode::OK);
    let hub = value(response).await;
    assert_eq!(hub["top_rated"][0]["rating"], 8.4);
    assert_eq!(hub["continue_watching"][0]["position_ticks"], 150000000);
    assert_eq!(hub["recently_added"][0]["id"], f.item);
    assert_eq!(
        f.request(
            "GET",
            "/api/v1/hub/home?profile_id=someone-else",
            &[],
            Value::Null
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let search = value(
        f.request("GET", "/api/v1/search?q=Smpl", &[], Value::Null)
            .await,
    )
    .await;
    assert_eq!(search["titles"][0]["id"], f.item);
    let people = value(
        f.request("GET", "/api/v1/search?q=Jane", &[], Value::Null)
            .await,
    )
    .await;
    assert_eq!(people["people"][0]["name"], "Jane Director");
    let person = value(
        f.request("GET", "/api/v1/people/Jane%20Director", &[], Value::Null)
            .await,
    )
    .await;
    assert_eq!(person["bio"], "Local biography");
    assert_eq!(person["items"][0]["id"], f.item);
    let lib: String = sqlx::query_scalar("SELECT library_id FROM media_items LIMIT 1")
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    for (id, season, ep) in [("ep1", 1, 1), ("ep2", 1, 2), ("ep3", 2, 1)] {
        sqlx::query("INSERT INTO media_items(id,library_id,path,kind,title,series_name,season,episode,container,size_bytes,modified_ns,scan_generation) VALUES(?,?,?,'episode',?,'Show',?,?,'mp4',1,'1','test')").bind(id).bind(&lib).bind(format!("/fixture/{id}.mp4")).bind(id).bind(season).bind(ep).execute(&f.state.pool).await.unwrap();
    }
    let exp = value(
        f.request("GET", "/api/v1/items/ep2/experience", &[], Value::Null)
            .await,
    )
    .await;
    assert_eq!(exp["next_episode"]["id"], "ep3");
    assert!(
        value(
            f.request("GET", "/api/v1/items/ep3/experience", &[], Value::Null)
                .await
        )
        .await["next_episode"]
            .is_null()
    );
}

#[tokio::test]
#[ignore = "requires real ffmpeg"]
async fn cinematic_real_frames_are_cached_and_served_without_overwriting_artwork() {
    let f = Fixture::new().await;
    let item = db::item(&f.state.pool, &f.item).await.unwrap();
    let status = tokio::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=orange:s=640x360:d=2",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&item.path)
        .status()
        .await
        .unwrap();
    assert!(status.success());
    sqlx::query("UPDATE media_items SET runtime_seconds=2 WHERE id=?")
        .bind(&f.item)
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert!(scanner::frames::run_once(&f.state).await.unwrap());
    let updated = db::item(&f.state.pool, &f.item).await.unwrap();
    let path = updated.poster_path.unwrap();
    assert!(path.contains("/extracted/"));
    assert_eq!(Some(path.clone()), updated.backdrop_path);
    for suffix in ["poster", "backdrop"] {
        let response = f
            .request(
                "GET",
                &format!("/api/v1/items/{}/{suffix}", f.item),
                &[],
                Value::Null,
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let data = to_bytes(response.into_body(), 10_000_000).await.unwrap();
        assert!(data.starts_with(&[255, 216, 255]));
    }
    assert!(!scanner::frames::run_once(&f.state).await.unwrap());
    assert_eq!(
        db::item(&f.state.pool, &f.item).await.unwrap().poster_path,
        Some(path)
    );
}
