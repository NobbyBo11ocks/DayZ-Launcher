//! Mod management live check (ignored by default, D-075): Steam init → subscribe and
//! download one small Workshop item → unsubscribe it through the worker → the
//! callback must confirm. Steam removes the files on its own afterwards (usually once
//! nothing runs as app 221100), so the folder state is printed, not asserted. Run with:
//!   cargo test --test live_m8 -- --ignored --nocapture
//! Optional: DAYZ_TEST_MOD=<workshop id> (default 1819514788, "Ear Plugs", ~1 MB).

use std::time::{Duration, Instant};

use dayz_launcher_lib::steam::sdk::{SteamEvent, SteamWorker};

#[test]
#[ignore = "subscribes to and unsubscribes from a Workshop item on this machine"]
fn unsubscribe_roundtrip() {
    let mod_id: u64 = std::env::var("DAYZ_TEST_MOD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_819_514_788);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SteamEvent>();
    let steam = SteamWorker::spawn(tx, None, None);
    let t0 = Instant::now();
    loop {
        match rx.try_recv() {
            Ok(SteamEvent::Status(s)) if s.initialized => {
                println!("steam ready in {:?}: persona {:?}", t0.elapsed(), s.persona);
                break;
            }
            Ok(SteamEvent::Status(s)) if s.error.is_some() => {
                panic!("steam init failed: {:?}", s.error)
            }
            _ => {}
        }
        assert!(
            t0.elapsed() < Duration::from_secs(15),
            "steam did not initialise"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // Subscribe + download so there is something to unsubscribe from.
    let t1 = Instant::now();
    steam.sync(8, vec![mod_id]).expect("sync queued");
    let done = loop {
        match rx.try_recv() {
            Ok(SteamEvent::SyncDone(d)) => break d,
            Ok(SteamEvent::SyncProgress(p)) => {
                if let Some(it) = p.items.first() {
                    println!(
                        "  {:>5.1}s {} {}/{}",
                        t1.elapsed().as_secs_f64(),
                        it.state,
                        it.downloaded,
                        it.total
                    );
                }
            }
            _ => {}
        }
        assert!(
            t1.elapsed() < Duration::from_secs(600),
            "sync did not finish in 10 min"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(done.ok, "sync failed: {:?}", done.error);
    println!(
        "installed in {:?}: {:?}",
        t1.elapsed(),
        done.items.first().and_then(|i| i.folder.clone())
    );

    // Unsubscribe through the worker (same path as the mods_unsubscribe command).
    let t2 = Instant::now();
    let results = steam.unsubscribe(&[mod_id]).expect("unsubscribe answered");
    println!("unsubscribe answered in {:?}: {:?}", t2.elapsed(), results);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, mod_id);
    assert!(results[0].1.is_ok(), "Steam refused: {:?}", results[0].1);

    // Informational: whether the content folder is still there a moment later
    // (Steam removes it on its own schedule, so this is printed, not asserted).
    std::thread::sleep(Duration::from_secs(3));
    if let Some(folder) = done.items.first().and_then(|i| i.folder.clone()) {
        println!(
            "content folder still exists: {}",
            std::path::Path::new(&folder).exists()
        );
    }
    drop(steam);
}
