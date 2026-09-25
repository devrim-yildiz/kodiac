//! Losslessness and referential integrity of the beta -> hierarchy upgrade.
use sqlx::{Connection, Row, SqliteConnection};

async fn beta() -> SqliteConnection {
    let mut c = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON;")
        .execute(&mut c)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0001_initial.sql"))
        .execute(&mut c)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0002_beta.sql"))
        .execute(&mut c)
        .await
        .unwrap();
    sqlx::raw_sql("INSERT INTO users(id,username,password_hash) VALUES('u','Owner','fixture'); INSERT INTO libraries(id,name,path) VALUES('a','A','/a'),('b','B','/b');").execute(&mut c).await.unwrap();
    c
}
#[tokio::test]
async fn all_5172_records_and_progress_survive_upgrade_and_rescans() {
    let mut c = beta().await;
    sqlx::raw_sql("BEGIN").execute(&mut c).await.unwrap();
    for n in 0..5172 {
        sqlx::query("INSERT INTO media_items(id,library_id,path,kind,title,plot,year,series_name,season,episode,poster_path,backdrop_path,container,streams_json,size_bytes,modified_ns,scan_generation,available) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(format!("id{n}")).bind(if n%7==0 {"b"} else {"a"}).bind(format!("/file{n}.mkv"))
            .bind(if n%3==0 {"movie"} else {"episode"}).bind(format!("Title {n}"))
            .bind(format!("Synopsis {n}")).bind(2025_i64)
            .bind(if n%11==0 {None} else {Some(if n%2==0 {"Alien: Earth"} else {"Other show"})})
            .bind(if n%5==0 {None} else {Some(n%3)})
            .bind(if n%13==0 {None} else {Some(n%12)})
            .bind("/poster.jpg").bind("/backdrop.jpg").bind("mkv").bind("[{\"codec_type\":\"audio\"}]")
            .bind(n+1).bind(n.to_string()).bind("generation").bind(n%17!=0).execute(&mut c).await.unwrap();
    }
    sqlx::raw_sql("INSERT INTO playback_sessions(id,user_id,media_item_id,position_ticks,is_paused,stopped,updated_at) VALUES('p1','u','id1',12345,1,0,123),('p2','u','id3',98765,0,1,456); COMMIT").execute(&mut c).await.unwrap();
    let columns = sqlx::query("PRAGMA table_info(media_items)")
        .fetch_all(&mut c)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect::<Vec<_>>()
        .join(",");
    let snapshot_sql = format!("SELECT json_array({columns}) FROM media_items ORDER BY id");
    let before: Vec<String> = sqlx::query_scalar(&snapshot_sql)
        .fetch_all(&mut c)
        .await
        .unwrap();
    sqlx::raw_sql("BEGIN").execute(&mut c).await.unwrap();
    sqlx::raw_sql(include_str!("../migrations/0003_relational_catalog.sql"))
        .execute(&mut c)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0004_spa_metadata.sql"))
        .execute(&mut c)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0005_cinematic.sql"))
        .execute(&mut c)
        .await
        .unwrap();
    sqlx::raw_sql("COMMIT").execute(&mut c).await.unwrap();
    let after: Vec<String> = sqlx::query_scalar(&snapshot_sql)
        .fetch_all(&mut c)
        .await
        .unwrap();
    assert_eq!(before.len(), 5172);
    // Byte-for-byte JSON comparison covers every original column, including NULLs.
    assert_eq!(before, after);
    let count: i64 =
        sqlx::query_scalar("SELECT (SELECT count(*) FROM movies)+(SELECT count(*) FROM episodes)")
            .fetch_one(&mut c)
            .await
            .unwrap();
    assert_eq!(count, 5172);
    let orphan:i64=sqlx::query_scalar("SELECT count(*) FROM media_items i LEFT JOIN movies m ON m.id=i.id LEFT JOIN episodes e ON e.id=i.id WHERE (i.kind='movie' AND m.id IS NULL) OR (i.kind='episode' AND e.id IS NULL) OR (m.id IS NOT NULL AND e.id IS NOT NULL)").fetch_one(&mut c).await.unwrap();
    assert_eq!(orphan, 0);
    let progress:Vec<(String,i64,bool,bool,i64)>=sqlx::query_as("SELECT media_item_id,position_ticks,is_paused,stopped,updated_at FROM playback_history WHERE profile_id='u' ORDER BY media_item_id").fetch_all(&mut c).await.unwrap();
    assert_eq!(
        progress,
        vec![
            ("id1".into(), 12345, true, false, 123),
            ("id3".into(), 98765, false, true, 456)
        ]
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut c)
            .await
            .unwrap()
            .is_empty()
    );
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut c)
        .await
        .unwrap();
    assert_eq!(integrity, "ok");
    let show_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM shows ORDER BY id")
        .fetch_all(&mut c)
        .await
        .unwrap();
    sqlx::query("UPDATE media_items SET scan_generation='rescan',available=1")
        .execute(&mut c)
        .await
        .unwrap();
    let rescanned: Vec<String> = sqlx::query_scalar("SELECT id FROM shows ORDER BY id")
        .fetch_all(&mut c)
        .await
        .unwrap();
    assert_eq!(show_ids, rescanned);
    // Reclassification and deletion cannot leave a playable item in both catalogs.
    sqlx::query("UPDATE media_items SET kind='episode',series_name='New Show',season=0,episode=1 WHERE id='id3'").execute(&mut c).await.unwrap();
    let exists: i64 = sqlx::query_scalar("SELECT count(*) FROM movies WHERE id='id3'")
        .fetch_one(&mut c)
        .await
        .unwrap();
    assert_eq!(exists, 0);
    sqlx::query("DELETE FROM media_items WHERE id='id3'")
        .execute(&mut c)
        .await
        .unwrap();
    let exists: i64 =
        sqlx::query_scalar("SELECT count(*) FROM playback_history WHERE media_item_id='id3'")
            .fetch_one(&mut c)
            .await
            .unwrap();
    assert_eq!(exists, 0);
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut c)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn migration_rolls_back_as_a_unit() {
    let mut c = beta().await;
    sqlx::raw_sql("BEGIN").execute(&mut c).await.unwrap();
    sqlx::raw_sql(include_str!("../migrations/0003_relational_catalog.sql"))
        .execute(&mut c)
        .await
        .unwrap();
    sqlx::raw_sql("ROLLBACK").execute(&mut c).await.unwrap();
    let exists: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='profiles'",
    )
    .fetch_one(&mut c)
    .await
    .unwrap();
    assert_eq!(exists, 0);
    let cols = sqlx::query("PRAGMA table_info(media_items)")
        .fetch_all(&mut c)
        .await
        .unwrap();
    assert!(
        !cols
            .iter()
            .any(|c| c.get::<String, _>("name") == "local_metadata")
    );
}

#[tokio::test]
#[ignore = "requires MIGRATION_FIXTURE_DB pointing to a consistent, private beta snapshot"]
async fn real_beta_snapshot_migrates_losslessly_and_reopens_idempotently() {
    use clap::Parser;
    use medienserver::{config::Config, db};
    let source = std::env::var("MIGRATION_FIXTURE_DB")
        .expect("MIGRATION_FIXTURE_DB must name a private SQLite snapshot");
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("upgrade.db");
    tokio::fs::copy(source, &copy).await.unwrap();
    let mut original = SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&copy)
            .read_only(true),
    )
    .await
    .unwrap();
    let columns = sqlx::query("PRAGMA table_info(media_items)")
        .fetch_all(&mut original)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect::<Vec<_>>()
        .join(",");
    let snapshot = format!("SELECT json_array({columns}) FROM media_items ORDER BY id");
    let before: Vec<String> = sqlx::query_scalar(&snapshot)
        .fetch_all(&mut original)
        .await
        .unwrap();
    let sessions:Vec<String>=sqlx::query_scalar("SELECT json_array(id,user_id,media_item_id,position_ticks,is_paused,stopped,updated_at) FROM playback_sessions ORDER BY id").fetch_all(&mut original).await.unwrap();
    original.close().await.unwrap();
    let config = Config::parse_from([
        "medienserver",
        "--media-path",
        dir.path().to_str().unwrap(),
        "--db-path",
        copy.to_str().unwrap(),
    ]);
    let pool = db::connect(&config).await.unwrap();
    let after: Vec<String> = sqlx::query_scalar(&snapshot)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(
        before == after,
        "source media columns changed during migration"
    );
    let sessions_after:Vec<String>=sqlx::query_scalar("SELECT json_array(id,user_id,media_item_id,position_ticks,is_paused,stopped,updated_at) FROM playback_sessions ORDER BY id").fetch_all(&pool).await.unwrap();
    assert!(
        sessions == sessions_after,
        "legacy playback rows changed during migration"
    );
    let unmapped:i64=sqlx::query_scalar("SELECT count(*) FROM playback_sessions p LEFT JOIN profiles pr ON pr.user_id=p.user_id AND pr.is_default=1 LEFT JOIN playback_history h ON h.profile_id=pr.id AND h.media_item_id=p.media_item_id WHERE h.media_item_id IS NULL OR h.position_ticks<>p.position_ticks OR h.is_paused<>p.is_paused OR h.stopped<>p.stopped OR h.updated_at<>p.updated_at").fetch_one(&pool).await.unwrap();
    assert_eq!(unmapped, 0);
    let counts:(i64,i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM movies),(SELECT count(*) FROM shows),(SELECT count(*) FROM seasons),(SELECT count(*) FROM episodes),(SELECT count(*) FROM playback_history)").fetch_one(&pool).await.unwrap();
    assert_eq!(counts.0 + counts.3, before.len() as i64);
    assert_eq!(counts.4, sessions.len() as i64);
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .unwrap(),
        "ok"
    );
    pool.close().await;
    let pool = db::connect(&config).await.unwrap();
    let reopened: i64 =
        sqlx::query_scalar("SELECT (SELECT count(*) FROM movies)+(SELECT count(*) FROM episodes)")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(reopened, before.len() as i64);
    println!(
        "Verified real beta snapshot: {} source records -> {} movies, {} shows, {} seasons, {} episodes; {} playback rows retained. Integrity/FKs/reopen OK.",
        before.len(),
        counts.0,
        counts.1,
        counts.2,
        counts.3,
        counts.4
    );
    pool.close().await;
}
