use clap::Parser;
use kodiac::{auth, config::Config, db};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn killing_metadata_worker_preserves_http_and_database() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let password = dir.path().join("password");
    std::fs::write(&password, "isolation-test-password").unwrap();
    let database = dir.path().join("media.db");
    let config = Config::parse_from([
        "kodiac",
        "--media-path",
        media.to_str().unwrap(),
        "--db-path",
        database.to_str().unwrap(),
        "--admin-password-file",
        password.to_str().unwrap(),
    ]);
    let pool = db::connect(&config).await.unwrap();
    auth::bootstrap(&pool, &config).await.unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    drop(listener);
    let args = [
        "--media-path",
        media.to_str().unwrap(),
        "--db-path",
        database.to_str().unwrap(),
        "--metadata-path",
        dir.path().to_str().unwrap(),
        "--port",
        &port,
        "--scan-interval-seconds",
        "0",
    ];
    let mut server = Process(
        Command::new(env!("CARGO_BIN_EXE_kodiac"))
            .args(args)
            .args(["--role", "server"])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let mut ready = false;
    for _ in 0..60 {
        if client
            .get(format!("{base}/api/v1/live"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "HTTP process failed to start");
    let mut worker = Process(
        Command::new(env!("CARGO_BIN_EXE_kodiac"))
            .args(args)
            .args(["--role", "metadata"])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let mut ready = false;
    for _ in 0..60 {
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM worker_status WHERE name='metadata'")
                .fetch_one(&pool)
                .await
                .unwrap();
        if count == 1 {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "metadata process failed to start");
    worker.0.kill().unwrap();
    worker.0.wait().unwrap();
    for path in ["/api/v1/live", "/api/v1/health", "/"] {
        assert!(
            client
                .get(format!("{base}{path}"))
                .send()
                .await
                .unwrap()
                .status()
                .is_success()
        );
    }
    assert!(server.0.try_wait().unwrap().is_none());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
}
