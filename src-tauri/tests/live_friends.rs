//! Friends live check (ignored by default, D-092/D-096): opens a Steamworks session
//! as app 221100 and prints, for every friend in a game, the raw
//! `GetFriendGamePlayed` values and every rich-presence key/value Steam holds for
//! them (`GetFriendRichPresenceKeyCount/KeyByIndex`, flat API), before and after a
//! `RequestFriendRichPresence` with three seconds of callbacks. Then compares with
//! what the worker's `friends()` reports. Run with:
//!   cargo test --test live_friends -- --ignored --nocapture

use std::ffi::{CStr, CString};
use std::time::{Duration, Instant};

use dayz_launcher_lib::steam::sdk::{SteamEvent, SteamWorker};
use steamworks::{sys, Client, Friend, FriendFlags};

fn presence_dump(f: &Friend) -> Vec<(String, String)> {
    // SAFETY: the flat accessor returns the live ISteamFriends of this process's
    // session; the key pointers are valid until the next Steamworks call.
    unsafe {
        let ptr = sys::SteamAPI_SteamFriends_v018();
        let id = f.id().raw();
        let n = sys::SteamAPI_ISteamFriends_GetFriendRichPresenceKeyCount(ptr, id);
        (0..n)
            .filter_map(|i| {
                let k = sys::SteamAPI_ISteamFriends_GetFriendRichPresenceKeyByIndex(ptr, id, i);
                if k.is_null() {
                    return None;
                }
                let key = CStr::from_ptr(k).to_string_lossy().into_owned();
                let ck = CString::new(key.clone()).ok()?;
                let v = sys::SteamAPI_ISteamFriends_GetFriendRichPresence(ptr, id, ck.as_ptr());
                let value = if v.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(v).to_string_lossy().into_owned()
                };
                Some((key, value))
            })
            .collect()
    }
}

#[test]
#[ignore = "reads the live Steam friends list"]
fn friends_raw_and_worker() {
    let client = Client::init_app(221_100).expect("Steam init");
    let friends = client.friends();
    let list = friends.get_friends(FriendFlags::IMMEDIATE);
    let playing: Vec<&Friend> = list.iter().filter(|f| f.game_played().is_some()).collect();
    for f in &playing {
        let g = f.game_played().expect("in a game");
        println!(
            "raw: {:<28} app {:>7} addr {}:{} query {} lobby {:?} state {:?} presence(before) {:?}",
            f.name(),
            g.game.app_id().0,
            g.game_address,
            g.game_port,
            g.query_port,
            g.lobby,
            f.state(),
            presence_dump(f)
        );
        // SAFETY: same live interface pointer as above.
        unsafe {
            sys::SteamAPI_ISteamFriends_RequestFriendRichPresence(
                sys::SteamAPI_SteamFriends_v018(),
                f.id().raw(),
            );
        }
    }
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(3) {
        client.run_callbacks();
        std::thread::sleep(Duration::from_millis(50));
    }
    for f in &playing {
        println!(
            "presence(after): {:<28} connect={:?} status={:?} all={:?}",
            f.name(),
            f.rich_presence("connect"),
            f.rich_presence("status"),
            presence_dump(f)
        );
    }
    println!("{} friends, {} in a game", list.len(), playing.len());
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
