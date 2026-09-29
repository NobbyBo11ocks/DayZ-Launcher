//! The mod scan's RULES reads against live servers, at the scan's own pace. Ignored by
//! default. Run with:
//!   DAYZ_RULES_LIST=<file with one ip:port per line> cargo test --test live_rules -- --ignored --nocapture
//! INFO first, since the scan's targets have answered a check; then RULES for those that
//! answered, 64 at once at 100 datagrams a second with one retry (`run_mod_scan`); then
//! every failure once more, one at a time with a 3 s deadline. The error classes and the
//! addresses tell a server that never answers from one the scan's pace loses. One NAT
//! flow per address: keep the list short (D-037).
//!
//! With `DAYZ_RULES_BURST=<file>` the RULES reads follow a start's load instead: INFO and
//! PLAYER to every address in that file, 128 at once at 400 datagrams a second (Steam's
//! refresh pings and the check pass), and the failures get the scan's second look 30 s
//! later (D-329) rather than the slow one. That is the load of one launcher start.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use dayz_launcher_lib::a2s::{A2sError, Client, Reply, Rules};

fn class(e: &A2sError) -> &'static str {
    match e {
        A2sError::Timeout => "timeout",
        A2sError::Unreachable => "unreachable",
        A2sError::Compressed => "compressed split",
        A2sError::ChallengeLoop => "challenge loop",
        A2sError::SplitMismatch(_) => "split mismatch",
        A2sError::Malformed(_) => "malformed",
        A2sError::Truncated { .. } => "truncated",
        A2sError::UnexpectedType { .. } => "unexpected type",
        A2sError::BadHeader(_) => "bad header",
        _ => "other",
    }
}

/// What one read gave: the mod count, "no DayZ data", or the error's class.
fn outcome(r: &Result<Reply<Rules>, A2sError>) -> Result<usize, &'static str> {
    match r {
        Ok(reply) => match &reply.value.dayz {
            Some(_) => Ok(reply.value.stored_mods().len()),
            None => Err("no DayZ data"),
        },
        Err(e) => Err(class(e)),
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "queries live servers"]
async fn live_rules() {
    let path = std::env::var("DAYZ_RULES_LIST").expect("DAYZ_RULES_LIST=<file> is required");
    let addrs: Vec<SocketAddr> = std::fs::read_to_string(&path)
        .expect("read list")
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();
    let info = Client::new(64).with_rate(100).with_retries(1);
    let answered: Vec<SocketAddr> = info
        .info_many(addrs.iter().copied())
        .await
        .into_iter()
        .filter(|(_, r)| r.is_ok())
        .map(|(a, _)| a)
        .collect();
    println!("INFO: {} of {} answered", answered.len(), addrs.len());

    let burst_path = std::env::var("DAYZ_RULES_BURST").ok();
    if let Some(p) = &burst_path {
        let burst: Vec<SocketAddr> = std::fs::read_to_string(p)
            .expect("read burst list")
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .collect();
        let pass = Client::new(128);
        let t = std::time::Instant::now();
        let infos = pass.info_many(burst.iter().copied()).await;
        let alive: Vec<SocketAddr> = infos
            .iter()
            .filter(|(_, r)| r.is_ok())
            .map(|(a, _)| *a)
            .collect();
        let mut set = tokio::task::JoinSet::new();
        for a in alive.iter().copied() {
            let c = pass.clone();
            set.spawn(async move { c.players(a).await.is_ok() });
        }
        let mut players = 0usize;
        while let Some(Ok(ok)) = set.join_next().await {
            players += usize::from(ok);
        }
        println!(
            "burst: INFO to {} servers ({} answered), PLAYER {} answered, in {:?}",
            burst.len(),
            alive.len(),
            players,
            t.elapsed()
        );
    }

    let scan = Client::new(64).with_rate(100).with_retries(1);
    let mut set = tokio::task::JoinSet::new();
    for a in answered.iter().copied() {
        let c = scan.clone();
        set.spawn(async move { (a, c.rules(a).await) });
    }
    let mut read = 0usize;
    let mut failed: Vec<(SocketAddr, &'static str)> = Vec::new();
    while let Some(joined) = set.join_next().await {
        let Ok((a, r)) = joined else { continue };
        match outcome(&r) {
            Ok(_) => read += 1,
            Err(why) => failed.push((a, why)),
        }
    }
    let mut classes: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, why) in &failed {
        *classes.entry(why).or_default() += 1;
    }
    println!(
        "RULES at the scan's pace: {read} read, {} failed {classes:?}",
        failed.len()
    );

    if burst_path.is_some() {
        // The scan's second look, as `second_look` makes it (D-329).
        tokio::time::sleep(Duration::from_secs(30)).await;
        let patient = Client::new(16)
            .with_rate(40)
            .with_timeout(Duration::from_secs(2))
            .with_retries(1);
        let mut set = tokio::task::JoinSet::new();
        for (a, _) in &failed {
            let (a, c) = (*a, patient.clone());
            set.spawn(async move { outcome(&c.rules(a).await).is_ok() });
        }
        let mut recovered = 0usize;
        while let Some(Ok(ok)) = set.join_next().await {
            recovered += usize::from(ok);
        }
        println!(
            "second look 30 s later: {recovered} of {} read",
            failed.len()
        );
        return;
    }

    let slow = Client::new(1)
        .with_timeout(Duration::from_secs(3))
        .with_retries(1);
    let mut again: BTreeMap<String, usize> = BTreeMap::new();
    for (a, first) in &failed {
        let second = outcome(&slow.rules(*a).await);
        let key = match second {
            Ok(_) => format!("{first} -> read"),
            Err(why) => format!("{first} -> {why}"),
        };
        println!("  {a}: {key}");
        *again.entry(key).or_default() += 1;
    }
    println!("once more, one at a time: {again:?}");
}
