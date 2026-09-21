//! DZSA fallback live check (ignored by default, D-089): downloads the public list
//! through the same code the `servers_dzsa` command uses and prints what came back.
//! Run with: cargo test --test live_dzsa -- --ignored --nocapture

use std::time::Instant;

use dayz_launcher_lib::browser::dzsa;

#[test]
#[ignore = "downloads ~24 MB from dayzsalauncher.com"]
fn fetches_and_converts_the_list() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let t0 = Instant::now();
    let rows = rt.block_on(dzsa::fetch()).expect("DZSA fetch");
    let elapsed = t0.elapsed();
    let populated = rows.iter().filter(|r| r.row.players > 0).count();
    let with_mods = rows.iter().filter(|r| !r.mods.is_empty()).count();
    let with_country = rows.iter().filter(|r| r.row.country.is_some()).count();
    let modded_tag = rows.iter().filter(|r| r.row.tags.modded).count();
    println!(
        "{} servers in {:?}: {} populated, {} with mods ({} tagged modded), {} with a country",
        rows.len(),
        elapsed,
        populated,
        with_mods,
        modded_tag,
        with_country
    );
    assert!(rows.len() > 5_000, "suspiciously small list");
    assert!(populated > 500);
    assert_eq!(with_mods, modded_tag, "mod tag must follow the mod list");
    let sample = rows
        .iter()
        .find(|r| r.row.players > 10 && !r.mods.is_empty())
        .expect("a busy modded server");
    println!(
        "sample: {} {} {}/{} v{} tags {:?} mods {}",
        sample.row.id,
        sample.row.name,
        sample.row.players,
        sample.row.max_players,
        sample.row.version,
        sample.row.keywords,
        sample.mods.len()
    );
}
