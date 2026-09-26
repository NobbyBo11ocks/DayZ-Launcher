//! M5 end-to-end acceptance on the real machine (ignored by default):
//! Steam init → Workshop details → download one small mod → junction → launch
//! DayZ_BE.exe against a vanilla server from the cache → confirm the process is
//! alive → kill it. Run with:
//!   cargo test --test live_join -- --ignored --nocapture
//! Optional: DAYZ_TEST_MOD=<workshop id> (default 1819514788, "Ear Plugs", ~1 MB),
//! DAYZ_TEST_SERVER=<ip:queryPort> to override the cache pick, DAYZ_NO_LAUNCH=1 to skip the launch.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use dayz_launcher_lib::browser::Cache;
use dayz_launcher_lib::launch::{build_args, ensure_junctions, spawn, LaunchSpec};
use dayz_launcher_lib::steam::sdk::{SteamEvent, SteamWorker};
use dayz_launcher_lib::steam::{locate, registry, workshop};

#[test]
#[ignore = "downloads a Workshop item and starts DayZ on this machine"]
fn live_join() {
    let mod_id: u64 = std::env::var("DAYZ_TEST_MOD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_819_514_788);

    // --- Steam -------------------------------------------------------------
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

    // --- Workshop details ----------------------------------------------------
    let details = steam.item_details(&[mod_id]).expect("item details");
    assert_eq!(details.len(), 1, "one detail row");
    println!(
        "mod {}: {:?} {} bytes, updated {}",
        mod_id, details[0].title, details[0].file_size, details[0].time_updated
    );

    // --- Sync (subscribe + download) ---------------------------------------
    let t1 = Instant::now();
    steam.sync(42, vec![mod_id], true).expect("sync queued");
    let mut last_state = String::new();
    let done = loop {
        match rx.try_recv() {
            Ok(SteamEvent::SyncProgress(p)) => {
                let it = &p.items[0];
                if it.state != last_state {
                    println!(
                        "  {:>6.1}s {} {}/{} bytes",
                        t1.elapsed().as_secs_f64(),
                        it.state,
                        it.downloaded,
                        it.total
                    );
                    last_state = it.state.clone();
                }
            }
            Ok(SteamEvent::SyncDone(d)) => break d,
            _ => {}
        }
        assert!(
            t1.elapsed() < Duration::from_secs(600),
            "sync did not finish in 10 min"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    println!(
        "sync done ok={} in {} ms: {:?}",
        done.ok, done.elapsed_ms, done.items[0]
    );
    assert!(done.ok, "sync failed: {:?}", done.error);
    let folder = PathBuf::from(done.items[0].folder.clone().expect("install folder"));
    assert!(
        folder.is_dir(),
        "installed folder exists: {}",
        folder.display()
    );

    // --- Junction ------------------------------------------------------------
    let st = registry::detect();
    assert!(st.running, "Steam must be running");
    let libs = locate::libraries(st.path.as_ref().unwrap()).unwrap();
    let game = locate::find_dayz(&libs).unwrap().expect("DayZ installed");
    let ws = workshop::read(&game.library.path)
        .unwrap()
        .expect("workshop manifest");
    let item = ws
        .items
        .iter()
        .find(|i| i.id == mod_id)
        .expect("mod in appworkshop acf");
    println!(
        "acf: meta_name {:?} mod_name {:?} folder {:?}",
        item.meta_name, item.mod_name, item.folder
    );
    let before = workshop::junctions(&game.workshop_dir()).len();
    let links = ensure_junctions(
        &game.folder,
        &[(mod_id, folder.clone(), item.meta_name.clone())],
    )
    .unwrap();
    println!(
        "junction: {} (created: {})",
        links[0].junction.display(),
        links[0].created
    );
    assert!(junction::exists(&links[0].junction).unwrap());
    let after = workshop::junctions(&game.workshop_dir()).len();
    assert!(
        after >= before,
        "no junction was removed ({before} -> {after})"
    );

    if std::env::var("DAYZ_NO_LAUNCH").is_ok() {
        println!("launch skipped (DAYZ_NO_LAUNCH)");
        return;
    }

    // --- Pick a vanilla server -----------------------------------------------
    let (ip, port) = if let Ok(s) = std::env::var("DAYZ_TEST_SERVER") {
        let (ip, _q) = s.split_once(':').expect("ip:queryPort");
        (ip.to_string(), 0u16)
    } else {
        let db = dirs_local()
            .join("com.dayzlauncher.desktop")
            .join("cache.db");
        let cache = Cache::open(&db).expect("open cache");
        let rows = cache.load_all().expect("rows");
        let local = dayz_launcher_lib::steam::version::read(&game.exe()).map(|v| v.game_string());
        let pick = rows
            .iter()
            .filter(|r| !r.tags.modded && !r.password && r.steam_empty == Some(false))
            .filter(|r| {
                r.verdict.as_deref() == Some("verified") && r.verified_players.unwrap_or(0) > 0
            })
            .filter(|r| local.as_deref().is_none_or(|v| v == r.version))
            .min_by_key(|r| r.ping_ms)
            .expect("a verified vanilla server in the cache");
        println!(
            "server: {} ({}:{}) {} players, {} ms, v{}",
            pick.name,
            pick.ip,
            pick.game_port,
            pick.verified_players.unwrap_or(0),
            pick.ping_ms,
            pick.version
        );
        (pick.ip.clone(), pick.game_port)
    };
    if port == 0 {
        panic!("DAYZ_TEST_SERVER needs a cache row; use ip:queryPort of a cached server");
    }

    // --- Launch --------------------------------------------------------------
    let spec = LaunchSpec {
        mod_paths: Vec::new(),
        ip,
        game_port: port,
        password: None,
        profile_name: Some("LauncherTest".into()),
        skip_intro: true,
        no_splash: true,
        no_pause: false,
        perf_args: Vec::new(),
        extra_args: String::new(),
    };
    let args = build_args(&spec);
    let (mut child, launched) = spawn(&game.folder, &args).expect("spawn DayZ_BE.exe");
    println!("launched pid {}: {}", launched.pid, launched.command_line);
    let t2 = Instant::now();
    let mut alive = true;
    while t2.elapsed() < Duration::from_secs(30) {
        if let Ok(Some(status)) = child.try_wait() {
            println!(
                "exited early after {:?} with {:?}",
                t2.elapsed(),
                status.code()
            );
            alive = false;
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    if alive {
        // The BE launcher hands over to DayZ_x64.exe; both are ours to stop.
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &launched.pid.to_string(), "/T", "/F"])
            .status();
        let _ = std::process::Command::new("taskkill")
            .args(["/IM", "DayZ_x64.exe", "/F"])
            .status();
        let _ = child.wait();
        println!("process was alive after 30 s; killed");
    }
    assert!(
        alive,
        "DayZ_BE.exe exited within 30 s (BattlEye/game refused to start)"
    );
}

fn dirs_local() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA"))
}
