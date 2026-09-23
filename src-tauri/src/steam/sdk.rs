//! Steamworks client thread (ADR-002, docs/05 §1–3).
//!
//! One OS thread owns the `steamworks::Client` (initialised as app 221100, which
//! makes Steam show the user as in DayZ, exactly like the official launcher), pumps
//! `run_callbacks()`, and serves server-list refreshes. The server-list callbacks
//! are `Rc`-based in steamworks 0.13.1, so everything touching them stays on this
//! thread; results leave through an unbounded channel in batches of up to 100 ms.
//!
//! Steam caps one internet list request at 10 000 servers (D-041) while ~12 500
//! DayZ servers are live, so a refresh runs a sequence of *partitions* (filter
//! sets) and merges them: populated servers first because they matter most and
//! arrive fastest, then empty ones.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use steamworks::{
    Client, FriendFlags, FriendState, GameServerItem, ItemState, MatchmakingServers,
    PublishedFileId, ServerListCallbacks, ServerListRequest, ServerResponse, UGC,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::a2s::DayzTags;
use crate::browser::ServerRow;

use super::DAYZ_APP_ID;

const BATCH_INTERVAL: Duration = Duration::from_millis(100);
/// Rows per `servers:batch`. A row serialises to 558 B, so 358 is the ~200 KB docs/04
/// §4 sets as the ceiling for one event (D-193).
const BATCH_MAX_ROWS: usize = 358;
const TICK_ACTIVE: Duration = Duration::from_millis(10);
/// Nothing is in flight, so the only reason to wake is to notice a new command,
/// which the 100 ms batch interval already bounds. 50 ms doubled the host's share
/// of the idle CPU budget for nothing (D-160).
const TICK_IDLE: Duration = Duration::from_millis(100);
/// Steam's per-request ceiling; a partition returning exactly this many is truncated.
pub const STEAM_LIST_CAP: usize = 10_000;
/// A non-forced refresh is ignored when the last one completed more recently than this.
pub const MIN_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

pub type Filters = HashMap<String, String>;

/// Pseudo filter key selecting Steam's LAN server discovery (D-087).
pub const LAN_PARTITION_KEY: &str = "lan";

/// Maps with the most genuinely empty servers (cache statistics 2026-09-21, D-044);
/// each `noplayers` + `map` partition stays far below the 10 000 cap.
pub const EMPTY_PARTITION_MAPS: [&str; 10] = [
    "chernarusplus",
    "deerisle",
    "enoch",
    "namalsk",
    "sakhal",
    "banov",
    "Bitterroot",
    "Pripyat",
    "takistanplus",
    "esseker",
];

/// Pause between partition requests: back-to-back requests made Steam's master
/// answer `NoServersListedOnMasterServer` for the last four of twelve (D-046).
const PARTITION_GAP: Duration = Duration::from_secs(3);
/// Stop a multi-partition refresh after this many consecutive empty master answers.
const MAX_CONSECUTIVE_EMPTY: usize = 2;

/// Default (automatic) refresh: only servers with authenticated players. That is
/// every server a player could join, arrives in ~40 s, and skips the ~27 000 fake
/// entries that dominate Steam's empty-server partitions (D-046). Keys are Steam
/// filter *operation codes*; the value is ignored for flag filters.
pub fn default_partitions() -> Vec<Filters> {
    vec![HashMap::from([("hasplayers".to_string(), "1".to_string())])]
}

/// Manual "full" refresh: populated servers, then empty servers per major map,
/// then a generic `noplayers` catch-all that is allowed to hit the cap. Duplicates
/// merge by id. Expect several minutes; fakes are flagged by rule R0 as they arrive.
pub fn full_partitions() -> Vec<Filters> {
    let flag = |k: &str| (k.to_string(), "1".to_string());
    let mut parts = default_partitions();
    for map in EMPTY_PARTITION_MAPS {
        parts.push(HashMap::from([
            flag("noplayers"),
            ("map".to_string(), map.to_string()),
        ]));
    }
    parts.push(HashMap::from([flag("noplayers")]));
    parts
}

/// What Steam's master server says about players for rows of this partition.
fn steam_empty_for(filters: &Filters) -> Option<bool> {
    if filters.contains_key("noplayers") {
        Some(true)
    } else if filters.contains_key("hasplayers") {
        Some(false)
    } else {
        None
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamStatus {
    pub initialized: bool,
    pub error: Option<String>,
    pub app_id: u32,
    pub steam_id: Option<u64>,
    pub persona: Option<String>,
    pub refreshing: bool,
    /// Seconds since the last completed refresh, if any.
    pub last_refresh_secs_ago: Option<u64>,
    /// The Steamworks client was released after inactivity (Q16, D-057); the
    /// next command re-initialises it transparently.
    pub idle: bool,
}

/// Release Steamworks after this long without a command or active job. Disabled by
/// default: measured on 2026-09-21, `SteamAPI_Shutdown` unloads `steamclient64.dll`
/// but the host's private bytes stayed at 61 MB (D-057). The reason to release is a
/// different one (D-077): while a Steamworks session for app 221100 exists, Steam
/// shows the user as playing DayZ and counts playtime. The Settings value drives it;
/// `DAYZ_STEAM_IDLE_SECS=<n>` overrides it for experiments.
fn idle_timeout() -> Option<Duration> {
    std::env::var("DAYZ_STEAM_IDLE_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|s| *s > 0)
        .map(Duration::from_secs)
}

/// Everything derived from one `SteamAPI_Init`; dropping it shuts Steamworks down.
struct Session {
    client: Client,
    mms: MatchmakingServers,
    ugc: UGC,
}

fn open_session() -> Result<Session, String> {
    let client = Client::init_app(DAYZ_APP_ID).map_err(|e| e.to_string())?;
    let mms = client.matchmaking_servers();
    let ugc = client.ugc();
    Ok(Session { client, mms, ugc })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartitionResult {
    pub filters: Filters,
    pub total: usize,
    pub responded: usize,
    pub failed: usize,
    /// Rows flagged by rule R0 (Steam says empty, INFO claims players).
    pub inflated: usize,
    pub elapsed_ms: u64,
    pub response: String,
    /// `total == STEAM_LIST_CAP`: Steam truncated this partition.
    pub capped: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshDone {
    pub total: usize,
    pub responded: usize,
    pub failed: usize,
    pub inflated: usize,
    pub elapsed_ms: u64,
    pub partitions: Vec<PartitionResult>,
    pub capped: bool,
    /// Remaining partitions were skipped after repeated empty master answers (throttling).
    pub stopped_early: bool,
    /// `steam` for the master server, `lan` for LAN discovery (D-087), `dzsa` for
    /// the fallback list (D-089).
    pub source: &'static str,
    /// The request never reached Steam (no session). The caller must not treat this
    /// as a completed refresh: it is an answer so the UI can stop waiting (D-160).
    #[serde(default)]
    pub rejected: bool,
}

/// Where a friend is playing, from `ISteamFriends::GetFriendGamePlayed` (S-64).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendServer {
    pub ip: String,
    pub game_port: u16,
    pub query_port: u16,
}

/// One Steam friend for the Friends tab (D-092).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendInfo {
    /// Decimal SteamID64 as text: JSON numbers lose precision above 2^53.
    pub steam_id: String,
    pub name: String,
    /// `offline | online | invisible | busy | away | snooze | looking_to_trade | looking_to_play`
    pub state: &'static str,
    /// In DayZ (app 221100) right now.
    pub in_dayz: bool,
    /// Set when Steam knows the server (game address and port announced by the client).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<FriendServer>,
}

/// Workshop item metadata from `ISteamUGC` details query (M5 join plan).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDetails {
    pub id: u64,
    pub title: String,
    pub file_size: u64,
    pub time_updated: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemProgress {
    pub id: u64,
    /// `subscribing | subscribed | pending | downloading | needs_update | installed | failed`
    pub state: String,
    pub downloaded: u64,
    pub total: u64,
    pub folder: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub job: u64,
    pub items: Vec<ItemProgress>,
    pub installed: usize,
    pub total: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncDone {
    pub job: u64,
    pub ok: bool,
    pub error: Option<String>,
    pub items: Vec<ItemProgress>,
    pub elapsed_ms: u64,
}

#[derive(Debug)]
pub enum SteamEvent {
    Status(SteamStatus),
    Batch(Vec<ServerRow>),
    Done(RefreshDone),
    SyncProgress(SyncProgress),
    SyncDone(SyncDone),
}

enum Cmd {
    Refresh(Vec<Filters>),
    Sync {
        job: u64,
        ids: Vec<u64>,
    },
    /// Which of these Workshop items Steam says are out of date, live (D-191).
    /// `None` means the client could not be asked, which is not the same as "none".
    StaleItems {
        ids: Vec<u64>,
        reply: mpsc::Sender<Option<Vec<u64>>>,
    },
    ItemDetails {
        ids: Vec<u64>,
        reply: mpsc::Sender<Result<Vec<ItemDetails>, String>>,
    },
    Unsubscribe {
        ids: Vec<u64>,
        reply: UnsubscribeReply,
    },
    Friends {
        reply: FriendsReply,
    },
    FriendAvatar {
        steam_id: u64,
        reply: mpsc::Sender<Option<Avatar>>,
    },
    Shutdown,
}

/// The local user's medium (64×64) Steam avatar as raw RGBA (D-100); the WebView
/// draws it on a canvas, so no image codec is needed on either side.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Avatar {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Per-item outcome of an unsubscribe request.
pub type UnsubscribeResults = Vec<(u64, Result<(), String>)>;
type UnsubscribeReply = mpsc::Sender<Result<UnsubscribeResults, String>>;
type FriendsReply = mpsc::Sender<Result<Vec<FriendInfo>, String>>;

/// Steam answers at most this many items per details request (`kNumUGCResultsPerPage`).
const DETAILS_PAGE: usize = 50;
const SYNC_EMIT_INTERVAL: Duration = Duration::from_millis(250);
/// Re-issue `DownloadItem` when Steam has not started within this time.
const SYNC_KICK_INTERVAL: Duration = Duration::from_secs(5);
const SYNC_TIMEOUT: Duration = Duration::from_secs(45 * 60);
/// Steam normally answers an unsubscribe within a second; ids still silent after this are reported as failed.
const UNSUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(15);

/// Handle owned by the Tauri state; cheap to clone the status, commands go over a channel.
pub struct SteamWorker {
    cmd: mpsc::Sender<Cmd>,
    status: Arc<Mutex<SteamStatus>>,
    /// Unix seconds of the last completed refresh; seeded from the cache so the
    /// throttle survives restarts (D-042).
    last_done: Arc<Mutex<Option<i64>>>,
    idle_after: Arc<Mutex<Option<Duration>>>,
    /// Only the original handle shuts the thread down when dropped.
    owner: bool,
    /// The worker thread, so exit can wait for Steamworks to shut down properly.
    join: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl SteamWorker {
    /// `idle_after`: release the session after that much inactivity (Settings, D-077).
    pub fn spawn(
        events: UnboundedSender<SteamEvent>,
        last_refresh_unix: Option<i64>,
        idle_after: Option<Duration>,
    ) -> Self {
        let (cmd, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(SteamStatus {
            app_id: DAYZ_APP_ID,
            ..Default::default()
        }));
        let last_done = Arc::new(Mutex::new(last_refresh_unix));
        let idle_after = Arc::new(Mutex::new(idle_after));
        let shared = Shared {
            status: Arc::clone(&status),
            last_done: Arc::clone(&last_done),
            idle_after: Arc::clone(&idle_after),
        };
        let join = std::thread::Builder::new()
            .name("steamworks".into())
            .spawn(move || run(rx, events, shared))
            .expect("spawn steamworks thread");
        Self {
            cmd,
            status,
            last_done,
            idle_after,
            owner: true,
            join: Arc::new(Mutex::new(Some(join))),
        }
    }

    /// A command-only handle usable from blocking tasks (e.g. `item_details`).
    pub fn clone_handle(&self) -> SteamWorker {
        SteamWorker {
            cmd: self.cmd.clone(),
            status: Arc::clone(&self.status),
            last_done: Arc::clone(&self.last_done),
            idle_after: Arc::clone(&self.idle_after),
            owner: false,
            join: Arc::clone(&self.join),
        }
    }

    /// Changes the idle release timeout for the running thread (takes effect on its next tick).
    pub fn set_idle_timeout(&self, d: Option<Duration>) {
        *self.idle_after.lock().unwrap_or_else(|e| e.into_inner()) = d;
    }

    pub fn status(&self) -> SteamStatus {
        let mut s = self
            .status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        s.last_refresh_secs_ago = self
            .last_done
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .map(|t| (ServerRow::now_unix() - t).max(0) as u64);
        s
    }

    /// Queues a refresh. Empty `partitions` means [`default_partitions`]. Returns
    /// `Ok(false)` when skipped because one is running or the last one is younger
    /// than [`MIN_REFRESH_INTERVAL`] and `force` is off.
    pub fn refresh(&self, partitions: Vec<Filters>, force: bool) -> Result<bool, String> {
        let s = self.status();
        if !s.initialized {
            return Err(s.error.unwrap_or_else(|| "Steam is not initialised".into()));
        }
        if s.refreshing {
            return Ok(false);
        }
        if !force
            && s.last_refresh_secs_ago
                .is_some_and(|ago| ago < MIN_REFRESH_INTERVAL.as_secs())
        {
            return Ok(false);
        }
        let parts = if partitions.is_empty() {
            default_partitions()
        } else {
            partitions
        };
        self.cmd
            .send(Cmd::Refresh(parts))
            .map_err(|_| "steamworks thread has stopped".to_string())?;
        Ok(true)
    }

    /// Subscribes, downloads and installs Workshop items; progress arrives as
    /// `SteamEvent::SyncProgress`, completion as `SteamEvent::SyncDone`.
    pub fn sync(&self, job: u64, ids: Vec<u64>) -> Result<(), String> {
        let s = self.status();
        if !s.initialized {
            return Err(s.error.unwrap_or_else(|| "Steam is not initialised".into()));
        }
        if ids.is_empty() {
            return Err("nothing to sync".into());
        }
        self.cmd
            .send(Cmd::Sync { job, ids })
            .map_err(|_| "steamworks thread has stopped".to_string())
    }

    /// Unsubscribes Workshop items (mod management, D-075); one result per id.
    /// Blocking: call from a blocking task. Steam removes the files on its own.
    pub fn unsubscribe(&self, ids: &[u64]) -> Result<UnsubscribeResults, String> {
        let s = self.status();
        if !s.initialized {
            return Err(s.error.unwrap_or_else(|| "Steam is not initialised".into()));
        }
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let (reply, rx) = mpsc::channel();
        self.cmd
            .send(Cmd::Unsubscribe {
                ids: ids.to_vec(),
                reply,
            })
            .map_err(|_| "steamworks thread has stopped".to_string())?;
        rx.recv_timeout(UNSUBSCRIBE_TIMEOUT + Duration::from_secs(5))
            .map_err(|_| "Steam did not answer the unsubscribe request".to_string())?
    }

    /// The friends list with presence and game server (D-092); Steam answers from
    /// its local cache, so this is quick. Blocking: call from a blocking task.
    pub fn friends(&self) -> Result<Vec<FriendInfo>, String> {
        let s = self.status();
        if !s.initialized {
            return Err(s.error.unwrap_or_else(|| "Steam is not initialised".into()));
        }
        let (reply, rx) = mpsc::channel();
        self.cmd
            .send(Cmd::Friends { reply })
            .map_err(|_| "steamworks thread has stopped".to_string())?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| "Steam did not answer the friends query in 10 s".to_string())?
    }

    /// Subscribed Workshop items Steam currently considers out of date. Blocking.
    ///
    /// `None` means the client could not be asked — no session, a stopped thread or a
    /// timeout — which callers must not read as "nothing is stale"; it is the signal to
    /// fall back to the `.acf` view (D-191).
    pub fn stale_items(&self, ids: Vec<u64>) -> Option<Vec<u64>> {
        if ids.is_empty() {
            return Some(Vec::new());
        }
        let (reply, rx) = mpsc::channel();
        if self.cmd.send(Cmd::StaleItems { ids, reply }).is_err() {
            return None;
        }
        rx.recv_timeout(Duration::from_secs(5)).ok().flatten()
    }

    /// A friend's small (32×32) avatar, if Steam has it cached (D-115). Blocking.
    pub fn friend_avatar(&self, steam_id: u64) -> Result<Option<Avatar>, String> {
        let s = self.status();
        if !s.initialized {
            return Err(s.error.unwrap_or_else(|| "Steam is not initialised".into()));
        }
        let (reply, rx) = mpsc::channel();
        self.cmd
            .send(Cmd::FriendAvatar { steam_id, reply })
            .map_err(|_| "steamworks thread has stopped".to_string())?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| "Steam did not answer the avatar query in 5 s".to_string())
    }

    /// Workshop titles and sizes, fetched in pages of 50. Blocking: call from a blocking task.
    pub fn item_details(&self, ids: &[u64]) -> Result<Vec<ItemDetails>, String> {
        let s = self.status();
        if !s.initialized {
            return Err(s.error.unwrap_or_else(|| "Steam is not initialised".into()));
        }
        let mut out = Vec::with_capacity(ids.len());
        for page in ids.chunks(DETAILS_PAGE) {
            let (reply, rx) = mpsc::channel();
            self.cmd
                .send(Cmd::ItemDetails {
                    ids: page.to_vec(),
                    reply,
                })
                .map_err(|_| "steamworks thread has stopped".to_string())?;
            let got = rx.recv_timeout(Duration::from_secs(15)).map_err(|_| {
                "Steam did not answer the Workshop details query in 15 s".to_string()
            })??;
            out.extend(got);
        }
        Ok(out)
    }
}

impl SteamWorker {
    /// Stops the worker and waits for `SteamAPI_Shutdown`.
    ///
    /// Tauri exits through `process::exit`, so `Drop` never runs and the Steamworks
    /// threads were still live when the process went away — `steamclient` then asserts
    /// *"Illegal termination of worker thread 'SocketThread'"* and fast-fails with
    /// 0xC0000409 instead of exiting, which also abandons whatever is still in the
    /// write-ahead log. Waiting is bounded: the thread checks for commands every
    /// 100 ms at worst (D-190).
    pub fn shutdown(&self) {
        if !self.owner {
            return;
        }
        let _ = self.cmd.send(Cmd::Shutdown);
        let handle = self.join.lock().ok().and_then(|mut h| h.take());
        if let Some(h) = handle {
            let _ = h.join();
        }
    }
}

impl Drop for SteamWorker {
    fn drop(&mut self) {
        if self.owner {
            let _ = self.cmd.send(Cmd::Shutdown);
        }
    }
}

struct Shared {
    status: Arc<Mutex<SteamStatus>>,
    last_done: Arc<Mutex<Option<i64>>>,
    /// Idle timeout from Settings (D-077); `None` keeps the session open.
    idle_after: Arc<Mutex<Option<Duration>>>,
}

impl Shared {
    fn set_status(&self, events: &UnboundedSender<SteamEvent>, f: impl FnOnce(&mut SteamStatus)) {
        let snapshot = {
            let mut s = self.status.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut s);
            s.clone()
        };
        let _ = events.send(SteamEvent::Status(snapshot));
    }
}

struct ActivePartition {
    filters: Filters,
    req: Arc<Mutex<ServerListRequest>>,
    rows: Rc<RefCell<Vec<ServerRow>>>,
    responded: Rc<Cell<usize>>,
    failed: Rc<Cell<usize>>,
    inflated: Rc<Cell<usize>>,
    done: Rc<Cell<Option<ServerResponse>>>,
    started: Instant,
    last_flush: Instant,
}

struct ActiveRefresh {
    pending: Vec<Filters>,
    current: Option<ActivePartition>,
    results: Vec<PartitionResult>,
    started: Instant,
    /// Earliest time the next partition may be requested.
    next_allowed: Instant,
    consecutive_empty: usize,
    stopped_early: bool,
}

struct ActiveSync {
    job: u64,
    ids: Vec<u64>,
    started: Instant,
    last_emit: Instant,
    /// Subscribe results arrive through Steam call-result callbacks on this thread.
    sub_results: mpsc::Receiver<(u64, Result<(), String>)>,
    failed: HashMap<u64, String>,
    kicked: HashMap<u64, Instant>,
}

fn start_sync(ugc: &UGC, job: u64, ids: Vec<u64>) -> ActiveSync {
    let (tx, rx) = mpsc::channel();
    let mut kicked = HashMap::with_capacity(ids.len());
    for &id in &ids {
        let file = PublishedFileId(id);
        let st = ugc.item_state(file);
        if !st.contains(ItemState::SUBSCRIBED) {
            let txc = tx.clone();
            ugc.subscribe_item(file, move |r| {
                let _ = txc.send((id, r.map_err(|e| format!("{e:?}"))));
            });
        }
        // High priority: start now instead of waiting for Steam's scheduler.
        let _ = ugc.download_item(file, true);
        kicked.insert(id, Instant::now());
    }
    ActiveSync {
        job,
        ids,
        started: Instant::now(),
        last_emit: Instant::now() - SYNC_EMIT_INTERVAL,
        sub_results: rx,
        failed: HashMap::new(),
        kicked,
    }
}

fn item_progress(ugc: &UGC, id: u64, failed: Option<&String>) -> ItemProgress {
    let file = PublishedFileId(id);
    let st = ugc.item_state(file);
    let (downloaded, total) = ugc.item_download_info(file).unwrap_or((0, 0));
    let info = ugc.item_install_info(file);
    let state = if failed.is_some() {
        "failed"
    } else if st.contains(ItemState::DOWNLOADING) {
        "downloading"
    } else if st.contains(ItemState::DOWNLOAD_PENDING) {
        "pending"
    } else if st.contains(ItemState::NEEDS_UPDATE) {
        "needs_update"
    } else if st.contains(ItemState::INSTALLED) {
        "installed"
    } else if st.contains(ItemState::SUBSCRIBED) {
        "subscribed"
    } else {
        "subscribing"
    };
    ItemProgress {
        id,
        state: state.into(),
        downloaded,
        total: total.max(info.as_ref().map_or(0, |i| i.size_on_disk)),
        folder: info.map(|i| i.folder),
        error: failed.cloned(),
    }
}

/// One scheduler tick for the active sync. Returns the completion event when finished.
fn tick_sync(
    ugc: &UGC,
    sync: &mut ActiveSync,
    events: &UnboundedSender<SteamEvent>,
) -> Option<SyncDone> {
    while let Ok((id, r)) = sync.sub_results.try_recv() {
        match r {
            Ok(()) => {
                let _ = ugc.download_item(PublishedFileId(id), true);
                sync.kicked.insert(id, Instant::now());
            }
            Err(e) => {
                sync.failed.insert(id, e);
            }
        }
    }
    let items: Vec<ItemProgress> = sync
        .ids
        .iter()
        .map(|&id| item_progress(ugc, id, sync.failed.get(&id)))
        .collect();
    for p in &items {
        if matches!(p.state.as_str(), "subscribed" | "needs_update") {
            let last = sync.kicked.get(&p.id).copied().unwrap_or(sync.started);
            if last.elapsed() >= SYNC_KICK_INTERVAL {
                let _ = ugc.download_item(PublishedFileId(p.id), true);
                sync.kicked.insert(p.id, Instant::now());
            }
        }
    }
    let installed = items.iter().filter(|p| p.state == "installed").count();
    let elapsed_ms = sync.started.elapsed().as_millis() as u64;
    let finished = installed == items.len();
    let failed = !sync.failed.is_empty();
    let timed_out = sync.started.elapsed() > SYNC_TIMEOUT;
    if finished || failed || timed_out {
        return Some(SyncDone {
            job: sync.job,
            ok: finished,
            error: if finished {
                None
            } else if failed {
                sync.failed.values().next().cloned()
            } else {
                Some("Steam did not finish the download within 45 minutes".into())
            },
            items,
            elapsed_ms,
        });
    }
    if sync.last_emit.elapsed() >= SYNC_EMIT_INTERVAL {
        sync.last_emit = Instant::now();
        let _ = events.send(SteamEvent::SyncProgress(SyncProgress {
            job: sync.job,
            items,
            installed,
            total: sync.ids.len(),
            elapsed_ms,
        }));
    }
    None
}

fn query_details(ugc: &UGC, ids: Vec<u64>, reply: mpsc::Sender<Result<Vec<ItemDetails>, String>>) {
    let files: Vec<PublishedFileId> = ids.into_iter().map(PublishedFileId).collect();
    match ugc.query_items(files) {
        Ok(handle) => handle.fetch(move |res| {
            let out = res
                .map(|results| {
                    (0..results.returned_results())
                        .filter_map(|i| results.get(i))
                        .map(|q| ItemDetails {
                            id: q.published_file_id.0,
                            title: q.title,
                            file_size: q.file_size as u64,
                            time_updated: q.time_updated,
                        })
                        .collect()
                })
                .map_err(|e| format!("{e:?}"));
            let _ = reply.send(out);
        }),
        Err(e) => {
            let _ = reply.send(Err(format!("{e:?}")));
        }
    }
}

/// How often the thread retries `SteamAPI_Init` while Steam is not running (D-125).
const STEAM_RETRY: Duration = Duration::from_secs(10);
/// How often a live session is checked for "Steam has gone away" (D-160). The probe
/// is a process snapshot measured at 2.46 ms here, so ten seconds keeps it at 0.025 %
/// of one core against an 0.2 % idle budget (docs/05 §6, D-192).
const LIVENESS_INTERVAL: Duration = Duration::from_secs(10);
/// The check has to fail for this long before the session is dropped, so that a
/// momentary reconnect inside Steam does not tear a working session down.
const LIVENESS_GRACE: Duration = Duration::from_secs(15);
/// A partition Steam never calls back about is abandoned after this long. The
/// slowest healthy partition measured on 2026-09-21 took 39 s (D-136).
const PARTITION_TIMEOUT: Duration = Duration::from_secs(180);

/// Answers a command that cannot be served because there is no Steam session.
fn reject(cmd: Cmd, events: &UnboundedSender<SteamEvent>, e: String) {
    match cmd {
        Cmd::StaleItems { reply, .. } => {
            let _ = reply.send(None);
        }
        Cmd::ItemDetails { reply, .. } => {
            let _ = reply.send(Err(e));
        }
        Cmd::Unsubscribe { reply, .. } => {
            let _ = reply.send(Err(e));
        }
        Cmd::Friends { reply } => {
            let _ = reply.send(Err(e));
        }
        Cmd::FriendAvatar { reply, .. } => {
            let _ = reply.send(None);
        }
        Cmd::Sync { job, .. } => {
            let _ = events.send(SteamEvent::SyncDone(SyncDone {
                job,
                ok: false,
                error: Some(e),
                items: Vec::new(),
                elapsed_ms: 0,
            }));
        }
        // A refresh must be answered, not dropped (D-159). The caller already told the
        // UI a refresh had started, and only a `Done` clears that, so silently ignoring
        // this left "refreshing…" and "verifying player counts…" on screen for ever.
        Cmd::Refresh(partitions) => {
            crate::log_warn!("steam", "refresh rejected, no session: {e}");
            let _ = events.send(SteamEvent::Done(RefreshDone {
                total: 0,
                responded: 0,
                failed: 0,
                inflated: 0,
                elapsed_ms: 0,
                partitions: Vec::new(),
                capped: false,
                stopped_early: true,
                rejected: true,
                source: if partitions.iter().any(|p| p.contains_key("lan")) {
                    "lan"
                } else {
                    "steam"
                },
            }));
        }
        Cmd::Shutdown => {}
    }
}

fn run(rx: mpsc::Receiver<Cmd>, events: UnboundedSender<SteamEvent>, shared: Shared) {
    // Steam may start after the launcher (auto-start, a cold boot, D-125): keep
    // trying every ten seconds, answering commands with the error meanwhile.
    let mut session: Option<Session> = None;
    let mut next_try = Instant::now();
    while session.is_none() {
        if Instant::now() >= next_try {
            match open_session() {
                Ok(s) => session = Some(s),
                Err(e) => {
                    shared.set_status(&events, |st| st.error = Some(e));
                    next_try = Instant::now() + STEAM_RETRY;
                }
            }
        }
        if session.is_none() {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(Cmd::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Ok(cmd) => reject(cmd, &events, "Steam is not running".into()),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
    {
        let s = session.as_ref().expect("session just opened");
        let steam_id = s.client.user().steam_id().raw();
        let persona = s.client.friends().name();
        shared.set_status(&events, |st| {
            st.initialized = true;
            st.error = None;
            st.steam_id = Some(steam_id);
            st.persona = Some(persona);
        });
    }

    let env_idle = idle_timeout();
    let mut last_activity = Instant::now();
    let mut last_liveness = Instant::now();
    // Since when the liveness check has been failing.
    let mut lost_since: Option<Instant> = None;
    let mut active: Option<ActiveRefresh> = None;
    let mut sync: Option<ActiveSync> = None;
    let mut unsub: Option<ActiveUnsubscribe> = None;

    loop {
        if let Some(s) = &session {
            s.client.run_callbacks();
        }

        loop {
            let cmd = match rx.try_recv() {
                Ok(cmd) => cmd,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            };
            // Presence reads (friends list, avatar) come from Steam's local cache and
            // must not keep an otherwise idle session alive (D-077, D-103); every
            // other command, and a session re-opened for any command, counts.
            if !matches!(cmd, Cmd::Friends { .. } | Cmd::FriendAvatar { .. }) {
                last_activity = Instant::now();
            }
            if session.is_none() && !matches!(cmd, Cmd::Shutdown) {
                match open_session() {
                    Ok(s) => {
                        session = Some(s);
                        last_activity = Instant::now();
                        lost_since = None;
                        shared.set_status(&events, |st| {
                            st.initialized = true;
                            st.idle = false;
                            st.error = None;
                        });
                    }
                    Err(e) => {
                        // A failed re-open left `initialized` true, so the UI kept
                        // reporting a connection that no longer existed (D-160).
                        shared.set_status(&events, |st| {
                            st.initialized = false;
                            st.error = Some(e.clone());
                        });
                        reject(cmd, &events, e);
                        continue;
                    }
                }
            }
            let Some(s) = session.as_ref() else { return }; // only reachable for Shutdown
            let (mms, ugc) = (&s.mms, &s.ugc);
            match cmd {
                Cmd::Sync { job, ids } => {
                    if let Some(old) = sync.take() {
                        let _ = events.send(SteamEvent::SyncDone(SyncDone {
                            job: old.job,
                            ok: false,
                            error: Some("superseded by a newer sync".into()),
                            items: Vec::new(),
                            elapsed_ms: old.started.elapsed().as_millis() as u64,
                        }));
                    }
                    sync = Some(start_sync(ugc, job, ids));
                }
                Cmd::StaleItems { ids, reply } => {
                    // Subscribed *and* out of date. An item can be installed on disk
                    // without a subscription — left over from an unsubscribe Steam has
                    // not collected, or pulled in by something else — and Steam does
                    // not maintain those, so offering to update one is offering
                    // something the client will not do (D-191).
                    let stale = ids
                        .into_iter()
                        .filter(|&id| {
                            let st = ugc.item_state(PublishedFileId(id));
                            st.contains(ItemState::SUBSCRIBED)
                                && st.contains(ItemState::NEEDS_UPDATE)
                        })
                        .collect();
                    let _ = reply.send(Some(stale));
                }
                Cmd::ItemDetails { ids, reply } => query_details(ugc, ids, reply),
                Cmd::Unsubscribe { ids, reply } => {
                    if let Some(old) = unsub.take() {
                        let _ = old.reply.send(Err("superseded by a newer request".into()));
                    }
                    unsub = Some(start_unsubscribe(ugc, ids, reply));
                }
                Cmd::Friends { reply } => {
                    let _ = reply.send(Ok(list_friends(&s.client)));
                }
                Cmd::FriendAvatar { steam_id, reply } => {
                    let avatar = s
                        .client
                        .friends()
                        .get_friend(steamworks::SteamId::from_raw(steam_id))
                        .small_avatar()
                        .filter(|rgba| rgba.len() == 32 * 32 * 4)
                        .map(|rgba| Avatar {
                            width: 32,
                            height: 32,
                            rgba,
                        });
                    let _ = reply.send(avatar);
                }
                // `refresh()` guards on `status.refreshing`, which the worker only
                // sets when it dequeues, so two calls inside one tick both pass it. The
                // second used to be discarded in silence — a manual Full refresh
                // downgraded to whatever was already running, with no event to say so.
                // D-159 made this rule for the no-session path; the busy path kept it
                // (D-197).
                Cmd::Refresh(_) if active.is_some() => {
                    let _ = events.send(SteamEvent::Done(RefreshDone {
                        total: 0,
                        responded: 0,
                        failed: 0,
                        inflated: 0,
                        elapsed_ms: 0,
                        partitions: Vec::new(),
                        capped: false,
                        stopped_early: true,
                        rejected: true,
                        source: "steam",
                    }));
                }
                Cmd::Refresh(mut parts) => {
                    if active.is_some() {
                        continue;
                    }
                    let _ = mms; // partitions start below on the same session
                    parts.reverse(); // pop() takes from the end
                    active = Some(ActiveRefresh {
                        pending: parts,
                        current: None,
                        results: Vec::new(),
                        started: Instant::now(),
                        next_allowed: Instant::now(),
                        consecutive_empty: 0,
                        stopped_early: false,
                    });
                    shared.set_status(&events, |s| {
                        s.refreshing = true;
                        s.error = None;
                    });
                }
                Cmd::Shutdown => return,
            }
        }

        let busy = active.is_some() || sync.is_some() || unsub.is_some();
        let idle_after =
            env_idle.or_else(|| *shared.idle_after.lock().unwrap_or_else(|e| e.into_inner()));
        if busy {
            last_activity = Instant::now();
        } else if session.is_some() && idle_after.is_some_and(|d| last_activity.elapsed() >= d) {
            // Drops Client/MatchmakingServers/UGC → SteamAPI_Shutdown; frees the
            // Steam client heaps (Q16). Any later command re-opens the session.
            session = None;
            shared.set_status(&events, |st| st.idle = true);
        }

        // Steam can quit while the launcher runs, and Steamworks does not report it:
        // the session was left in place, `initialized` stayed true, an in-flight
        // refresh never completed and every later command went to a dead client, with
        // "Refreshing…" on screen until the app was restarted (D-160).
        // D-160 used `ISteamUser::BLoggedOn` to notice Steam going away. It is the
        // wrong signal: it reports false whenever the client is briefly offline or
        // reconnecting, which happens routinely, so the launcher tore the session down
        // and rebuilt it in a loop — logged live as "session is gone" / "session
        // re-opened" ten seconds apart, with a refresh landing "0 listed, 0 answered,
        // early=true" in between. Dropping the `Client` also calls `SteamAPI_Shutdown`
        // while Steam's own socket thread is mid-flight, which is what made
        // `steamclient` assert and fast-fail the process.
        //
        // `SteamAPI_IsSteamRunning` was the next attempt and is no better: it answers
        // from `HKCU\Software\Valve\Steam\ActiveProcess\pid`, the one value this
        // repository already records as unreliable — D-026 caught it reading 16884
        // while the live steam.exe was 15176, and it still read 16884 against 14916
        // today. A launcher built on it declared "Steam is no longer running" twenty
        // seconds after every start, with Steam plainly up (D-192). `registry::detect`
        // is what the rest of the app has always used: it enumerates processes and
        // requires the image to live under `SteamPath`, so it needs no cooperation from
        // Steam and no key can make it wrong.
        //
        // The session is no longer dropped when this fires: an unusable `Client` costs
        // nothing to hold, and holding it keeps `SteamAPI_Shutdown` out of a code path
        // that cannot run it safely. Only the deliberate idle release (D-077), which
        // runs when nothing is in flight, still drops it (D-190).
        if session.is_some() && last_liveness.elapsed() >= LIVENESS_INTERVAL {
            last_liveness = Instant::now();
            let alive = crate::steam::registry::detect().running;
            match (alive, lost_since) {
                (true, None) => {}
                (true, Some(_)) => {
                    // It came back inside the grace period, or after we reported it gone.
                    let was_reported_gone = lost_since
                        .is_some_and(|t: Instant| t.elapsed() >= LIVENESS_GRACE)
                        && !shared
                            .status
                            .lock()
                            .map(|st| st.initialized)
                            .unwrap_or(false);
                    lost_since = None;
                    if was_reported_gone {
                        // Steam really did go away, so the handles this session holds
                        // point at a client that no longer exists: D-190 stopped
                        // dropping them at the moment of the loss, which is unsafe, but
                        // marking the session live again without re-opening it left a
                        // corpse — every later refresh spun for the full 180 s partition
                        // timeout and came back "0 listed". Dropping it *here* is safe:
                        // Steam has just started, so nothing of ours is in flight, and
                        // the next command re-initialises (D-194).
                        session = None;
                        crate::log_info!("steam", "Steam is back; the session will re-open");
                        shared.set_status(&events, |st| {
                            st.initialized = true;
                            st.error = None;
                        });
                    }
                }
                (false, None) => lost_since = Some(Instant::now()),
                (false, Some(t)) if t.elapsed() >= LIVENESS_GRACE => {
                    // Report it once, not every interval.
                    if shared
                        .status
                        .lock()
                        .map(|st| st.initialized)
                        .unwrap_or(false)
                    {
                        crate::log_warn!("steam", "Steam is no longer running");
                        // Everything in flight belonged to a Steam that is gone, and
                        // each one holds a spinner in the UI until it is answered.
                        if let Some(r) = active.take() {
                            let _ = events.send(SteamEvent::Done(RefreshDone {
                                total: 0,
                                responded: 0,
                                failed: 0,
                                inflated: 0,
                                elapsed_ms: r.started.elapsed().as_millis() as u64,
                                partitions: Vec::new(),
                                capped: false,
                                stopped_early: true,
                                rejected: true,
                                source: "steam",
                            }));
                        }
                        if let Some(job) = sync.take() {
                            let _ = events.send(SteamEvent::SyncDone(SyncDone {
                                job: job.job,
                                ok: false,
                                error: Some("Steam closed during the download".into()),
                                items: Vec::new(),
                                elapsed_ms: job.started.elapsed().as_millis() as u64,
                            }));
                        }
                        if let Some(u) = unsub.take() {
                            let _ = u.reply.send(Err("Steam closed".into()));
                        }
                        shared.set_status(&events, |st| {
                            st.initialized = false;
                            st.refreshing = false;
                            st.idle = false;
                            st.error = Some("Steam is no longer running".into());
                        });
                    }
                }
                (false, Some(_)) => {}
            }
        }

        let Some(s) = session.as_ref() else {
            std::thread::sleep(TICK_IDLE);
            continue;
        };
        let (mms, ugc) = (&s.mms, &s.ugc);

        if let Some(r) = active.as_mut() {
            // Start the next partition when none is running and the gap has passed.
            if r.current.is_none() && Instant::now() >= r.next_allowed {
                if r.consecutive_empty >= MAX_CONSECUTIVE_EMPTY && !r.pending.is_empty() {
                    r.pending.clear();
                    r.stopped_early = true;
                }
                match r.pending.pop() {
                    Some(filters) => match start_partition(mms, filters) {
                        Ok(p) => r.current = Some(p),
                        Err(e) => shared.set_status(&events, |s| s.error = Some(e)),
                    },
                    None => {
                        let lan_only = !r.results.is_empty()
                            && r.results
                                .iter()
                                .all(|p| p.filters.contains_key(LAN_PARTITION_KEY));
                        let done = RefreshDone {
                            source: if lan_only { "lan" } else { "steam" },
                            total: r.results.iter().map(|p| p.total).sum(),
                            responded: r.results.iter().map(|p| p.responded).sum(),
                            failed: r.results.iter().map(|p| p.failed).sum(),
                            inflated: r.results.iter().map(|p| p.inflated).sum(),
                            elapsed_ms: r.started.elapsed().as_millis() as u64,
                            capped: r.results.iter().any(|p| p.capped),
                            stopped_early: r.stopped_early,
                            rejected: false,
                            partitions: std::mem::take(&mut r.results),
                        };
                        let _ = events.send(SteamEvent::Done(done));
                        *shared.last_done.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(ServerRow::now_unix());
                        active = None;
                        shared.set_status(&events, |s| s.refreshing = false);
                    }
                }
            }
            if let Some(r) = active.as_mut() {
                if let Some(p) = r.current.as_mut() {
                    let finished = p.done.get();
                    // Steam calls back once per partition; when it never does, nothing
                    // else in this loop can end the refresh (D-160).
                    let timed_out = finished.is_none() && p.started.elapsed() >= PARTITION_TIMEOUT;
                    // Steam can hand over several thousand rows inside one 100 ms
                    // window — the first callback of a partition usually does — and
                    // the flush took whatever had accumulated, so a single
                    // `servers:batch` could be megabytes against the ~200 KB docs/04
                    // §4 asks for. Split it; the loop comes back in 10 ms while a
                    // refresh is active, so nothing is held up (D-193).
                    let ending = finished.is_some() || timed_out;
                    let ready = p.rows.borrow().len();
                    if ending || ready >= BATCH_MAX_ROWS || p.last_flush.elapsed() >= BATCH_INTERVAL
                    {
                        loop {
                            let batch: Vec<ServerRow> = {
                                let mut rows = p.rows.borrow_mut();
                                let take = rows.len().min(BATCH_MAX_ROWS);
                                if take == 0 {
                                    break;
                                }
                                rows.drain(..take).collect()
                            };
                            let _ = events.send(SteamEvent::Batch(batch));
                            // The partition is about to be taken below, so everything
                            // still queued has to leave now or it is dropped.
                            if !ending {
                                break;
                            }
                        }
                        p.last_flush = Instant::now();
                    }
                    if finished.is_some() || timed_out {
                        let total = p
                            .req
                            .lock()
                            .map(|q| q.get_server_count().unwrap_or(0))
                            .unwrap_or(0)
                            .max(0) as usize;
                        if let Ok(mut q) = p.req.lock() {
                            let _ = q.release();
                        }
                        let p = r.current.take().expect("current partition");
                        // Only when another request follows. The completion branch is
                        // gated on the same clock, so arming this unconditionally held
                        // `servers:done` back by the full gap after the last partition
                        // had already finished — measured at exactly 3 000 ms (D-197).
                        if !r.pending.is_empty() {
                            r.next_allowed = Instant::now() + PARTITION_GAP;
                        }
                        let no_answer = timed_out
                            || (total == 0
                                && finished == Some(ServerResponse::NoServersListedOnMasterServer));
                        if no_answer {
                            r.consecutive_empty += 1;
                        } else {
                            r.consecutive_empty = 0;
                        }
                        if timed_out {
                            crate::log_warn!(
                                "steam",
                                "partition {:?} never answered in {} s; abandoned",
                                p.filters,
                                PARTITION_TIMEOUT.as_secs()
                            );
                        }
                        r.results.push(PartitionResult {
                            filters: p.filters,
                            total,
                            responded: p.responded.get(),
                            failed: p.failed.get(),
                            inflated: p.inflated.get(),
                            elapsed_ms: p.started.elapsed().as_millis() as u64,
                            response: match finished {
                                Some(response) => format!("{response:?}"),
                                None => "NoAnswer".into(),
                            },
                            capped: total >= STEAM_LIST_CAP,
                        });
                    }
                }
            }
        }

        if let Some(job) = sync.as_mut() {
            if let Some(done) = tick_sync(ugc, job, &events) {
                let _ = events.send(SteamEvent::SyncDone(done));
                sync = None;
            }
        }

        if unsub.as_mut().is_some_and(tick_unsubscribe) {
            unsub = None;
        }

        std::thread::sleep(if active.is_some() || sync.is_some() || unsub.is_some() {
            TICK_ACTIVE
        } else {
            TICK_IDLE
        });
    }
}

/// In-flight unsubscribe request: Steam answers each id through a callback that
/// `run_callbacks` delivers on this thread, so results are collected across ticks.
struct ActiveUnsubscribe {
    ids: Vec<u64>,
    results: UnsubscribeResults,
    rx: mpsc::Receiver<(u64, Result<(), String>)>,
    reply: UnsubscribeReply,
    started: Instant,
}

fn start_unsubscribe(ugc: &UGC, ids: Vec<u64>, reply: UnsubscribeReply) -> ActiveUnsubscribe {
    let (tx, rx) = mpsc::channel();
    for &id in &ids {
        let txc = tx.clone();
        ugc.unsubscribe_item(PublishedFileId(id), move |r| {
            let _ = txc.send((id, r.map_err(|e| format!("{e:?}"))));
        });
    }
    ActiveUnsubscribe {
        ids,
        results: Vec::new(),
        rx,
        reply,
        started: Instant::now(),
    }
}

/// Returns `true` once every id has answered or the timeout passed and the reply was sent.
fn tick_unsubscribe(u: &mut ActiveUnsubscribe) -> bool {
    while let Ok(r) = u.rx.try_recv() {
        u.results.push(r);
    }
    if u.results.len() < u.ids.len() && u.started.elapsed() < UNSUBSCRIBE_TIMEOUT {
        return false;
    }
    let mut results = std::mem::take(&mut u.results);
    for &id in &u.ids {
        if !results.iter().any(|(i, _)| *i == id) {
            results.push((id, Err("Steam did not confirm the unsubscribe".into())));
        }
    }
    let _ = u.reply.send(Ok(results));
    true
}

fn friend_state_name(state: FriendState) -> &'static str {
    match state {
        FriendState::Offline => "offline",
        FriendState::Online => "online",
        FriendState::Invisible => "invisible",
        FriendState::Busy => "busy",
        FriendState::Away => "away",
        FriendState::Snooze => "snooze",
        FriendState::LookingToTrade => "looking_to_trade",
        FriendState::LookingToPlay => "looking_to_play",
    }
}

/// Server address from a rich-presence `connect` string (S-65): Steam hands this
/// text to the game as its command line when a friend clicks "Join Game", so it is
/// whatever the game chose: `+connect 1.2.3.4:2302`, `-connect=1.2.3.4 -port=2302`,
/// `connect 1.2.3.4`. The first IPv4 wins; a port after a colon or a later bare
/// number follows it; no port means DayZ's default game port 2302.
fn parse_connect(text: &str) -> Option<(std::net::Ipv4Addr, u16)> {
    let mut ip: Option<std::net::Ipv4Addr> = None;
    let mut port: Option<u16> = None;
    for tok in text.split(|c: char| c.is_whitespace() || c == '=' || c == ',' || c == ';') {
        let t = tok.trim_matches(|c| c == '+' || c == '-' || c == '"' || c == '\'');
        if ip.is_none() {
            if let Some((a, p)) = t.rsplit_once(':') {
                if let Ok(a) = a.parse() {
                    ip = Some(a);
                    port = p.parse().ok();
                    continue;
                }
            }
            if let Ok(a) = t.parse() {
                ip = Some(a);
            }
        } else if port.is_none() {
            if let Ok(p) = t.parse::<u16>() {
                if p != 0 {
                    port = Some(p);
                }
            }
        }
    }
    ip.filter(|a| !a.is_unspecified())
        .map(|a| (a, port.unwrap_or(2302)))
}

/// Regular friends (`FriendFlags::IMMEDIATE`) with presence and, for those in DayZ,
/// the server Steam knows them on (S-64): the game-server address Steam records when
/// the client authenticates with a server, or failing that a rich-presence `connect`
/// string (S-65). Presence for friends in DayZ is (re)requested each call so a value
/// that arrives later shows on the next poll. Sorted: in DayZ first, then online,
/// then by name. `GameId::app_id` masks the low 24 bits, which is where the app id lives.
fn list_friends(client: &Client) -> Vec<FriendInfo> {
    let friends = client.friends();
    let mut out: Vec<FriendInfo> = friends
        .get_friends(FriendFlags::IMMEDIATE)
        .iter()
        .map(|f| {
            let game = f.game_played();
            let in_dayz = game
                .as_ref()
                .is_some_and(|g| g.game.app_id().0 == DAYZ_APP_ID);
            let mut server = game
                .filter(|g| in_dayz && !g.game_address.is_unspecified() && g.game_port != 0)
                .map(|g| FriendServer {
                    ip: g.game_address.to_string(),
                    game_port: g.game_port,
                    query_port: g.query_port,
                });
            if in_dayz && server.is_none() {
                server = f
                    .rich_presence("connect")
                    .and_then(|c| parse_connect(&c))
                    .map(|(ip, game_port)| FriendServer {
                        ip: ip.to_string(),
                        game_port,
                        query_port: 0,
                    });
                // SAFETY: the flat accessor returns this session's live ISteamFriends;
                // the request is asynchronous and rate-limited by Steam.
                unsafe {
                    steamworks::sys::SteamAPI_ISteamFriends_RequestFriendRichPresence(
                        steamworks::sys::SteamAPI_SteamFriends_v018(),
                        f.id().raw(),
                    );
                }
            }
            FriendInfo {
                steam_id: f.id().raw().to_string(),
                name: f.name(),
                state: friend_state_name(f.state()),
                in_dayz,
                server,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.in_dayz
            .cmp(&a.in_dayz)
            .then((a.state == "offline").cmp(&(b.state == "offline")))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

fn start_partition(mms: &MatchmakingServers, filters: Filters) -> Result<ActivePartition, String> {
    let rows = Rc::new(RefCell::new(Vec::with_capacity(256)));
    let responded = Rc::new(Cell::new(0usize));
    let failed = Rc::new(Cell::new(0usize));
    let inflated = Rc::new(Cell::new(0usize));
    let done: Rc<Cell<Option<ServerResponse>>> = Rc::new(Cell::new(None));

    let steam_empty = steam_empty_for(&filters);
    let (rows_cb, responded_cb, failed_cb, inflated_cb, done_cb) = (
        Rc::clone(&rows),
        Rc::clone(&responded),
        Rc::clone(&failed),
        Rc::clone(&inflated),
        Rc::clone(&done),
    );
    let callbacks = ServerListCallbacks::new(
        Box::new(move |list: Arc<Mutex<ServerListRequest>>, index: i32| {
            let item = list
                .lock()
                .ok()
                .and_then(|q| q.get_server_details(index).ok());
            if let Some(item) = item {
                responded_cb.set(responded_cb.get() + 1);
                let row = row_from(item, steam_empty);
                if row.inflated() {
                    inflated_cb.set(inflated_cb.get() + 1);
                }
                rows_cb.borrow_mut().push(row);
            }
        }),
        Box::new(move |_list: Arc<Mutex<ServerListRequest>>, _index: i32| {
            failed_cb.set(failed_cb.get() + 1);
        }),
        Box::new(
            move |_list: Arc<Mutex<ServerListRequest>>, response: ServerResponse| {
                done_cb.set(Some(response));
            },
        ),
    );

    // A partition with the pseudo-key `lan` asks Steam's LAN discovery instead of the
    // master server (D-087); it takes no filters and its rows carry no Steam count.
    let req = if filters.contains_key(LAN_PARTITION_KEY) {
        mms.lan_server_list(DAYZ_APP_ID, callbacks)
    } else {
        let borrowed: HashMap<&str, &str> = filters
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        mms.internet_server_list(DAYZ_APP_ID, &borrowed, callbacks)
            .map_err(|()| "server list filter key or value exceeds 255 bytes".to_string())?
    };
    Ok(ActivePartition {
        filters,
        req,
        rows,
        responded,
        failed,
        inflated,
        done,
        started: Instant::now(),
        last_flush: Instant::now(),
    })
}

fn row_from(item: GameServerItem, steam_empty: Option<bool>) -> ServerRow {
    let ip = item.addr.to_string();
    let tags = DayzTags::parse(&item.tags);
    ServerRow {
        id: ServerRow::id_for(&ip, item.query_port),
        country: ServerRow::country_for(&ip),
        ip,
        game_port: item.connection_port,
        query_port: item.query_port,
        name: item.server_name,
        map: item.map,
        description: item.game_description,
        players: item.players,
        max_players: item.max_players,
        bots: item.bot_players,
        password: item.have_password,
        secure: item.secure,
        server_version: item.server_version,
        version: ServerRow::version_string(item.server_version),
        ping_ms: item.ping.as_millis() as u32,
        keywords: item.tags,
        tags,
        steam_id: item.steamid,
        last_seen: ServerRow::now_unix(),
        verified_players: None,
        steam_empty,
        verified_at: None,
        verdict: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partitions_order_and_flags() {
        let d = default_partitions();
        assert_eq!(d.len(), 1);
        assert!(d[0].contains_key("hasplayers"));
        let p = full_partitions();
        assert_eq!(p.len(), 2 + EMPTY_PARTITION_MAPS.len());
        assert!(p[0].contains_key("hasplayers"));
        assert_eq!(steam_empty_for(&p[0]), Some(false));
        assert_eq!(p[1].get("map").map(String::as_str), Some("chernarusplus"));
        assert_eq!(steam_empty_for(&p[1]), Some(true));
        assert!(
            p.last().unwrap().contains_key("noplayers") && !p.last().unwrap().contains_key("map")
        );
        assert_eq!(steam_empty_for(&HashMap::new()), None);
    }

    #[test]
    fn connect_strings() {
        let ip = |s: &str| s.parse::<std::net::Ipv4Addr>().unwrap();
        assert_eq!(
            parse_connect("+connect 51.81.8.81:2402"),
            Some((ip("51.81.8.81"), 2402))
        );
        assert_eq!(
            parse_connect("-connect=51.81.8.81 -port=2402"),
            Some((ip("51.81.8.81"), 2402))
        );
        assert_eq!(
            parse_connect("connect 51.81.8.81"),
            Some((ip("51.81.8.81"), 2302))
        );
        assert_eq!(
            parse_connect("-connect=\"51.81.8.81\" -port=\"2402\" -mod=@CF"),
            Some((ip("51.81.8.81"), 2402))
        );
        assert_eq!(parse_connect("+connect 0.0.0.0:0"), None);
        assert_eq!(parse_connect(""), None);
        assert_eq!(parse_connect("-nolauncher"), None);
    }
}
