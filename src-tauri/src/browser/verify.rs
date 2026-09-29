//! Population trust: rules R2–R5, the continuity rule R11 and R12–R14 from docs/11-fake-population-detection.md.
//!
//! For each server: one A2S_PLAYER (the real head-count), preceded by an A2S_INFO
//! (fresh reported count, ping, clock) when the check is on demand or the stored count
//! is over `INFO_FRESH_SECS` old. The verdict compares the two and inspects the
//! player entries for synthetic patterns.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

use serde::Serialize;
use tokio::task::JoinSet;

use crate::a2s::{A2sError, Client, Info, Players};

use super::ServerRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// R2: PLAYER agrees with INFO (difference ≤ 4, D-233).
    Verified,
    /// R3: INFO exceeds PLAYER by ≥ 5, or by ≥ 20 % of max with at least three missing
    /// (D-233); R12: the list carries entries no clock produced (D-238).
    Inflated,
    /// R4: INFO answers with players > 0 but PLAYER never answers.
    Unverifiable,
    /// R5: the PLAYER list looks fabricated (at most two distinct durations on a list
    /// that is not all young, all young and repetitive, or any name); R11: sessions old
    /// enough to have been on at the previous check were not on it; R13: more entries
    /// than slots; R14: a burst of sessions under a second beside an old one (D-314).
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
    /// Unix seconds at which `reported` was read. The automatic pass skips INFO
    /// because Steam's is "a minute old at most" (D-047) — true of the default
    /// refresh, not of the full one D-141 made every Refresh, which takes minutes;
    /// an older count is re-read (D-237). 0 when the age is unknown.
    pub reported_at: i64,
    /// The cached verdict was "synthetic", which R11 must not forget at a restart.
    pub was_synthetic: bool,
    /// What the row says about the server, so a check sends only the facts that changed.
    pub known: InfoFacts,
}

impl Target {
    pub fn from_row(r: &ServerRow) -> Option<Self> {
        let addr: SocketAddr = format!("{}:{}", r.ip, r.query_port).parse().ok()?;
        Some(Self {
            id: r.id.clone(),
            addr,
            reported: r.players,
            max_players: r.max_players,
            // A count DZSA wrote is judged against a fresh INFO, never against itself
            // (D-237), in the minute after an import too: a check of the rows on screen
            // then took it as a fresh listing's and, with INFO lost, judged a head-count
            // against it (row 28).
            reported_at: if super::dzsa::stamped(r.last_seen) {
                0
            } else {
                r.last_seen
            },
            was_synthetic: r.verdict.as_deref() == Some("synthetic"),
            known: InfoFacts::of_row(r),
        })
    }
}

/// A count read from INFO longer ago than this is read again before it is judged.
/// Steam's own list is at most a minute old when the default refresh completes; a
/// full refresh reaches its verification pass minutes after the populated partition
/// was listed, and a restart or ordinary churn in between read as "INFO 60 vs PLAYER
/// 4" — Inflated, and hidden until the next Refresh (D-237).
pub(crate) const INFO_FRESH_SECS: i64 = 60;

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
    /// PLAYER's round trip, for a row whose ping was never measured — the automatic
    /// pass sends no INFO, so a DZSA row (ping 0) stayed at "—" through every pass:
    /// 1 498 of 4 488 populated rows on 2026-09-25, outside every ping preset (D-247).
    pub player_rtt_ms: Option<u32>,
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
    /// What this check's INFO said about the server itself, when that differs from the
    /// row it started from (row 23). Only a listing or a probe used to write these, so a
    /// favourite Steam's listings never return, or one imported from the official
    /// launcher's file while it was offline, kept the name, map and version it was
    /// stored with; a DayZ update then marked it "≠ mine" although it had updated too.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facts: Option<InfoFacts>,
}

/// The server's own description of itself in A2S_INFO, as far as a row keeps it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InfoFacts {
    pub name: String,
    pub map: String,
    /// `1.29.163709`, and Steam's integer form beside it (0 when unparsable).
    pub version: String,
    pub server_version: i32,
    /// `None` when the reply carried no game port.
    pub game_port: Option<u16>,
    pub password: bool,
    pub bots: i32,
}

impl InfoFacts {
    pub fn of(info: &Info) -> Self {
        Self {
            name: info.name.clone(),
            map: info.map.clone(),
            version: info.version.clone(),
            server_version: ServerRow::version_int(&info.version),
            game_port: info.game_port,
            password: info.password,
            bots: i32::from(info.bots),
        }
    }

    pub fn of_row(r: &ServerRow) -> Self {
        Self {
            name: r.name.clone(),
            map: r.map.clone(),
            version: r.version.clone(),
            server_version: r.server_version,
            game_port: Some(r.game_port),
            password: r.password,
            bots: r.bots,
        }
    }

    /// Whether these facts would change `known`. An empty name or map, an unreadable
    /// version and a missing game port say nothing, and are never written either.
    pub fn changes(&self, known: &InfoFacts) -> bool {
        (!self.name.is_empty() && self.name != known.name)
            || (!self.map.is_empty() && self.map != known.map)
            || (self.server_version > 0 && self.server_version != known.server_version)
            || self.game_port.is_some_and(|p| Some(p) != known.game_port)
            || self.password != known.password
            || self.bots != known.bots
    }
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
            // R12: sessions no clock produced. The one published tool that fakes
            // A2S_PLAYER (docs/11 T2) has, since its second commit, appended its fake
            // entries with the duration bytes `00 00 00 01` — 2.35e-38 s — while the
            // random durations it still draws go unused. A real connection is never
            // that young twice over, and those entries are not sessions: they are left
            // out of the count, and a server that sends them is inflating by exactly
            // that many. R5 caught the tool only with no real player on, R11 only when
            // four fifths of the list were fake; with four real players it verified at
            // fourteen (D-238). An exact 0.0 counts too, one byte from the tool's
            // constant: none of 2 409 honest lists read live held a single one (D-314).
            let fabricated = p
                .players
                .iter()
                .filter(|x| (0.0..0.001).contains(&x.duration_secs))
                .count();
            if fabricated >= 2 {
                let v = (p.players.len() - fabricated) as i32;
                let info_txt = if reported < 0 {
                    "no INFO".to_string()
                } else {
                    format!("INFO {reported}")
                };
                return (
                    Verdict::Inflated,
                    Some(v),
                    format!(
                        "{info_txt}, {v} real sessions: {fabricated} listed entries are zero-length, which no connected player is"
                    ),
                );
            }
            let v = p.players.len() as i32;
            if v >= 5 {
                // Players who join together land within half a second of each other
                // (three such groups in one live 114-player list), so in whole seconds a
                // trio and a duo on a five-player server were two durations; in tenths
                // they are five. Only a list that repeats itself stays at two (D-268).
                let distinct_tenths = p
                    .players
                    .iter()
                    .map(|x| (x.duration_secs * 10.0).round() as i64)
                    .collect::<HashSet<i64>>()
                    .len();
                let all_young = p.players.iter().all(|x| x.duration_secs < 60.0);
                let named = p.players.iter().any(|x| !x.name.is_empty());
                // "Everyone joined in the last minute" is what an honest server looks
                // like just after its scheduled restart, so on its own it no longer
                // flags (D-288): it was hiding real servers every few hours. What gives
                // a fabricated list away is repetition — many players sharing very few
                // distinct durations — so young sessions only count when they also
                // repeat.
                // In tenths as well: in whole seconds a reconnect wave read as repetition.
                // Six players back at 19.2, 19.2, 19.1, 18.8, 18.5 and 18.1 s were two
                // durations, and two honest servers (31–40 players) went hidden seconds
                // after their restarts; in tenths they are five, and a list that copies
                // its values still repeats (row 22).
                let repetitive = distinct_tenths * 3 <= v as usize;
                // `distinct <= 2` alone fired on a five-player restart reconnect
                // (durations 10.9–11.9 s, 34 more joined within five minutes), so
                // it needs the list not to be young; a young fabricated list is
                // still caught by `repetitive` from six entries up (D-233).
                if (distinct_tenths <= 2 && !all_young) || named || (all_young && repetitive) {
                    return (
                        Verdict::Synthetic,
                        Some(v),
                        format!(
                            "{v} entries, {distinct_tenths} distinct durations, all_young={all_young}, named={named}"
                        ),
                    );
                }
            }
            // R13: more sessions than slots (D-314). A full server lists exactly its
            // slots and never its login queue — 110 of 110 with 21 waiting and 121 of 121
            // with 15, read live — so an entry past the last slot is held by no player.
            // Only against a slot count from this check or a fresh listing: a stale one
            // may predate a change of slots.
            if reported >= 0 && max > 0 && v > max {
                return (
                    Verdict::Synthetic,
                    Some(v),
                    format!("{v} entries on a server of {max} slots"),
                );
            }
            // R14: three or more sessions under a second beside one of ten minutes or
            // more (D-314): what T2 would send writing any value under a second where it
            // writes 2.35e-38. Squads land within half a second of each other (D-268),
            // so an honest list shows this only to a check that arrives within a second
            // of a squad, while older players are on; none of 2 252 live lists did.
            let fresh = p.players.iter().filter(|x| x.duration_secs < 1.0).count();
            let oldest = p
                .players
                .iter()
                .map(|x| x.duration_secs)
                .fold(0.0_f32, f32::max);
            if fresh >= 3 && oldest >= 600.0 {
                return (
                    Verdict::Synthetic,
                    Some(v),
                    format!("{fresh} sessions under a second beside one of {oldest:.0} s"),
                );
            }
            // No count to compare: the cached one was stale and its re-read failed
            // (`verify_one`, D-245). R5, R11–R14 have had their say above; the
            // head-count stands on its own.
            if reported < 0 {
                return (
                    Verdict::Verified,
                    Some(v),
                    format!("PLAYER {v}; INFO did not answer"),
                );
            }
            let diff = reported - v;
            // Four ghosts of tolerance on every path, and deliberately so. It looked
            // like a fresh INFO could only differ from PLAYER by the joins and leaves
            // in the 50 ms between the datagrams, and a tolerance of two was shipped
            // briefly on that reasoning; a 245-address live probe then found ~70
            // honest servers on one hosting provider whose INFO comes from an edge
            // cache and whose PLAYER is a 100-second snapshot. Around every restart
            // the two disagree by a few for ~15 minutes, and that tolerance would have
            // flagged them every three hours. The 20 % clause needs three ghosts: one or two
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
pub const CONTINUITY_MIN_GAP_SECS: f32 = 60.0;
/// How far the real advance may sit from the wall-clock gap. A snapshot-serving host
/// hands out a PLAYER list refreshed every 100 s, so its durations advance by the
/// snapshot's age, not the gap — 400.0 s over a 341.9 s gap was measured live (D-233).
const CONTINUITY_SHIFT_WINDOW_SECS: f32 = 120.0;
/// Once the shift is known, real durations line up to well under a second; three
/// seconds covers the pacer and a few-second proxy cache (docs/11, T9).
const CONTINUITY_SLACK_SECS: f32 = 3.0;
/// Consecutive failed comparisons before the verdict changes.
const CONTINUITY_STRIKES: u8 = 2;
/// A comparison fails when at least this many sessions old enough to have been on at
/// the previous check were not on it, and they are at least a tenth of those sessions.
const CONTINUITY_MISSING: usize = 2;
/// The one-in-five count judges checks up to this far apart. Churn alone takes a
/// steady, honest server under it over an hour or two — sessions average about 58
/// minutes on docs/11's figure (91 % still there after 5½ minutes) — so two refreshes
/// 90 minutes apart could strike it twice. At 30 minutes ~60 % remain (D-237). The
/// invariant needs no cap: churn does not enter it (D-314).
const CONTINUITY_FLOOR_MAX_GAP_SECS: f32 = 1800.0;
/// The reason given while a standing verdict waits for a check it can compare. Any rule
/// may have given it — R5, R13 and R14 as well as R11 — and it said sessions had not
/// carried over whichever did (row 28).
pub const CONTINUITY_STANDING: &str =
    "judged fake at an earlier check; none since was close enough to compare";

/// Lines `now` up with `prev` advanced by `shift`: of the sessions in `now` at least
/// `min_age` long, how many there were and how many found one in `prev` within
/// `slack`. Both lists run longest first and are matched in order, each entry once;
/// `offsets` collects `now − prev` of every match.
fn align(
    prev: &[f64],
    now: &[f64],
    shift: f64,
    slack: f64,
    min_age: f64,
    mut offsets: Option<&mut Vec<f64>>,
) -> (usize, usize) {
    let (mut old, mut matched, mut j) = (0, 0, 0);
    for &d in now {
        if d < min_age {
            break;
        }
        old += 1;
        let want = d - shift;
        while j < prev.len() && prev[j] > want + slack {
            j += 1;
        }
        if j < prev.len() && prev[j] >= want - slack {
            if let Some(o) = offsets.as_deref_mut() {
                o.push(d - prev[j]);
            }
            matched += 1;
            j += 1;
        }
    }
    (old, matched)
}

/// Durations as the comparisons read them: finite, longest first.
fn longest_first(v: &[f32]) -> Vec<f64> {
    let mut v: Vec<f64> = v
        .iter()
        .map(|&d| f64::from(d))
        .filter(|d| d.is_finite())
        .collect();
    v.sort_unstable_by(|a, b| b.total_cmp(a));
    v
}

/// The whole-second shift within `window` of `dt` that carries the most sessions of
/// `prev` into `now`, nearest the gap among equals, and how many it carries. The scan
/// stops once a shift carries more than `enough`.
fn most_carried(
    prev: &[f64],
    now: &[f64],
    dt: f64,
    window: f64,
    slack: f64,
    enough: usize,
) -> (usize, f64) {
    let w = window.ceil() as i32;
    let mut best = (0, dt);
    for i in 0..=2 * w {
        let off = f64::from((i + 1) / 2);
        let shift = if i % 2 == 1 { dt + off } else { dt - off };
        let (_, matched) = align(prev, now, shift, slack, f64::NEG_INFINITY, None);
        if matched > best.0 {
            best = (matched, shift);
            if matched > enough {
                break;
            }
        }
    }
    best
}

/// R11's count, kept beside the invariant (D-314): the most sessions of `prev` that one
/// shift within `window` of `dt` carries into `now`, counting stopped past `enough`.
/// Every whole second of the window is tried; since D-268 no shift is left out because
/// the three oldest sessions had left, which struck an honest server.
fn carried(prev: &[f32], now: &[f32], dt: f32, window: f32, slack: f32, enough: usize) -> usize {
    let (p, n) = (longest_first(prev), longest_first(now));
    most_carried(
        &p,
        &n,
        f64::from(dt),
        f64::from(window),
        f64::from(slack),
        enough,
    )
    .0
}

/// R11's comparison (D-314): the sessions in `now` old enough to have been on at the
/// check `dt` seconds earlier — longer than the shift plus `slack` — that `prev` does
/// not hold. `None` when some shift within `window` of `dt` accounts for all of them
/// but one, or all but under a tenth; otherwise the fewest any shift leaves unexplained
/// and how many sessions were that old at it.
///
/// Churn does not enter it: a player who left is not looked for, and one who joined is
/// younger than the gap. So unlike the count beside it — whether more than one session
/// in five carried over — it needs no cap on the gap, and real sessions padded with
/// re-drawn ones fail it for the padding alone, where the count passes padding up to
/// four fifths of the list as long as the real sessions stay. Every shift is tried, not only
/// the one most sessions agree on: players who happened to join as far apart as others
/// who left could outvote two who stayed, and strike an honest list (row 22).
fn unexplained(
    prev: &[f32],
    now: &[f32],
    dt: f32,
    window: f32,
    slack: f32,
) -> Option<(usize, usize)> {
    let (p, n) = (longest_first(prev), longest_first(now));
    let (dt, window, slack) = (f64::from(dt), f64::from(window), f64::from(slack));
    let mut fewest: Option<(usize, usize)> = None;
    let mut explains = |shift: f64| {
        let (old, matched) = align(&p, &n, shift, slack, shift + slack, None);
        let missing = old - matched;
        if missing < CONTINUITY_MISSING || missing * 10 < old {
            return true;
        }
        if fewest.is_none_or(|(m, _)| missing < m) {
            fewest = Some((missing, old));
        }
        false
    };
    // Every half second of the window, nearest the gap first: an honest list lines up
    // at its real shift, seconds from the gap on a host that answers directly.
    let steps = (window * 2.0).ceil() as i32;
    for i in 0..=2 * steps {
        let off = f64::from((i + 1) / 2) * 0.5;
        if explains(if i % 2 == 1 { dt + off } else { dt - off }) {
            return None;
        }
    }
    // And the shift most sessions agree on, refined to the median offset of its
    // matches, as row 22 measured it on 2 797 live pairs; a half-second grid can
    // straddle it (S-122).
    let (agreed, shift) = most_carried(&p, &n, dt, window, slack, usize::MAX);
    if agreed > 0 {
        let mut offsets = Vec::with_capacity(agreed);
        align(&p, &n, shift, slack, f64::NEG_INFINITY, Some(&mut offsets));
        offsets.sort_unstable_by(f64::total_cmp);
        if explains(offsets[offsets.len() / 2]) {
            return None;
        }
    }
    fewest
}

/// Rule R11: real sessions carry over between checks, advanced by the time elapsed;
/// a fabricated list is re-drawn on every query.
///
/// The one tool found that fakes A2S_PLAYER (docs/11, T2) appended, in its first
/// commit, entries whose durations are `random() × 10 000` on each query, after the
/// real ones; the version it shipped writes zero-length entries instead, which R12
/// catches (D-238). A faker that re-draws its list on every query passes R2 (INFO
/// matches the list) and R5 (distinct, not young, unnamed). Between two checks a minute
/// or more apart, every session in the second list old enough to have been on at the
/// first was on it, advanced by the gap; re-drawn entries were not (`unexplained`,
/// D-314). Up to 30 minutes apart, more than one session in five must also carry over
/// (`carried`, D-233): entries re-drawn younger than the gap escape the first test,
/// and padding up to four fifths of a list passes the second while the real sessions stay.
/// Two consecutive checks that fail either, and the list is synthetic. A restart is
/// exempt (every session younger than the gap), and so is a list that halved, which
/// is a wipe or a mass leave and not a lie. A later check that passes both clears the
/// strikes, so the verdict heals itself (D-233).
///
/// A verdict stands until a comparison overturns it (D-237). The first sample after a
/// restart and a check inside the minimum gap used to answer "no opinion", which the
/// caller published as Verified: a farm got its fabricated count back at every launch
/// and every time its row was opened. A cached "synthetic" verdict now starts at the
/// full strike count, and comparisons that are exempt (restart, halved list) or
/// impossible (too close) keep the strikes they found — resetting them let a list that
/// alternated sizes, halving every other check, never be flagged.
pub fn continuity(id: &str, durations: &[f32], was_synthetic: bool) -> Option<String> {
    continuity_at(id, durations, was_synthetic, Instant::now())
}

fn continuity_at(id: &str, durations: &[f32], was_synthetic: bool, now: Instant) -> Option<String> {
    if durations.len() < 5 {
        return None;
    }
    let Ok(mut map) = LAST_SEEN.lock() else {
        return None;
    };
    if map.len() > 20_000 {
        map.clear();
    }
    let mut strikes = if was_synthetic { CONTINUITY_STRIKES } else { 0 };
    let mut reason = None;
    if let Some(prev) = map.get(id) {
        strikes = prev.strikes;
        let dt = now.saturating_duration_since(prev.at).as_secs_f32();
        if dt < CONTINUITY_MIN_GAP_SECS {
            // Too soon to tell: keep the earlier sample, its strikes and its verdict.
            return (prev.strikes >= CONTINUITY_STRIKES).then(|| CONTINUITY_STANDING.to_string());
        }
        let restarted = durations.iter().all(|&d| d < dt);
        let halved = durations.len() * 2 < prev.durations.len();
        if prev.durations.len() >= 5 && !restarted && !halved {
            let (window, slack) = (CONTINUITY_SHIFT_WINDOW_SECS, CONTINUITY_SLACK_SECS);
            let strike = match unexplained(&prev.durations, durations, dt, window, slack) {
                Some((missing, old)) => Some(format!(
                    "{missing} of {old} sessions older than the gap were missing from the list {dt:.0} s earlier"
                )),
                None if dt <= CONTINUITY_FLOOR_MAX_GAP_SECS => {
                    let floor = (prev.durations.len() / 5).max(1);
                    let matched = carried(&prev.durations, durations, dt, window, slack, floor);
                    (matched <= floor).then(|| {
                        format!(
                            "{matched} of {} sessions carried over between checks {dt:.0} s apart",
                            prev.durations.len()
                        )
                    })
                }
                None => None,
            };
            match strike {
                Some(why) => {
                    strikes = prev.strikes.saturating_add(1);
                    reason = Some(why);
                }
                None => strikes = 0,
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
    (strikes >= CONTINUITY_STRIKES)
        .then(|| reason.unwrap_or_else(|| CONTINUITY_STANDING.to_string()))
}

/// `judge` plus the continuity rule, for every caller that publishes a verdict. The
/// details pane called `judge` alone, so opening a row wrote "verified" over a
/// standing R11 verdict, in the UI and in the cache (D-237).
pub fn judge_with_continuity(
    id: &str,
    info: Option<&Info>,
    players: Result<&Players, &A2sError>,
    fallback_reported: i32,
    fallback_max: i32,
    was_synthetic: bool,
) -> (Verdict, Option<i32>, String) {
    let (mut verdict, verified, mut reason) = judge(info, players, fallback_reported, fallback_max);
    // R11 runs only on a list R2 and R5 have already passed; it is the check those
    // two cannot make from one sample (D-233).
    if verdict == Verdict::Verified {
        if let Ok(p) = players {
            let durations: Vec<f32> = p.players.iter().map(|x| x.duration_secs).collect();
            if let Some(why) = continuity(id, &durations, was_synthetic) {
                verdict = Verdict::Synthetic;
                reason = why;
            }
        }
    }
    (verdict, verified, reason)
}

pub async fn verify_one(client: &Client, t: Target, with_info: bool) -> Verification {
    let stale = ServerRow::now_unix().saturating_sub(t.reported_at) > INFO_FRESH_SECS;
    let info = if with_info || stale {
        client.info(t.addr).await.ok()
    } else {
        None
    };
    // Another game answering on this address: the port was handed on, and its numbers
    // are not this server's, least of all a favourite's, which is never pruned (row 18).
    if info.as_ref().is_some_and(|r| !r.value.is_dayz()) {
        return Verification {
            id: t.id,
            verdict: Verdict::Offline,
            reported: t.reported,
            verified: None,
            max_players: t.max_players,
            ping_ms: None,
            player_rtt_ms: None,
            keywords: None,
            tags: None,
            verified_at: ServerRow::now_unix(),
            reason: "another game answers on this address".into(),
            facts: None,
        };
    }
    let players = client.players(t.addr).await;
    // A stale count that could not be refreshed is no count: judged against it, a
    // fresh PLAYER read as "INFO 60 vs PLAYER 4" after ordinary churn and hid the
    // server as inflated until the next Refresh — the D-237 (1) symptom on the lossy
    // path. `-1` tells `judge` to skip that comparison (D-245).
    let fallback_reported = if stale && info.is_none() {
        -1
    } else {
        t.reported
    };
    let (verdict, verified, reason) = judge_with_continuity(
        &t.id,
        info.as_ref().map(|r| &r.value),
        players.as_ref().map(|r| &r.value),
        fallback_reported,
        t.max_players,
        t.was_synthetic,
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
        player_rtt_ms: players.as_ref().ok().map(|r| r.rtt.as_millis() as u32),
        keywords: info.as_ref().and_then(|r| r.value.keywords.clone()),
        tags: info.as_ref().map(|r| r.value.tags.clone()),
        verified_at: ServerRow::now_unix(),
        reason,
        facts: info
            .as_ref()
            .map(|r| InfoFacts::of(&r.value))
            .filter(|f| f.changes(&t.known)),
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
    fn a_stale_count_that_did_not_refresh_is_not_compared() {
        // Four sessions on a server whose cached INFO said 60 minutes ago and whose
        // re-read failed: judged against the stale 60 it read as inflated (D-237's
        // symptom on the lossy path); the sentinel stands the head-count on its own.
        let p = players(&[100.0, 200.0, 300.0, 400.0], "");
        let (old, _, _) = judge(None, Ok(&p), 60, 60);
        assert_eq!(
            old,
            Verdict::Inflated,
            "the stale number would have hidden it"
        );
        let (v, n, why) = judge(None, Ok(&p), -1, 60);
        assert_eq!((v, n), (Verdict::Verified, Some(4)));
        assert!(why.contains("INFO did not answer"), "{why}");
        // With no PLAYER either the caller still gets Offline and retries with INFO.
        let err = crate::a2s::A2sError::Timeout;
        assert_eq!(judge(None, Err(&err), -1, 60).0, Verdict::Offline);
    }

    #[test]
    fn continuity_explains_real_sessions() {
        // Twenty sessions two minutes later: every duration advanced by 120 s.
        let a: Vec<f32> = (0..20).map(|i| 300.0 + i as f32 * 37.0).collect();
        let b: Vec<f32> = a.iter().map(|d| d + 120.0).collect();
        assert_eq!(unexplained(&a, &b, 120.0, 120.0, 3.0), None);
        // One left and one joined: the one who left is not looked for, and the one who
        // joined is younger than the gap.
        let mut c = b.clone();
        c[0] = 12.0;
        assert_eq!(unexplained(&a, &c, 120.0, 120.0, 3.0), None);
        // A snapshot-serving host: durations advanced 400 s over a 342 s gap (measured
        // live on Dead City), a few-second proxy cache at +6 s (KarmaKrew), and two
        // checks served the same snapshot, nothing advanced at all.
        let d: Vec<f32> = a.iter().map(|x| x + 400.0).collect();
        assert_eq!(unexplained(&a, &d, 341.9, 120.0, 3.0), None);
        let e: Vec<f32> = a.iter().map(|x| x + 126.0).collect();
        assert_eq!(unexplained(&a, &e, 120.0, 120.0, 3.0), None);
        assert_eq!(unexplained(&a, &a, 90.0, 120.0, 3.0), None);
        // Pacer jitter of two and a half seconds either way.
        let f: Vec<f32> = (0..20)
            .map(|i| a[i] + 120.0 + if i % 2 == 0 { 2.5 } else { -2.5 })
            .collect();
        assert_eq!(unexplained(&a, &f, 120.0, 120.0, 3.0), None);
    }

    #[test]
    fn continuity_survives_the_oldest_sessions_leaving() {
        // PLAYER lists run oldest first. The three longest sessions left between two
        // checks twenty minutes apart and three players joined; the other seventeen
        // carried over. Shifts drawn from the first three entries found none (D-268).
        let a: Vec<f32> = (0..20).map(|i| 20_000.0 - i as f32 * 613.0).collect();
        let mut b: Vec<f32> = a[3..].iter().map(|d| d + 1200.0).collect();
        b.extend([40.0, 25.0, 3.0]);
        assert_eq!(unexplained(&a, &b, 1200.0, 120.0, 3.0), None);
        // Five players, the three oldest gone: the two that stayed still line up.
        let a = [9000.0, 8000.0, 7000.0, 900.0, 300.0];
        let b = [1500.0, 900.0, 60.0, 30.0, 10.0];
        assert_eq!(unexplained(&a, &b, 600.0, 120.0, 3.0), None);
    }

    /// The count beside the invariant, on the cases it was built against (D-268).
    #[test]
    fn continuity_counts_what_carried_over() {
        let all = usize::MAX;
        let a: Vec<f32> = (0..20).map(|i| 300.0 + i as f32 * 37.0).collect();
        let b: Vec<f32> = a.iter().map(|d| d + 120.0).collect();
        assert_eq!(carried(&a, &b, 120.0, 120.0, 3.0, all), 20);
        let mut c = b.clone();
        c[0] = 12.0;
        assert_eq!(carried(&a, &c, 120.0, 120.0, 3.0, all), 19);
        let d: Vec<f32> = a.iter().map(|x| x + 400.0).collect();
        assert_eq!(carried(&a, &d, 341.9, 120.0, 3.0, all), 20);
        let e: Vec<f32> = a.iter().map(|x| x + 126.0).collect();
        assert_eq!(carried(&a, &e, 120.0, 120.0, 3.0, all), 20);
        // The three oldest left: seventeen, and two of five.
        let a: Vec<f32> = (0..20).map(|i| 20_000.0 - i as f32 * 613.0).collect();
        let mut b: Vec<f32> = a[3..].iter().map(|d| d + 1200.0).collect();
        b.extend([40.0, 25.0, 3.0]);
        assert_eq!(carried(&a, &b, 1200.0, 120.0, 3.0, all), 17);
        let a = [9000.0, 8000.0, 7000.0, 900.0, 300.0];
        let b = [1500.0, 900.0, 60.0, 30.0, 10.0];
        assert_eq!(carried(&a, &b, 600.0, 120.0, 3.0, all), 2);
        // Re-drawn beside one real player: nothing past chance coincidences.
        let mut a = redrawn(11, 19);
        a.push(2000.0);
        let mut b = redrawn(12, 19);
        b.push(2120.0);
        assert!(carried(&a, &b, 120.0, 120.0, 3.0, all) <= 4);
        // Counting stops once enough carried over.
        let a: Vec<f32> = (0..20).map(|i| 300.0 + i as f32 * 37.0).collect();
        let b: Vec<f32> = a.iter().map(|d| d + 120.0).collect();
        assert!(carried(&a, &b, 120.0, 120.0, 3.0, 4) > 4);
    }

    /// Entries re-drawn younger than the gap are never old enough for the invariant to
    /// look for; that most of the list failed to carry over still gives them away within
    /// 30 minutes, as it did before D-314 (row 22's model: 298 of 381 such servers).
    #[test]
    fn continuity_still_finds_a_list_that_does_not_carry_over() {
        let t0 = Instant::now();
        let at = |secs: u64| t0 + std::time::Duration::from_secs(secs);
        let real = [7200.0, 5400.0, 3100.0, 900.0];
        let list = |seed: u64, since: f32| -> Vec<f32> {
            let mut v: Vec<f32> = real.iter().map(|d| d + since).collect();
            v.extend(redrawn(seed, 16).iter().map(|d| d % 60.0));
            v
        };
        let id = "test:young";
        assert!(continuity_at(id, &list(21, 0.0), false, at(0)).is_none());
        assert!(continuity_at(id, &list(22, 300.0), false, at(300)).is_none());
        let why = continuity_at(id, &list(23, 600.0), false, at(600)).expect("two strikes");
        assert!(why.contains("carried over between checks"), "{why}");
    }

    /// Three who joined 70–90 s after a check line up with three who had joined 10–30 s
    /// before it and left, at a shift 100 s short of the gap, and outvote the two who
    /// stayed. At that shift the stayers look invented; at the real one nothing is
    /// missing, and the real one is tried as well (row 22).
    #[test]
    fn continuity_is_not_outvoted_by_a_chance_alignment() {
        let a = [5000.0, 4000.0, 30.0, 20.0, 10.0];
        let b = [6800.0, 5800.0, 1730.0, 1720.0, 1710.0];
        assert_eq!(unexplained(&a, &b, 1800.0, 120.0, 3.0), None);
    }

    #[test]
    fn continuity_finds_a_redrawn_list() {
        // A list re-drawn as `random() × 10 000` on every query, as T2's first commit
        // did (docs/11). One real player at 2 000 s carries over; nineteen fakes do not.
        let mut a: Vec<f32> = (1..20).map(|i| (i * 7919 % 10_000) as f32).collect();
        a.push(2000.0);
        let mut b: Vec<f32> = (1..20).map(|i| (i * 104_729 % 10_000) as f32).collect();
        b.push(2120.0);
        let (missing, old) = unexplained(&a, &b, 120.0, 120.0, 3.0).expect("a strike");
        assert!(missing * 10 >= old * 8, "{missing} of {old}");
    }

    /// The padding alone condemns a list: twenty real sessions carry over and five
    /// re-drawn entries beside them do not (D-314). The count R11 used before passed
    /// it, twenty carried over being far above one in five.
    #[test]
    fn continuity_finds_padding_beside_real_sessions() {
        let real: Vec<f32> = (0..20).map(|i| 400.0 + i as f32 * 181.0).collect();
        let mut a = real.clone();
        a.extend([7310.0, 5120.0, 2890.0, 9480.0, 660.0]);
        let mut b: Vec<f32> = real.iter().map(|d| d + 300.0).collect();
        b.extend([1830.0, 8470.0, 4410.0, 6150.0, 3260.0]);
        assert_eq!(unexplained(&a, &b, 300.0, 120.0, 3.0), Some((5, 25)));
    }

    #[test]
    fn continuity_needs_a_gap_and_two_strikes() {
        // Same id, two immediate calls: the second is inside the minimum gap, so it
        // neither strikes nor replaces the sample.
        let a: Vec<f32> = (0..10).map(|i| 500.0 + i as f32 * 50.0).collect();
        let b: Vec<f32> = (0..10).map(|i| (i * 977 % 10_000) as f32).collect();
        assert!(continuity("test:1", &a, false).is_none());
        assert!(continuity("test:1", &b, false).is_none());
        // Fewer than five entries never take part.
        assert!(continuity("test:2", &[1.0, 2.0, 3.0], false).is_none());
    }

    /// A re-drawn list is struck twice in a row; the verdict then survives a check too
    /// soon to compare, a check too far apart and an exempt one, and only a list that
    /// carries over clears it (D-237).
    #[test]
    fn continuity_verdict_stands_until_a_comparison_clears_it() {
        let t0 = Instant::now();
        let at = |secs: u64| t0 + std::time::Duration::from_secs(secs);
        let fake = redrawn;
        let id = "test:r11";
        assert!(continuity_at(id, &fake(1, 20), false, at(0)).is_none());
        assert!(
            continuity_at(id, &fake(2, 20), false, at(120)).is_none(),
            "one strike"
        );
        let why = continuity_at(id, &fake(3, 20), false, at(240)).expect("two strikes");
        // The details pane keys its sentence on this wording (src/lib/verdict.ts).
        assert!(
            why.contains("sessions older than the gap were missing from the list"),
            "{why}"
        );
        // Inside the minimum gap: nothing to compare, the verdict stands.
        assert!(continuity_at(id, &fake(4, 20), false, at(250)).is_some());
        // An hour on: compared all the same since D-314, and struck again.
        assert!(continuity_at(id, &fake(5, 20), false, at(4_000)).is_some());
        // A list that halved is exempt from comparison, and exempt is not cleared.
        let half = fake(6, 9);
        assert!(continuity_at(id, &half, false, at(4_200)).is_some());
        // Real sessions carrying over, advanced by the gap, clear it.
        let carried: Vec<f32> = half.iter().map(|d| d + 150.0).collect();
        assert!(continuity_at(id, &carried, false, at(4_350)).is_none());
    }

    /// Churn on an honest server over a long gap is not evidence, so the gap needs no
    /// cap (D-314): ninety minutes on, seventeen of twenty have left and seventeen
    /// joined since, and the three who stayed line up. A re-drawn list is found across
    /// the same gaps, which the 30-minute cap of D-237 never compared.
    #[test]
    fn continuity_compares_across_long_gaps() {
        let t0 = Instant::now();
        let at = |secs: u64| t0 + std::time::Duration::from_secs(secs);
        let a: Vec<f32> = (0..20).map(|i| 300.0 + i as f32 * 97.0).collect();
        let mut b: Vec<f32> = a[17..].iter().map(|d| d + 5_400.0).collect();
        b.extend((0..17).map(|i| 50.0 + i as f32 * 211.0));
        let mut c: Vec<f32> = b[..3].iter().map(|d| d + 5_400.0).collect();
        c.extend((0..17).map(|i| 70.0 + i as f32 * 173.0));
        assert!(continuity_at("test:gap", &a, false, at(0)).is_none());
        assert!(continuity_at("test:gap", &b, false, at(5_400)).is_none());
        assert!(continuity_at("test:gap", &c, false, at(10_800)).is_none());
        assert!(continuity_at("test:gap2", &redrawn(1, 20), false, at(0)).is_none());
        assert!(continuity_at("test:gap2", &redrawn(2, 20), false, at(5_400)).is_none());
        assert!(continuity_at("test:gap2", &redrawn(3, 20), false, at(10_800)).is_some());
    }

    /// A verdict cached before a restart stands until a comparison can overturn it.
    #[test]
    fn continuity_keeps_a_cached_synthetic_verdict() {
        let a: Vec<f32> = (0..10).map(|i| 500.0 + i as f32 * 50.0).collect();
        assert!(continuity("test:cached", &a, true).is_some());
        assert!(continuity("test:fresh", &a, false).is_none());
    }

    /// A fresh `random() × 10 000` list on every query, as T2's first commit did
    /// (docs/11; xorshift64).
    fn redrawn(seed: u64, n: usize) -> Vec<f32> {
        let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x % 10_000) as f32
            })
            .collect()
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

    /// docs/11 T2 as it ships (anatolykopyl/server-query-fake-player-count, every
    /// commit since c539ddc): `amount` entries appended to the real reply, each with
    /// index 0, no name, score 0 and the duration bytes `00 00 00 01`, and the count
    /// byte raised to match. Rebuilt byte for byte on the four-player live capture, which
    /// the rules before R12 verified at fourteen (D-238).
    #[test]
    fn the_published_player_faker_is_counted_out() {
        let honest: &[u8] = include_bytes!("../../tests/fixtures/a2s/volatile.player.bin");
        let mut forged = honest.to_vec();
        forged[5] += 10; // the count, after FF FF FF FF 44
        for _ in 0..10 {
            forged.extend_from_slice(&[0x00, 0x00, 0, 0, 0, 0, 0x00, 0x00, 0x00, 0x01]);
        }
        let parse = |bytes: &[u8]| {
            let Datagram::Single(payload) = classify(bytes).unwrap() else {
                panic!("single")
            };
            crate::a2s::players::parse(payload).unwrap()
        };
        let fake = parse(&forged);
        assert_eq!(fake.players.len(), 14);
        let mut i = live_info();
        i.max_players = 60;
        i.players = 14;
        let (v, n, _) = judge(Some(&i), Ok(&fake), 0, 0);
        assert_eq!(
            (v, n),
            (Verdict::Inflated, Some(4)),
            "the four real sessions, not fourteen"
        );
        // No real player at all: ten zero-length entries, a count of nothing.
        let mut empty = vec![0xff, 0xff, 0xff, 0xff, 0x44, 10];
        for _ in 0..10 {
            empty.extend_from_slice(&[0x00, 0x00, 0, 0, 0, 0, 0x00, 0x00, 0x00, 0x01]);
        }
        i.players = 10;
        assert_eq!(judge(Some(&i), Ok(&parse(&empty)), 0, 0).1, Some(0));
        // The honest capture itself is untouched.
        i.players = 4;
        assert_eq!(
            judge(Some(&i), Ok(&parse(honest)), 0, 0),
            (Verdict::Verified, Some(4), "INFO 4 vs PLAYER 4".into())
        );
        // One very young entry alone is a player who has just connected.
        let p = players(&[1963.8, 946.1, 0.0004, 300.0], "");
        assert_eq!(judge(Some(&i), Ok(&p), 0, 0).0, Verdict::Verified);
    }

    /// R12 counts an exact 0.0 as zero-length too (D-314): the tool a byte from its
    /// constant. One alone is still a player who has just connected.
    #[test]
    fn exact_zero_sessions_are_counted_out() {
        let mut i = live_info();
        i.max_players = 60;
        i.players = 14;
        let mut d = vec![1963.8, 946.1, 300.0, 120.5];
        d.extend([0.0; 10]);
        let (v, n, _) = judge(Some(&i), Ok(&players(&d, "")), 0, 0);
        assert_eq!((v, n), (Verdict::Inflated, Some(4)));
        i.players = 5;
        let one = players(&[1963.8, 946.1, 300.0, 120.5, 0.0], "");
        assert_eq!(judge(Some(&i), Ok(&one), 0, 0).0, Verdict::Verified);
    }

    /// R13 (D-314): a full list is honest, one entry past the slots is not — judged
    /// against a slot count that is current, never a stale one.
    #[test]
    fn more_entries_than_slots_is_synthetic() {
        let mut i = live_info();
        i.max_players = 10;
        i.players = 10;
        let full: Vec<f32> = (0..10).map(|k| 100.0 + k as f32 * 61.0).collect();
        assert_eq!(
            judge(Some(&i), Ok(&players(&full, "")), 0, 0).0,
            Verdict::Verified
        );
        let mut over = full.clone();
        over.push(1234.5);
        let over = players(&over, "");
        let (v, n, why) = judge(Some(&i), Ok(&over), 0, 0);
        assert_eq!((v, n), (Verdict::Synthetic, Some(11)), "{why}");
        // The details pane keys its sentence on this wording (src/lib/verdict.ts).
        assert_eq!(why, "11 entries on a server of 10 slots");
        // A fresh listing's slot count is as current as INFO; a stale one is not used.
        assert_eq!(judge(None, Ok(&over), 10, 10).0, Verdict::Synthetic);
        assert_eq!(judge(None, Ok(&over), -1, 10).0, Verdict::Verified);
    }

    /// R14 (D-314): three sessions under a second beside one of ten minutes or more.
    /// The same burst after a restart is its players coming back, and two together
    /// beside old sessions are a duo.
    #[test]
    fn a_sub_second_burst_beside_an_old_session_is_synthetic() {
        let mut i = live_info();
        i.max_players = 60;
        i.players = 6;
        let burst = players(&[2400.0, 830.0, 312.0, 0.4, 0.2, 0.05], "");
        let (v, _, why) = judge(Some(&i), Ok(&burst), 0, 0);
        assert_eq!(v, Verdict::Synthetic, "{why}");
        assert_eq!(why, "3 sessions under a second beside one of 2400 s");
        let back = players(&[4.1, 2.2, 1.3, 0.8, 0.5, 0.3], "");
        assert_eq!(judge(Some(&i), Ok(&back), 0, 0).0, Verdict::Verified);
        let duo = players(&[2400.0, 830.0, 312.0, 95.0, 0.6, 0.4], "");
        assert_eq!(judge(Some(&i), Ok(&duo), 0, 0).0, Verdict::Verified);
    }

    /// Row 23: a check sends the server's facts only when they differ from the row, and
    /// a reply that says nothing about a field (no game port, no version) changes nothing.
    #[test]
    fn a_check_sends_only_the_facts_that_changed() {
        let i = live_info();
        let known = InfoFacts::of(&i);
        assert!(!InfoFacts::of(&i).changes(&known));
        let mut renamed = i.clone();
        renamed.name = "New name".into();
        assert!(InfoFacts::of(&renamed).changes(&known));
        let mut updated = i.clone();
        updated.version = "1.30.100000".into();
        assert!(InfoFacts::of(&updated).changes(&known));
        let mut moved = i.clone();
        moved.game_port = Some(2402);
        assert!(InfoFacts::of(&moved).changes(&known));
        let mut silent = i.clone();
        silent.game_port = None;
        silent.version = "x".into();
        silent.name = String::new();
        assert!(!InfoFacts::of(&silent).changes(&known));
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
            "young but varied: what an honest server looks like just after a restart (D-288)"
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
        // A trio and a duo who each joined together, an hour and twenty minutes in:
        // two durations in whole seconds, five in tenths (D-268).
        i.players = 5;
        let squads = players(&[3600.4, 3600.3, 3600.1, 1200.4, 1200.2], "");
        assert_eq!(judge(Some(&i), Ok(&squads), 0, 0).0, Verdict::Verified);
        // A list that repeats itself exactly is still what it was.
        let copies = players(&[3600.0, 3600.0, 3600.0, 1200.0, 1200.0], "");
        assert_eq!(judge(Some(&i), Ok(&copies), 0, 0).0, Verdict::Synthetic);
        // Two live reconnect waves seconds after a restart (row 22): honest, where whole
        // seconds made both repetitive.
        i.players = 6;
        for wave in [
            [19.2, 19.2, 19.1, 18.8, 18.5, 18.1],
            [30.24, 30.23, 30.21, 29.74, 29.62, 6.33],
        ] {
            assert_eq!(
                judge(Some(&i), Ok(&players(&wave, "")), 0, 0).0,
                Verdict::Verified
            );
        }
    }

    #[test]
    fn fallback_uses_cached_numbers_when_info_is_missing() {
        let p = players(&[], "");
        assert_eq!(judge(None, Ok(&p), 90, 90).0, Verdict::Inflated);
        assert_eq!(Verdict::Inflated.as_str(), "inflated");
    }
}
