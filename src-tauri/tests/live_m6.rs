//! M6 live checks (ignored by default): official favourites file on this machine,
//! direct-connect port probing against the reference server, and the favourites /
//! population tables in a temporary database.
//!   cargo test --test live_m6 -- --ignored --nocapture

use std::net::SocketAddr;
use std::time::Duration;

use dayz_launcher_lib::a2s::Client;
use dayz_launcher_lib::browser::{Cache, ServerRow};
use dayz_launcher_lib::steam::official;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "reads the official launcher's favourites and queries live servers"]
async fn live_m6() {
    // --- official favourites -------------------------------------------------
    let path = official::favourites_path().expect("LOCALAPPDATA");
    let favs = official::read_favourites(&path).expect("read FavouriteServers.xml");
    println!("official favourites at {}: {}", path.display(), favs.len());
    assert!(
        !favs.is_empty(),
        "this machine has at least one official favourite"
    );
    let client = Client::new(8)
        .with_timeout(Duration::from_millis(1500))
        .with_retries(0);
    for f in &favs {
        let addr: SocketAddr = format!("{}:{}", f.query_ip, f.query_port).parse().unwrap();
        match client.info(addr).await {
            Ok(r) => println!(
                "  {} -> live: {} {}/{} {} ms",
                f.name,
                r.value.name,
                r.value.players,
                r.value.max_players,
                r.rtt.as_millis()
            ),
            Err(e) => println!(
                "  {} -> offline now ({e}); would be imported from XML fields",
                f.name
            ),
        }
    }

    // --- direct connect: user types the GAME port of the reference server ---------
    // 51.81.8.81 hosts several servers: 27016 (game 2302) and 27017 (game 2402), so the
    // typed game port must be matched against the INFO reply (same rule as direct_connect).
    let ip: std::net::IpAddr = "51.81.8.81".parse().unwrap();
    let typed_port: u16 = 2402;
    assert!(
        client.info(SocketAddr::new(ip, typed_port)).await.is_err(),
        "the game port does not answer A2S"
    );
    let candidates = [
        typed_port + 1,
        typed_port + 2,
        typed_port + 3,
        27016,
        27017,
        27018,
        27019,
        27020,
        27015,
    ];
    let mut found: Option<ServerRow> = None;
    for &qport in &candidates {
        if let Ok(r) = client.info(SocketAddr::new(ip, qport)).await {
            println!(
                "direct connect: query port {qport} answered ({}), game port {:?}",
                r.value.name, r.value.game_port
            );
            if r.value.game_port == Some(typed_port) {
                found = Some(ServerRow::from_info(
                    &ip.to_string(),
                    qport,
                    &r.value,
                    r.rtt.as_millis() as u32,
                ));
                break;
            }
        }
    }
    let row = found.expect("a query port advertising game port 2402 answered");
    assert_eq!(row.id, "51.81.8.81:27017");
    assert_eq!(
        row.game_port, 2402,
        "game port comes from the INFO EDF field"
    );
    assert_eq!(row.steam_empty, None);
    assert_eq!(row.version, "1.29.163709");
    assert_eq!(row.server_version, 129_163_709);

    // --- favourites + population in a scratch DB --------------------------------
    let db = std::env::temp_dir().join(format!("dzl-m6-{}.db", std::process::id()));
    let mut cache = Cache::open(&db).expect("open temp cache");
    cache.upsert(std::slice::from_ref(&row)).unwrap();
    cache.favourite_set(&row.id, true).unwrap();
    assert_eq!(cache.favourites().unwrap()[0].0, row.id);
    let now = ServerRow::now_unix();
    cache
        .population_add(&[
            (row.id.clone(), now - 3600, 12, 0),
            (row.id.clone(), now, 15, 1),
        ])
        .unwrap();
    let samples = cache.population(&row.id, now - 72 * 3600).unwrap();
    assert_eq!(samples.len(), 2);
    println!("population samples: {samples:?}");
    drop(cache);
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_file(db.with_extension("db-wal"));
    let _ = std::fs::remove_file(db.with_extension("db-shm"));
}
