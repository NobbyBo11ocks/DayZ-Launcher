//! Live A2S fan-out benchmark (M2). Ignored by default. Run with:
//!   DAYZ_SWEEP_LIST=<file with one ip:port per line> cargo test --test live_sweep -- --ignored --nocapture
//! Optional: DAYZ_SWEEP_CONCURRENCY (default 256), DAYZ_SWEEP_TIMEOUT_MS (default 1000),
//! DAYZ_SWEEP_RETRIES (default 0), DAYZ_SWEEP_LIMIT.
//! Prints throughput, RTT percentiles, error classes, a players histogram and
//! "fake population" suspects. Budgets are reported, not asserted: results depend
//! on the caller's NAT and on how many listed addresses are dead (D-037).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use dayz_launcher_lib::a2s::{A2sError, Client};

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "queries thousands of live servers"]
async fn live_sweep() {
    let path = std::env::var("DAYZ_SWEEP_LIST").expect("DAYZ_SWEEP_LIST=<file> is required");
    let text = std::fs::read_to_string(&path).expect("read list");
    let limit: usize = env_or("DAYZ_SWEEP_LIMIT", usize::MAX);
    let addrs: Vec<SocketAddr> = text
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .take(limit)
        .collect();
    let concurrency: usize = env_or("DAYZ_SWEEP_CONCURRENCY", 256);
    let timeout_ms: u64 = env_or("DAYZ_SWEEP_TIMEOUT_MS", 1000);
    let retries: u8 = env_or("DAYZ_SWEEP_RETRIES", 0);
    let pps: u32 = env_or("DAYZ_SWEEP_PPS", 400);
    let client = Client::new(concurrency)
        .with_timeout(Duration::from_millis(timeout_ms))
        .with_retries(retries)
        .with_rate(pps);

    let t0 = Instant::now();
    let results = client.info_many(addrs.iter().copied()).await;
    let elapsed = t0.elapsed();

    let mut ok = 0usize;
    let mut errs: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut first_io: Option<String> = None;
    let mut rtts: Vec<u128> = Vec::new();
    let mut dayz = 0usize;
    let mut players_sum = 0u64;
    let mut hist: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut over_max = 0usize;
    let mut suspects: Vec<String> = Vec::new();
    let mut versions: BTreeMap<String, usize> = BTreeMap::new();
    for (addr, r) in &results {
        match r {
            Ok(reply) => {
                ok += 1;
                rtts.push(reply.rtt.as_millis());
                let i = &reply.value;
                if i.app_id == 221_100 {
                    dayz += 1;
                }
                players_sum += i.players as u64;
                let bucket = match i.players {
                    0 => "0",
                    1..=10 => "1-10",
                    11..=30 => "11-30",
                    31..=60 => "31-60",
                    61..=127 => "61-127",
                    _ => "128+",
                };
                *hist.entry(bucket).or_default() += 1;
                if i.players > i.max_players {
                    over_max += 1;
                }
                if i.players >= 60 && suspects.len() < 8 {
                    suspects.push(format!(
                        "{addr} {}/{} {:?}",
                        i.players,
                        i.max_players,
                        i.name.chars().take(40).collect::<String>()
                    ));
                }
                *versions.entry(i.version.clone()).or_default() += 1;
            }
            Err(e) => {
                let k = match e {
                    A2sError::Timeout => "timeout",
                    A2sError::Unreachable => "unreachable",
                    A2sError::Truncated { .. } => "truncated",
                    A2sError::UnexpectedType { .. } => "unexpected-type",
                    A2sError::BadHeader(_) => "bad-header",
                    A2sError::Io(io) => {
                        first_io.get_or_insert_with(|| format!("{addr}: {io}"));
                        "io"
                    }
                    _ => "other",
                };
                *errs.entry(k).or_default() += 1;
            }
        }
    }
    rtts.sort_unstable();
    let pct = |p: f64| {
        rtts.get(((rtts.len() as f64 - 1.0) * p) as usize)
            .copied()
            .unwrap_or(0)
    };
    let mut top: Vec<_> = versions.into_iter().collect();
    top.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let budget = Duration::from_secs_f64(15.0 * addrs.len() as f64 / 20_000.0);

    println!(
        "sweep: {} addresses, concurrency {}, timeout {} ms, retries {}, pacing {} pps",
        addrs.len(),
        concurrency,
        timeout_ms,
        retries,
        pps
    );
    println!(
        "elapsed: {:.2} s ({:.0} addr/s); pro-rata 20k/15s budget would be {:.1} s",
        elapsed.as_secs_f64(),
        addrs.len() as f64 / elapsed.as_secs_f64(),
        budget.as_secs_f64()
    );
    println!(
        "ok: {ok} (dayz app id: {dayz})  errors: {errs:?}  first io: {}",
        first_io.as_deref().unwrap_or("-")
    );
    println!(
        "rtt ms: p50 {} p90 {} p99 {} max {}",
        pct(0.5),
        pct(0.9),
        pct(0.99),
        rtts.last().copied().unwrap_or(0)
    );
    println!("players: sum {players_sum}, histogram {hist:?}, players>max {over_max}");
    println!("suspects (players>=60): {suspects:#?}");
    println!("top versions: {:?}", &top[..top.len().min(5)]);

    assert!(ok > 0, "no server answered");
}
