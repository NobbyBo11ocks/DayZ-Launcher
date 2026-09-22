//! Population trust: rules R2–R5 from docs/11-fake-population-detection.md.
//!
//! For each server: one A2S_INFO (fresh reported count, ping, clock) and one
//! A2S_PLAYER (real head-count). The verdict compares the two and inspects the
//! player entries for synthetic patterns.

use std::collections::HashSet;
use std::net::SocketAddr;

use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use crate::a2s::{A2sError, Client, Info, Players};

use super::ServerRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// R2: PLAYER agrees with INFO (difference ≤ 2).
    Verified,
    /// R3: INFO exceeds PLAYER by ≥ 5 or ≥ 20 % of max.
    Inflated,
    /// R4: INFO answers with players > 0 but PLAYER never answers.
    Unverifiable,
    /// R5: PLAYER list looks fabricated (identical durations, all young, or named).
    Synthetic,
    /// Neither INFO nor PLAYER answered.
    Offline,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Verified => "verified",
            Verdict::Inflated => "inflated",
            Verdict::Unverifiable => "unverifiable",
            Verdict::Synthetic => "synthetic",
            Verdict::Offline => "offline",
        }
    }
}

/// What to verify: cached identity plus the last reported numbers as fallback.
#[derive(Debug, Clone)]
pub struct Target {
    pub id: String,
    pub addr: SocketAddr,
    pub reported: i32,
    pub max_players: i32,
}

impl Target {
    pub fn from_row(r: &ServerRow) -> Option<Self> {
        let addr: SocketAddr = format!("{}:{}", r.ip, r.query_port).parse().ok()?;
        Some(Self {
            id: r.id.clone(),
            addr,
            reported: r.players,
            max_players: r.max_players,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verification {
    pub id: String,
    pub verdict: Verdict,
    /// INFO count (fresh when INFO answered, else the cached value).
    pub reported: i32,
    /// PLAYER head-count when PLAYER answered.
    pub verified: Option<i32>,
    pub max_players: i32,
    pub ping_ms: Option<u32>,
    /// Written to the cache by `apply_verifications`, never rendered: the browser
    /// reads the parsed `tags` instead. One 500-server `servers:verified` event was
    /// 202 KB with it — already at the ~200 KB cap docs/04 §4 sets — and 157 KB
    /// without (D-188).
    #[serde(skip_serializing)]
    pub keywords: Option<String>,
    /// Parsed keywords when INFO answered, so the UI can update the row without a parser.
    pub tags: Option<crate::a2s::DayzTags>,
    pub verified_at: i64,
    pub reason: String,
}

/// Pure decision function; `info` is the fresh INFO if it answered.
pub fn judge(
    info: Option<&Info>,
    players: Result<&Players, &A2sError>,
    fallback_reported: i32,
    fallback_max: i32,
) -> (Verdict, Option<i32>, String) {
    let (reported, max) = info
        .map(|i| (i.players as i32, i.max_players as i32))
        .unwrap_or((fallback_reported, fallback_max));
    match players {
        // Without a fresh INFO we cannot tell "down" from "drops PLAYER"; the
        // caller retries offline targets with a longer timeout before deciding.
        Err(_) if info.is_none() => (Verdict::Offline, None, "no PLAYER reply".into()),
        Err(e) => {
            if reported > 0 {
                (
                    Verdict::Unverifiable,
                    None,
                    format!("INFO reports {reported} but PLAYER did not answer ({e})"),
                )
            } else {
                (
                    Verdict::Verified,
                    Some(0),
                    "INFO reports 0 and PLAYER did not answer".into(),
                )
            }
        }
        Ok(p) => {
            let v = p.players.len() as i32;
            if v >= 5 {
                let distinct: HashSet<i64> = p
                    .players
                    .iter()
                    .map(|x| x.duration_secs.round() as i64)
                    .collect();
                let all_young = p.players.iter().all(|x| x.duration_secs < 60.0);
                let named = p.players.iter().any(|x| !x.name.is_empty());
                // "Everyone joined in the last minute" is what an honest server looks
                // like just after its scheduled restart, so on its own it no longer
                // flags (D-160): it was hiding real servers every few hours. What gives
                // a fabricated list away is repetition — many players sharing very few
                // distinct durations — so young sessions only count when they also
                // repeat.
                let repetitive = distinct.len() * 3 <= v as usize;
                if distinct.len() <= 2 || named || (all_young && repetitive) {
                    return (
                        Verdict::Synthetic,
                        Some(v),
                        format!(
                            "{v} entries, {} distinct durations, all_young={all_young}, named={named}",
                            distinct.len()
                        ),
                    );
                }
            }
            let diff = reported - v;
            if diff >= 5 || (max > 0 && diff * 5 >= max && diff > 0) {
                (
                    Verdict::Inflated,
                    Some(v),
                    format!("INFO {reported} vs PLAYER {v}"),
                )
            } else {
                (
                    Verdict::Verified,
                    Some(v),
                    format!("INFO {reported} vs PLAYER {v}"),
                )
            }
        }
    }
}

/// Verifies many servers under the client's concurrency and pacing bounds.
/// `with_info` also refreshes INFO (ping, clock, reported count); the automatic
/// post-refresh pass passes `false` because Steam just delivered fresh INFO and
/// halving the datagrams keeps the burst small (D-047). Results are in completion order.
pub async fn verify_many(
    client: &Client,
    targets: Vec<Target>,
    with_info: bool,
) -> Vec<Verification> {
    let mut set = JoinSet::new();
    for t in targets {
        let c = client.clone();
        set.spawn(async move { verify_one(&c, t, with_info).await });
    }
    let mut out = Vec::with_capacity(set.len());
    while let Some(r) = set.join_next().await {
        if let Ok(v) = r {
            out.push(v);
        }
    }
    out
}

pub async fn verify_one(client: &Client, t: Target, with_info: bool) -> Verification {
    let info = if with_info {
        client.info(t.addr).await.ok()
    } else {
        None
    };
    let players = client.players(t.addr).await;
    let (verdict, verified, reason) = judge(
        info.as_ref().map(|r| &r.value),
        players.as_ref().map(|r| &r.value),
        t.reported,
        t.max_players,
    );
    let (reported, max_players) = info
        .as_ref()
        .map(|r| (r.value.players as i32, r.value.max_players as i32))
        .unwrap_or((t.reported, t.max_players));
    Verification {
        id: t.id,
        verdict,
        reported,
        verified,
        max_players,
        ping_ms: info.as_ref().map(|r| r.rtt.as_millis() as u32),
        keywords: info.as_ref().and_then(|r| r.value.keywords.clone()),
        tags: info.as_ref().map(|r| r.value.tags.clone()),
        verified_at: ServerRow::now_unix(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2s::players::Player;
    use crate::a2s::{
        info,
        packet::{classify, Datagram},
    };

    fn players(durations: &[f32], name: &str) -> Players {
        Players {
            count: durations.len() as u8,
            players: durations
                .iter()
                .map(|&d| Player {
                    index: 0,
                    name: name.into(),
                    score: 0,
                    duration_secs: d,
                })
                .collect(),
        }
    }

    fn live_info() -> Info {
        let bytes = include_bytes!("../../tests/fixtures/a2s/kingofgames.info.bin");
        let Datagram::Single(p) = classify(bytes).unwrap() else {
            panic!()
        };
        info::parse(p).unwrap() // 2/100 players
    }

    #[test]
    fn verified_when_counts_agree() {
        let i = live_info();
        let p = players(&[1963.8, 946.1], "");
        let (v, n, _) = judge(Some(&i), Ok(&p), 0, 0);
        assert_eq!((v, n), (Verdict::Verified, Some(2)));
    }

    #[test]
    fn inflated_when_info_exceeds_player_list() {
        let mut i = live_info();
        i.players = 120;
        i.max_players = 120;
        let p = players(&[], "");
        let (v, n, _) = judge(Some(&i), Ok(&p), 0, 0);
        assert_eq!((v, n), (Verdict::Inflated, Some(0)));
        // small drift is fine
        i.players = 4;
        let p = players(&[10.0, 20.0], "");
        assert_eq!(judge(Some(&i), Ok(&p), 0, 0).0, Verdict::Verified);
        // 20 % rule on small servers: 3 of max 10 missing
        i.players = 5;
        i.max_players = 10;
        assert_eq!(judge(Some(&i), Ok(&p), 0, 0).0, Verdict::Inflated);
    }

    #[test]
    fn unverifiable_when_player_query_is_dropped() {
        let mut i = live_info();
        i.players = 100;
        let err = A2sError::Timeout;
        assert_eq!(judge(Some(&i), Err(&err), 0, 0).0, Verdict::Unverifiable);
        i.players = 0;
        assert_eq!(
            judge(Some(&i), Err(&err), 0, 0),
            (
                Verdict::Verified,
                Some(0),
                "INFO reports 0 and PLAYER did not answer".into()
            )
        );
        assert_eq!(judge(None, Err(&err), 7, 60).0, Verdict::Offline);
    }

    #[test]
    fn synthetic_lists() {
        let mut i = live_info();
        i.players = 6;
        assert_eq!(
            judge(Some(&i), Ok(&players(&[5.0; 6], "")), 0, 0).0,
            Verdict::Synthetic,
            "identical durations"
        );
        assert_eq!(
            judge(
                Some(&i),
                Ok(&players(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], "")),
                0,
                0
            )
            .0,
            Verdict::Verified,
            "young but varied: what an honest server looks like just after a restart (D-160)"
        );
        assert_eq!(
            judge(
                Some(&i),
                Ok(&players(
                    &[10.0, 20.0, 30.0, 10.0, 20.0, 30.0, 10.0, 20.0, 30.0],
                    ""
                )),
                0,
                0
            )
            .0,
            Verdict::Synthetic,
            "young and repetitive: nine sessions sharing three durations"
        );
        assert_eq!(
            judge(
                Some(&i),
                Ok(&players(&[100.0, 200.0, 300.0, 400.0, 500.0, 600.0], "Bob")),
                0,
                0
            )
            .0,
            Verdict::Synthetic,
            "named entries"
        );
        assert_eq!(
            judge(
                Some(&i),
                Ok(&players(&[100.0, 200.0, 300.0, 400.0, 500.0, 600.0], "")),
                0,
                0
            )
            .0,
            Verdict::Verified
        );
    }

    #[test]
    fn fallback_uses_cached_numbers_when_info_is_missing() {
        let p = players(&[], "");
        assert_eq!(judge(None, Ok(&p), 90, 90).0, Verdict::Inflated);
        assert_eq!(Verdict::Inflated.as_str(), "inflated");
    }
}
