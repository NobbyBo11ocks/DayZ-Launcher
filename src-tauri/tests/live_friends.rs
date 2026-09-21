//! Friends live check (ignored by default, D-092): opens a Steamworks session as app
//! 221100 and prints, for every friend, the raw `GetFriendGamePlayed` values next
//! to what the worker's `friends()` reports, so the "server known" logic can be
//! compared with Steam's own friends list. Run with:
//!   cargo test --test live_friends -- --ignored --nocapture

use std::time::{Duration, Instant};

use dayz_launcher_lib::steam::sdk::{SteamEvent, SteamWorker};
use steamworks::{Client, FriendFlags};

#[test]
#[ignore = "reads the live Steam friends list"]
fn friends_raw_and_worker() {
    let client = Client::init_app(221_100).expect("Steam init");
    let friends = client.friends();
    let list = friends.get_friends(FriendFlags::IMMEDIATE);
    let mut in_game = 0;
    for f in &list {
        if let Some(g) = f.game_played() {
            in_game += 1;
            println!(
                "raw: {:<28} app {:>7} addr {}:{} query {} lobby {:?} state {:?}",
                f.name(),
                g.game.app_id().0,
                g.game_address,
                g.game_port,
                g.query_port,
                g.lobby,
                f.state()
            );
        }
    }
    println!("{} friends, {} in a game", list.len(), in_game);
    drop(friends);
    drop(client);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SteamEvent>();
    let steam = SteamWorker::spawn(tx, None, None);
    let t0 = Instant::now();
    loop {
        match rx.try_recv() {
            Ok(SteamEvent::Status(s)) if s.initialized => break,
            Ok(SteamEvent::Status(s)) if s.error.is_some() => {
                panic!("steam init failed: {:?}", s.error)
            }
            _ => {
                assert!(t0.elapsed() < Duration::from_secs(20), "steam init timeout");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    let t1 = Instant::now();
    let out = steam.friends().expect("friends()");
    println!("worker: {} friends in {:?}", out.len(), t1.elapsed());
    for f in out.iter().filter(|f| f.in_dayz || f.server.is_some()) {
        println!("worker: {f:?}");
    }
    assert_eq!(out.len(), list.len());
}
