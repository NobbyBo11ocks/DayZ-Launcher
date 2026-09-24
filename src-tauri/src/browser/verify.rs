//! Population trust: rules R2–R5 and the continuity rule R11 from docs/11-fake-population-detection.md.
//!
//! For each server: one A2S_INFO (fresh reported count, ping, clock) and one
//! A2S_PLAYER (real head-count). The verdict compares the two and inspects the
//! player entries for synthetic patterns.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

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
    /// 202 KB with it — already at the ~200 KB cap docs/05 §4 sets — and 157 KB
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
                // `distinct <= 2` alone fired on a five-player restart reconnect
                // (durations 10.9–11.9 s, 34 more joined within five minutes), so
                // it needs the list not to be young; a young fabricated list is
                // still caught by `repetitive` from six entries up (D-233).
                if (distinct.len() <= 2 && !all_young) || named || (all_young && repetitive) {
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
            // Four ghosts of tolerance on every path, and deliberately so. It looked
            // like a fresh INFO could only differ from PLAYER by the joins and leaves
            // in the 50 ms between the datagrams, and a slack of three was shipped
            // briefly on that reasoning; a 245-address live probe then found ~70
            // honest servers on one hosting provider whose INFO comes from an edge
            // cache and whose PLAYER is a 100-second snapshot. Around every restart
            // the two disagree by a few for ~15 minutes, and three would have flagged
            // them every three hours. The 20 % clause needs three ghosts: one or two
            // connecting players on a ten-slot server tripped it four times in 102
            // verdicts (D-233).
            if diff >= 5 || (max > 0 && diff * 5 >= max && diff >= 3) {
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
/// Every target spawned at once, handed back one at a time as each finishes.
///
/// `verify_many` cannot report anything until its slowest target has answered, so a
/// caller that wants results on screen while the pass runs has to chop the work into
/// chunks — and every chunk boundary idles the pacer for the length of that chunk's
/// tail. The `JoinSet` is the whole pass; the client's permits still bound how many
/// are in flight (D-193).
pub fn verify_stream(client: &Client, targets: Vec<Target>, with_info: bool) -> VerifyStream {
    let mut set = JoinSet::new();
    for t in targets {
        let c = client.clone();
        set.spawn(async move { verify_one(&c, t, with_info).await });
    }
    VerifyStream { set }
}

pub struct VerifyStream {
    set: JoinSet<Verification>,
}

impl VerifyStream {
    /// The next result to finish, or `None` when every target has been reported. A task
    /// that panicked is skipped rather than ending the pass.
    pub async fn next(&mut self) -> Option<Verification> {
        while let Some(joined) = self.set.join_next().await {
            match joined {
                Ok(v) => return Some(v),
                Err(_) => continue,
            }
        }
        None
    }
}

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

/// What the previous check saw on one server, for the continuity rule (D-233).
struct Seen {
    at: Instant,
    durations: Vec<f32>,
    strikes: u8,
}

/// Sessions per server from the previous check. Only servers that answered PLAYER
/// with five or more entries are kept, and the map is cleared past 20 000 entries,
/// which is more servers than have ever verified here.
static LAST_SEEN: LazyLock<Mutex<HashMap<String, Seen>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Two checks closer than this cannot tell a re-drawn list from a stable one.
const CONTINUITY_MIN_GAP_SECS: f32 = 60.0;
/// How far the real advance may sit from the wall-clock gap. A snapshot-serving host
/// hands out a PLAYER list refreshed every 100 s, so its durations advance by the
/// snapshot's age, not the gap — 400.0 s over a 341.9 s gap was measured live (D-233).
const CONTINUITY_SHIFT_WINDOW_SECS: f32 = 120.0;
/// Once the shift is known, real durations line up to well under a second; three
/// seconds covers the pacer and a few-second proxy cache (docs/11, T9).
const CONTINUITY_SLACK_SECS: f32 = 3.0;
/// Consecutive failed comparisons before the verdict changes.
const CONTINUITY_STRIKES: u8 = 2;

/// How many of `prev` reappear in `now` advanced by exactly `shift`, each entry used once.
pub fn carried_over(prev: &[f32], now: &[f32], shift: f32, slack: f32) -> usize {
    let mut pool: Vec<f32> = now.to_vec();
    let mut matched = 0;
    for &d in prev {
        let want = d + shift;
        if let Some(i) = pool.iter().position(|&n| (n - want).abs() <= slack) {
            pool.swap_remove(i);
            matched += 1;
        }
    }
    matched
}

/// The most of `prev` that any single shift within `window` of `dt` carries into
/// `now`. Candidate shifts are the differences between each of the first three old
/// durations and every new one, so a session that left does not hide the shift; at
/// most 3 × 127 candidates, each scored in one pass (D-233).
pub fn best_carried_over(prev: &[f32], now: &[f32], dt: f32, window: f32, slack: f32) -> usize {
    let mut best = 0;
    for &p in prev.iter().take(3) {
        for &n in now {
            let shift = n - p;
            if (shift - dt).abs() > window {
                continue;
            }
            let m = carried_over(prev, now, shift, slack);
            if m > best {
                best = m;
                if best == prev.len() {
                    return best;
                }
            }
        }
    }
    best
}

/// Rule R11: real sessions carry over between checks, advanced by the time elapsed;
/// a fabricated list is re-drawn on every query.
///
/// The one tool found that fakes A2S_PLAYER (docs/11, T2) appends entries whose
/// durations are `random() × 10 000` on each query, after the real ones. R2 passes it
/// (INFO matches the list) and R5 passes it (distinct, not young, unnamed). Between
/// two checks a minute or more apart every real duration reappears advanced by the
/// gap; the fakes never do. Two consecutive checks in which at most one session — or
/// a fifth of them, on a busy server — carried over, and the list is synthetic. A
/// restart is exempt (every session younger than the gap), and so is a list that
/// halved, which is a wipe or a mass leave and not a lie. A later check that carries
/// over clears the strikes, so the verdict heals itself (D-233).
pub fn continuity(id: &str, durations: &[f32]) -> Option<String> {
    if durations.len() < 5 {
        return None;
    }
    let now = Instant::now();
    let Ok(mut map) = LAST_SEEN.lock() else {
        return None;
    };
    if map.len() > 20_000 {
        map.clear();
    }
    let mut strikes = 0u8;
    let mut reason = None;
    if let Some(prev) = map.get(id) {
        let dt = now.duration_since(prev.at).as_secs_f32();
        if dt < CONTINUITY_MIN_GAP_SECS {
            // Too soon to tell: keep the earlier sample and its strikes.
            return None;
        }
        let restarted = durations.iter().all(|&d| d < dt);
        let halved = durations.len() * 2 < prev.durations.len();
        if prev.durations.len() >= 5 && !restarted && !halved {
            let matched = best_carried_over(
                &prev.durations,
                durations,
                dt,
                CONTINUITY_SHIFT_WINDOW_SECS,
                CONTINUITY_SLACK_SECS,
            );
            let floor = (prev.durations.len() / 5).max(1);
            if matched <= floor {
                strikes = prev.strikes.saturating_add(1);
                if strikes >= CONTINUITY_STRIKES {
                    reason = Some(format!(
                        "{matched} of {} sessions carried over between checks {:.0} s apart",
                        prev.durations.len(),
                        dt
                    ));
                }
            }
        }
    }
    map.insert(
        id.to_string(),
        Seen {
            at: now,
            durations: durations.to_vec(),
            strikes,
        },
    );
    reason
}

pub async fn verify_one(client: &Client, t: Target, with_info: bool) -> Verification {
    let info = if with_info {
        client.info(t.addr).await.ok()
    } else {
        None
    };
    let players = client.players(t.addr).await;
    let (mut verdict, verified, mut reason) = judge(
        info.as_ref().map(|r| &r.value),
        players.as_ref().map(|r| &r.value),
        t.reported,
        t.max_players,
    );
    // R11 runs only on a list R2 and R5 have already passed; it is the check those
    // two cannot make from one sample (D-233).
    if verdict == Verdict::Verified {
        if let Ok(p) = players.as_ref() {
            let durations: Vec<f32> = p.value.players.iter().map(|x| x.duration_secs).collect();
            if let Some(why) = continuity(&t.id, &durations) {
                verdict = Verdict::Synthetic;
                reason = why;
            }
        }
    }
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

    #[test]
    fn continuity_real_sessions_carry_over() {
        // Twenty sessions two minutes later: every duration advanced by 120 s.
        let a: Vec<f32> = (0..20).map(|i| 300.0 + i as f32 * 37.0).collect();
        let b: Vec<f32> = a.iter().map(|d| d + 120.0).collect();
        assert_eq!(best_carried_over(&a, &b, 120.0, 120.0, 3.0), 20);
        // One left and one joined: nineteen still carry over, and the one that left
        // was among the three the shift candidates are drawn from.
        let mut c = b.clone();
        c[0] = 12.0;
        assert_eq!(best_carried_over(&a, &c, 120.0, 120.0, 3.0), 19);
        // A snapshot-serving host: durations advanced 400 s over a 342 s gap (measured
        // live on Dead City), and a few-second proxy cache at +6 s (KarmaKrew).
        let d: Vec<f32> = a.iter().map(|x| x + 400.0).collect();
        assert_eq!(best_carried_over(&a, &d, 341.9, 120.0, 3.0), 20);
        let e: Vec<f32> = a.iter().map(|x| x + 126.0).collect();
        assert_eq!(best_carried_over(&a, &e, 120.0, 120.0, 3.0), 20);
    }

    #[test]
    fn continuity_redrawn_list_does_not_carry_over() {
        // docs/11 T2: `random() × 10 000` on every query. One real player at 2 000 s
        // carries over; nineteen fakes do not.
        let mut a: Vec<f32> = (1..20).map(|i| (i * 7919 % 10_000) as f32).collect();
        a.push(2000.0);
        let mut b: Vec<f32> = (1..20).map(|i| (i * 104_729 % 10_000) as f32).collect();
        b.push(2120.0);
        // No shift lines up more than the floor a twenty-entry list allows (four): the
        // real player plus whatever coincides within three seconds.
        assert!(best_carried_over(&a, &b, 120.0, 120.0, 3.0) <= 4);
    }

    #[test]
    fn continuity_needs_a_gap_and_two_strikes() {
        // Same id, two immediate calls: the second is inside the minimum gap, so it
        // neither strikes nor replaces the sample.
        let a: Vec<f32> = (0..10).map(|i| 500.0 + i as f32 * 50.0).collect();
        let b: Vec<f32> = (0..10).map(|i| (i * 977 % 10_000) as f32).collect();
        assert!(continuity("test:1", &a).is_none());
        assert!(continuity("test:1", &b).is_none());
        // Fewer than five entries never take part.
        assert!(continuity("test:2", &[1.0, 2.0, 3.0]).is_none());
    }

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
