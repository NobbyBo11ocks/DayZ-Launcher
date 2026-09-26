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
    CallbackHandle, Client, DownloadItemResult, FriendFlags, FriendState, GameServerItem,
    ItemState, MatchmakingServers, PublishedFileId, ReleaseError, ServerListCallbacks,
    ServerListRequest, ServerResponse, UGC,
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
/// then a `noplayers` catch-all for maps outside that list. Duplicates merge by id.
/// Expect several minutes; fakes are flagged by rule R0 as they arrive.
///
/// The catch-all asks for one server per address (`collapse_addr_hash`, S-83): it
/// used to hit the 10 000 cap re-listing the ten maps' farm rows — 0 of its rows
/// were on another map in the 2026-09-25 cache — at the cost of 10 000 pings. A map
/// partition that hits the cap gets the same collapsed request queued behind it
/// (see [`COLLAPSE_KEY`]), so every address keeps at least one listed server on that
/// map when the farms overflow it: enoch, namalsk and chernarusplus each carried
/// 10 000–14 800 empty entries that day, 96–98 % fake (D-245).
pub fn full_partitions() -> Vec<Filters> {
    let flag = |k: &str| (k.to_string(), "1".to_string());
    let mut parts = default_partitions();
    for map in EMPTY_PARTITION_MAPS {
        parts.push(HashMap::from([
            flag("noplayers"),
            ("map".to_string(), map.to_string()),
        ]));
    }
    parts.push(HashMap::from([flag("noplayers"), flag(COLLAPSE_KEY)]));
    parts
}

/// Steam's "one server per unique address" filter (`isteammatchmaking.h`, S-83).
pub const COLLAPSE_KEY: &str = "collapse_addr_hash";

/// The follow-up request for a map partition Steam truncated: the same filters plus
/// one server per address, which no farm can fill (D-245).
fn collapsed(filters: &Filters) -> Option<Filters> {
    if !filters.contains_key("map") || filters.contains_key(COLLAPSE_KEY) {
        return None;
    }
    let mut f = filters.clone();
    f.insert(COLLAPSE_KEY.to_string(), "1".to_string());
    Some(f)
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
    /// The Steamworks client was released after inactivity (Q16, D-057); the next
    /// command that needs a session re-opens it. The reads Steam answers from its cache
    /// (the Workshop update flags, friends' avatars) do not (D-220, D-275).
    pub idle: bool,
}

/// Release Steamworks after this long without a command or active job: 15 minutes by
/// default, from Settings (D-077). Memory is not the reason — measured on 2026-09-21,
/// `SteamAPI_Shutdown` unloads `steamclient64.dll` but the host's private bytes stayed
/// at 61 MB (D-057) — playtime is: while a Steamworks session for app 221100 exists,
/// Steam shows the user as playing DayZ and counts it. `DAYZ_STEAM_IDLE_SECS=<n>`
/// overrides the setting for experiments.
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
    /// Workshop downloads Steam finished with an error, as (item, error) (D-242).
    download_errors: mpsc::Receiver<(u64, String)>,
    /// Keeps the `DownloadItemResult` callback registered for the session's life.
    _on_download: CallbackHandle,
}

fn open_session() -> Result<Session, String> {
    let client = Client::init_app(DAYZ_APP_ID).map_err(|e| e.to_string())?;
    let mms = client.matchmaking_servers();
    let ugc = client.ugc();
    // Nothing listened for a failed download: a Steam in offline mode, a full disk or
    // an item that cannot be fetched showed "Queued" until the stall limit, re-kicked
    // every five seconds, and the busy sync held the idle release off the whole time.
    let (tx, download_errors) = mpsc::channel();
    let on_download = client.register_callback(move |r: DownloadItemResult| {
        if r.app_id.0 == DAYZ_APP_ID {
            if let Some(e) = r.error {
                let _ = tx.send((r.published_file_id.0, format!("{e:?}")));
            }
        }
    });
    Ok(Session {
        client,
        mms,
        ugc,
        download_errors,
        _on_download: on_download,
    })
}

/// Publishes a session that has just opened, and returns the pid of the `steam.exe`
/// it talks to. Every open goes through here: the on-demand re-open used to set the
/// flags only, so a Steam restarted under another account kept the old persona and
/// id on screen (D-236).
fn publish_open(shared: &Shared, events: &UnboundedSender<SteamEvent>, s: &Session) -> u32 {
    let steam_id = s.client.user().steam_id().raw();
    let persona = s.client.friends().name();
    shared.set_status(events, |st| {
        st.initialized = true;
        st.idle = false;
        st.error = None;
        st.steam_id = Some(steam_id);
        st.persona = Some(persona);
    });
    crate::steam::registry::detect().pid
}

/// Answers everything in flight when the Steam it was running against is gone. Each
/// one holds a spinner in the UI until it is answered (D-160).
fn abandon_in_flight(
    active: &mut Option<ActiveRefresh>,
    sync: &mut Option<ActiveSync>,
    unsub: &mut Option<ActiveUnsubscribe>,
    events: &UnboundedSender<SteamEvent>,
    why: &str,
) {
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
            complete: false,
            rejected: true,
            reason: Some("no-session"),
            source: "steam",
        }));
    }
    if let Some(job) = sync.take() {
        let _ = events.send(SteamEvent::SyncDone(SyncDone {
            job: job.job,
            ok: false,
            error: Some(format!("{why} during the download")),
            failed_id: None,
            superseded: false,
            items: Vec::new(),
            elapsed_ms: job.started.elapsed().as_millis() as u64,
        }));
    }
    if let Some(u) = unsub.take() {
        let _ = u.reply.send(Err(why.to_string()));
    }
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
    /// Steam finished the `hasplayers` answer, uncapped: only such a refresh may withdraw
    /// the vouch from servers it did not list (D-233). Decided here, where the partition
    /// answers are known, so the host and the store use the same test instead of each
    /// inferring it from the other flags (D-236); on that partition alone (`vouch_complete`,
    /// D-271).
    #[serde(default)]
    pub complete: bool,
    /// `steam` for the master server, `lan` for LAN discovery (D-087), `dzsa` for
    /// the fallback list (D-089).
    pub source: &'static str,
    /// The request never reached Steam. The caller must not treat this as a completed
    /// refresh: it is an answer so the UI can stop waiting (D-160).
    #[serde(default)]
    pub rejected: bool,
    /// *Why* it was rejected, because the two reasons want opposite handling and one
    /// flag for both put a permanent "Steam did not answer the refresh" on screen while
    /// the refresh was running perfectly (D-208).
    ///
    /// `"busy"` — a refresh is already running and will report for itself. Not an error,
    /// and the targets it is collecting belong to it.
    /// `"no-session"` — Steam is not there. A real failure; nothing else is coming.
    /// `"no-answer"` — Steam took the request and listed nothing (D-236). Nothing is coming.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
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

/// Workshop item metadata from `ISteamUGC` details query (M5 join plan). The join plan
/// reads it into its own type, so it needs no `Serialize` (D-257).
#[derive(Debug, Clone)]
pub struct ItemDetails {
    pub id: u64,
    pub title: String,
    pub file_size: u64,
    /// Printed by the live join test (`tests/live_join.rs`).
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
    /// The item `error` is about, when it is about one, so the message can name the
    /// mod (D-277).
    pub failed_id: Option<u64>,
    /// Replaced by a newer download, which is not a failure (D-277).
    pub superseded: bool,
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
        /// Subscribe to items that are not subscribed: a join needs them; an update
        /// from the Mods page must not bring back what was unsubscribed (D-276).
        subscribe: bool,
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

impl Cmd {
    /// What a command is, for the line that says which one opened a session (D-275).
    fn name(&self) -> &'static str {
        match self {
            Cmd::Refresh(_) => "a server list refresh",
            Cmd::Sync { .. } => "a Workshop download",
            Cmd::StaleItems { .. } => "the Workshop update check",
            Cmd::ItemDetails { .. } => "a Workshop details query",
            Cmd::Unsubscribe { .. } => "an unsubscribe",
            Cmd::Friends { .. } => "the friends list",
            Cmd::FriendAvatar { .. } => "a friend's avatar",
            Cmd::Shutdown => "shutdown",
        }
    }
}

/// A friend's 32×32 Steam avatar as raw RGBA. It crosses IPC as bytes rather than
/// JSON decimals (D-181), so it is never serialised and carries no dimensions - the
/// WebView knows the size it asked for and draws it straight onto a canvas.
#[derive(Debug, Clone)]
pub struct Avatar {
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
/// How long an item Steam calls installed may go without a folder after its kick
/// before the sync gives up on it with the remedy (Q30, D-265).
const MISSING_FOLDER_GRACE: Duration = Duration::from_secs(60);
/// A download is given up when Steam has moved nothing for this long. It used to be 45
/// minutes from the start whatever was happening, and a first join to a big modded
/// server — the project's own twelve-mod set is 5.91 GB (D-120) — needs 17.5 Mbit/s to
/// finish in that, so a slower line failed a healthy download while the dialog's
/// estimate said an hour to go (D-240).
const SYNC_STALL: Duration = Duration::from_secs(15 * 60);
/// …and a ceiling that still ends a download Steam keeps trickling forever.
const SYNC_CAP: Duration = Duration::from_secs(8 * 3600);
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
    /// One unsubscribe at a time. The worker answers an active request "superseded"
    /// when a second arrives, although Steam already has the first: a quick second
    /// click on the Mods page reported the first mod as failed while Steam went ahead
    /// and removed it (D-239).
    unsubscribing: Arc<Mutex<()>>,
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
            unsubscribing: Arc::new(Mutex::new(())),
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
            unsubscribing: Arc::clone(&self.unsubscribing),
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
            // A refresh "in the future" means the clock was set back since. Clamped to
            // "0 s ago", it held every automatic refresh back until the clock caught up
            // again — hours, for a clock that had been wrong by hours (D-236).
            .and_then(|t| u64::try_from(ServerRow::now_unix() - t).ok());
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
    pub fn sync(&self, job: u64, ids: Vec<u64>, subscribe: bool) -> Result<(), String> {
        let s = self.status();
        if !s.initialized {
            return Err(s.error.unwrap_or_else(|| "Steam is not initialised".into()));
        }
        if ids.is_empty() {
            return Err("nothing to sync".into());
        }
        self.cmd
            .send(Cmd::Sync {
                job,
                ids,
                subscribe,
            })
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
        let _one_at_a_time = self.unsubscribing.lock().unwrap_or_else(|e| e.into_inner());
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
        // The one wrapper without this check, and the worker keeps a dead session after
        // Steam closes (D-190): the client answered from it, a likely "nothing is stale"
        // replaced the `.acf` view and cleared the update badges while Steam was
        // closed (D-236).
        if !self.status().initialized {
            return None;
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
    /// write-ahead log (D-190).
    ///
    /// The thread reads the command between Steam calls, so this is quick — but one
    /// stuck inside `SteamAPI_Init`, a Steam call or `SteamAPI_Shutdown` against a hung
    /// Steam kept a windowless launcher alive for good. Three seconds, then the exit
    /// goes on without it (D-275). Safe to call twice: the second call finds the handle
    /// gone.
    pub fn shutdown(&self) {
        if !self.owner {
            return;
        }
        let _ = self.cmd.send(Cmd::Shutdown);
        let handle = self.join.lock().ok().and_then(|mut h| h.take());
        if let Some(h) = handle {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !h.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if h.is_finished() {
                let _ = h.join();
            } else {
                crate::log_warn!("steam", "the Steam thread did not stop within 3 s");
            }
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
    /// A join (true) subscribes to what it needs; an update (false) only refreshes what
    /// is still subscribed (D-276).
    subscribe: bool,
    started: Instant,
    /// When the bytes downloaded or the items installed last changed, and what they were.
    progressed: Instant,
    progress_mark: (u64, usize),
    last_emit: Instant,
    /// Subscribe results arrive through Steam call-result callbacks on this thread.
    sub_results: mpsc::Receiver<(u64, Result<(), String>)>,
    failed: HashMap<u64, String>,
    /// Download errors Steam reported per item; one can be transient, two are not.
    download_errors: HashMap<u64, u8>,
    kicked: HashMap<u64, Instant>,
}

fn start_sync(ugc: &UGC, job: u64, mut ids: Vec<u64>, subscribe: bool) -> ActiveSync {
    let (tx, rx) = mpsc::channel();
    // The Mods page offers Update from Steam's last answer or from the Workshop file,
    // either of which can be older than an unsubscribe; subscribing here brought the
    // mod back. An update leaves it out instead: `DownloadItem` fetches unsubscribed
    // items too, so kicking it would still download it (S-94, D-276).
    if !subscribe {
        ids.retain(|&id| {
            let kept = ugc
                .item_state(PublishedFileId(id))
                .contains(ItemState::SUBSCRIBED);
            if !kept {
                crate::log_info!("mods", "update skipped {id}: not subscribed in Steam");
            }
            kept
        });
    }
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
        subscribe,
        started: Instant::now(),
        progressed: Instant::now(),
        progress_mark: (0, 0),
        last_emit: Instant::now() - SYNC_EMIT_INTERVAL,
        sub_results: rx,
        failed: HashMap::new(),
        download_errors: HashMap::new(),
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
        // Steam's record, not the disk: an item whose folder was deleted behind
        // Steam's back stays INSTALLED in its books, the join plan (which looks at
        // the disk) sends it here, and "installed" finished the sync at once, so the
        // launch failed with "sync mods first" and the next Join did it again. It
        // stays pending; a minute after its kick with still no folder, `tick_sync` fails
        // it with the remedy rather than letting it wait out the 15-minute stall (S-79,
        // Q30, D-265).
        if info
            .as_ref()
            .is_some_and(|i| std::path::Path::new(&i.folder).is_dir())
        {
            "installed"
        } else {
            "pending"
        }
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
    download_errors: &mpsc::Receiver<(u64, String)>,
    events: &UnboundedSender<SteamEvent>,
) -> Option<SyncDone> {
    // A single failed attempt may be transient — the item is re-kicked every five
    // seconds — so an item fails on Steam's second error report for it (D-242).
    while let Ok((id, e)) = download_errors.try_recv() {
        if !sync.ids.contains(&id) {
            continue;
        }
        let n = sync.download_errors.entry(id).or_insert(0);
        *n = n.saturating_add(1);
        if *n >= 2 {
            sync.failed
                .insert(id, format!("Steam could not download it ({e})"));
        }
    }
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
    // An item Steam lists as installed whose folder is gone (D-245) had only the
    // start's kick and no second error report to fail on, so it sat on "Queued" for the
    // whole 15-minute stall and then failed without saying what fixes it (Q30). A
    // minute after its last kick, if Steam still calls it installed and there is still
    // no folder, Steam is not going to fetch it: it fails now, with the remedy (S-79,
    // D-265).
    for &id in &sync.ids {
        if sync.failed.contains_key(&id) {
            continue;
        }
        let st = ugc.item_state(PublishedFileId(id));
        let installed_in_steam = st.contains(ItemState::INSTALLED)
            && !st.intersects(ItemState::DOWNLOADING | ItemState::DOWNLOAD_PENDING);
        if !installed_in_steam {
            continue;
        }
        let folder_there = ugc
            .item_install_info(PublishedFileId(id))
            .is_some_and(|i| std::path::Path::new(&i.folder).is_dir());
        let since = sync.kicked.get(&id).copied().unwrap_or(sync.started);
        if !folder_there && since.elapsed() >= MISSING_FOLDER_GRACE {
            sync.failed.insert(
                id,
                "Steam lists it as installed but its folder is gone. Unsubscribe it on the \
                 Mods page (or in Steam), then join again to download it afresh"
                    .into(),
            );
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
    let downloaded: u64 = items.iter().map(|p| p.downloaded).sum();
    if (downloaded, installed) != sync.progress_mark {
        sync.progress_mark = (downloaded, installed);
        sync.progressed = Instant::now();
    }
    let elapsed_ms = sync.started.elapsed().as_millis() as u64;
    let finished = installed == items.len();
    // The first failure in the order the items were asked for, not whichever the map
    // returned first, and with its id so the message can name the mod (D-277).
    let first_failed = sync
        .ids
        .iter()
        .find_map(|id| sync.failed.get(id).map(|e| (*id, e.clone())));
    let failed = first_failed.is_some();
    let stalled = sync.progressed.elapsed() > SYNC_STALL;
    let capped = sync.started.elapsed() > SYNC_CAP;
    if finished || failed || stalled || capped {
        return Some(SyncDone {
            job: sync.job,
            ok: finished,
            failed_id: first_failed
                .as_ref()
                .filter(|_| !finished)
                .map(|(id, _)| *id),
            superseded: false,
            error: if finished {
                None
            } else if let Some((_, e)) = first_failed {
                Some(e)
            } else if stalled {
                // What to do next, not only what happened (D-265).
                Some(format!(
                    "Steam made no progress on the download for {} minutes. Check Steam's Downloads page; if a mod stays stuck, unsubscribe it on the Mods page and join again",
                    SYNC_STALL.as_secs() / 60
                ))
            } else {
                Some(format!(
                    "Steam did not finish the download within {} hours",
                    SYNC_CAP.as_secs() / 3600
                ))
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
/// The check has to fail for this long before Steam is reported gone, so that a
/// momentary reconnect inside Steam does not tear a working session down. With the
/// check every ten seconds that is 20–30 s after Steam goes, not the 15–25 s D-192
/// gave; the session itself is kept until Steam is back (D-190, D-275).
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
                failed_id: None,
                superseded: false,
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
                complete: false,
                rejected: true,
                reason: Some("no-session"),
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
            // Until the next attempt, or a command, whichever comes first: a 250 ms
            // poll woke the thread four times a second for nothing (D-275).
            let wait = next_try
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(10));
            match rx.recv_timeout(wait) {
                Ok(Cmd::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Ok(cmd) => reject(cmd, &events, "Steam is not running".into()),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
    // Which `steam.exe` the session talks to. A Steam that restarts inside the
    // liveness grace (a self-update does) was never noticed, and the session kept
    // talking to a process that no longer existed (D-236).
    let mut session_pid = publish_open(
        &shared,
        &events,
        session.as_ref().expect("session just opened"),
    );
    // Opens, releases and failed re-opens were never logged, so whether the idle
    // release (D-077) worked could not be read from the Logs page (D-158, D-275).
    crate::log_info!("steam", "session opened");

    let env_idle = idle_timeout();
    let mut last_activity = Instant::now();
    let mut last_liveness = Instant::now();
    // Since when the liveness check has been failing.
    let mut lost_since: Option<Instant> = None;
    let mut active: Option<ActiveRefresh> = None;
    let mut sync: Option<ActiveSync> = None;
    let mut unsub: Option<ActiveUnsubscribe> = None;
    // Timed-out server-list queries Steam had not finished with (D-242). Only valid
    // while their session is: every place that drops the session drops these first,
    // unreleased, because releasing one after `SteamAPI_Shutdown` would call into a
    // client that no longer exists.
    let mut unreleased: Vec<Arc<Mutex<ServerListRequest>>> = Vec::new();
    // A command that woke the thread from its no-session wait at the bottom of the loop.
    let mut woke_by: Option<Cmd> = None;

    loop {
        if let Some(s) = &session {
            s.client.run_callbacks();
        }

        loop {
            let cmd = match woke_by.take() {
                Some(cmd) => cmd,
                None => match rx.try_recv() {
                    Ok(cmd) => cmd,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                },
            };
            // Presence reads (friends list, avatar) and the Workshop's own update
            // flags come from Steam's local cache and must not keep an otherwise idle
            // session alive (D-077, D-103); every other command, and a session
            // re-opened for any command, counts.
            //
            // StaleItems was missing, and the Workshop poll runs every 15 minutes -
            // exactly the default `steam_idle_minutes`. So the release lasted about
            // five seconds per cycle: Steam went on showing the user in DayZ and
            // counting playtime all session, with a Shutdown/Init pair every quarter
            // hour, which is the churn D-190 blamed for the 0xC0000409 exit (D-220).
            let idle_safe = matches!(
                cmd,
                Cmd::Friends { .. } | Cmd::FriendAvatar { .. } | Cmd::StaleItems { .. }
            );
            if !idle_safe {
                last_activity = Instant::now();
            }
            // A released session answers the Workshop poll with `None`, which
            // `mods_stale` already reads as "could not be asked" rather than "nothing
            // is stale" - so there is nothing worth re-opening a session for. Nor for
            // an avatar: the Friends page kept asking for them past the release, and
            // "Show offline" re-opened the session for the first one it had not
            // fetched, with Steam back to "Playing DayZ" for the whole idle period.
            // The page asks again once the session is back (D-165, D-275).
            if session.is_none() && matches!(cmd, Cmd::StaleItems { .. } | Cmd::FriendAvatar { .. })
            {
                reject(cmd, &events, "Steam session released while idle".into());
                continue;
            }
            if session.is_none() && !matches!(cmd, Cmd::Shutdown) {
                match open_session() {
                    Ok(s) => {
                        session_pid = publish_open(&shared, &events, &s);
                        session = Some(s);
                        last_activity = Instant::now();
                        lost_since = None;
                        crate::log_info!("steam", "session re-opened for {}", cmd.name());
                    }
                    Err(e) => {
                        crate::log_warn!(
                            "steam",
                            "session did not re-open for {}: {e}",
                            cmd.name()
                        );
                        // A failed re-open left `initialized` true, so the UI kept
                        // reporting a connection that no longer existed (D-160); and
                        // `idle` true beside it read as "released while idle,
                        // reconnects on use" under the error (D-275).
                        shared.set_status(&events, |st| {
                            st.initialized = false;
                            st.idle = false;
                            st.error = Some(e.clone());
                        });
                        reject(cmd, &events, e);
                        continue;
                    }
                }
            }
            let Some(s) = session.as_ref() else { return }; // only reachable for Shutdown
            let ugc = &s.ugc;
            match cmd {
                Cmd::Sync {
                    job,
                    ids,
                    subscribe,
                } => {
                    if let Some(old) = sync.take() {
                        let _ = events.send(SteamEvent::SyncDone(SyncDone {
                            job: old.job,
                            ok: false,
                            error: Some("superseded by a newer sync".into()),
                            failed_id: None,
                            superseded: true,
                            items: Vec::new(),
                            elapsed_ms: old.started.elapsed().as_millis() as u64,
                        }));
                    }
                    sync = Some(start_sync(ugc, job, ids, subscribe));
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
                    // An item unsubscribed while a download holds it was re-kicked every
                    // five seconds, which Steam serves for unsubscribed items too (S-94),
                    // and then reported as downloaded. An update drops it; a join needs
                    // it, so the join stops with the reason instead of launching without
                    // it (D-276).
                    if let Some(s) = sync.as_mut() {
                        if s.subscribe {
                            for id in ids.iter().filter(|id| s.ids.contains(id)) {
                                s.failed.insert(
                                    *id,
                                    "It was unsubscribed while it downloaded; join again to download it".into(),
                                );
                            }
                        } else {
                            s.ids.retain(|id| !ids.contains(id));
                            for id in &ids {
                                s.kicked.remove(id);
                                s.failed.remove(id);
                                s.download_errors.remove(id);
                            }
                        }
                    }
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
                        .map(|rgba| Avatar { rgba });
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
                        complete: false,
                        rejected: true,
                        reason: Some("busy"),
                        source: "steam",
                    }));
                }
                // Not busy here: the arm above answers every refresh that arrives
                // while one is running. Partitions start below, on the same session.
                Cmd::Refresh(mut parts) => {
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
        // Not while Steam is reported gone: the liveness check below only runs while a
        // session exists, so releasing a dead one here was the end of it — when Steam
        // came back nothing noticed, every command wrapper refused to send while
        // `initialized` was false, and only a launcher restart recovered. The dead
        // session is kept until the "back" branch drops it, where D-194 showed that is
        // safe (D-236).
        let steam_present = shared
            .status
            .lock()
            .map(|st| st.initialized)
            .unwrap_or(false);
        if busy {
            last_activity = Instant::now();
        } else if session.is_some()
            && steam_present
            // Nor while the liveness check has started failing and not yet reported
            // it: releasing then ran `SteamAPI_Shutdown` against a Steam that had just
            // died — the path D-190 keeps the session for — and the loss was never
            // reported, because the check stops with the session (D-275).
            && lost_since.is_none()
            && idle_after.is_some_and(|d| last_activity.elapsed() >= d)
        {
            // Drops Client/MatchmakingServers/UGC → SteamAPI_Shutdown; frees the
            // Steam client heaps (Q16). Any later command re-opens the session.
            unreleased.clear();
            session = None;
            shared.set_status(&events, |st| st.idle = true);
            crate::log_info!(
                "steam",
                "session released after {} min without use",
                last_activity.elapsed().as_secs() / 60
            );
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
            let steam = crate::steam::registry::detect();
            let alive = steam.running;
            if alive && session_pid == 0 {
                session_pid = steam.pid;
            }
            let reported_gone = !shared
                .status
                .lock()
                .map(|st| st.initialized)
                .unwrap_or(false);
            // The first `steam.exe` found is not necessarily the client: a `steam://`
            // link runs a second copy for a moment. Only a session whose own process is
            // gone has lost its Steam.
            let restarted = alive
                && steam.pid != session_pid
                && !crate::steam::registry::process::steam_pids(steam.path.as_deref())
                    .contains(&session_pid);
            if restarted && !reported_gone {
                // Another `steam.exe` than the one the session opened against: Steam
                // restarted between two checks — a self-update takes a few seconds — so
                // the grace below never saw it go, and the session went on talking to a
                // process that no longer exists. Every refresh then spun for the full
                // partition timeout (D-194). Handled like the "back" branch: Steam has
                // just started, so dropping the session is safe, and the next command
                // re-opens it (D-236).
                crate::log_info!(
                    "steam",
                    "Steam restarted (pid {session_pid} → {}); the session will re-open",
                    steam.pid
                );
                abandon_in_flight(
                    &mut active,
                    &mut sync,
                    &mut unsub,
                    &events,
                    "Steam restarted",
                );
                unreleased.clear();
                session = None;
                session_pid = 0;
                lost_since = None;
                // Released unless the launcher was in use within the idle period: with
                // `idle` false the title bar's friends poll re-opened the session within
                // the minute, and Steam showed "Playing DayZ" for a whole idle period
                // nobody asked for. The no-session branch below already did this (D-236);
                // these two branches predated it (D-275).
                let released = idle_after.is_some_and(|d| last_activity.elapsed() >= d);
                shared.set_status(&events, |st| {
                    st.initialized = true;
                    st.idle = released;
                    st.refreshing = false;
                    st.error = None;
                });
            } else {
                match (alive, lost_since) {
                    (true, None) => {}
                    (true, Some(t)) => {
                        // It came back inside the grace period, or after we reported it gone.
                        let was_reported_gone = t.elapsed() >= LIVENESS_GRACE && reported_gone;
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
                            //
                            // Answered first, as in the "restarted" branch above: a
                            // refresh queued before the loss was reported still started on
                            // the dead session, and dropping the session under it left
                            // `refreshing` stuck, so every later refresh was declined, and
                            // left its request pointing into the session just shut down
                            // (D-256). With nothing in flight this does nothing.
                            abandon_in_flight(
                                &mut active,
                                &mut sync,
                                &mut unsub,
                                &events,
                                "Steam restarted",
                            );
                            unreleased.clear();
                            session = None;
                            session_pid = 0;
                            crate::log_info!("steam", "Steam is back; the session will re-open");
                            // As in the "restarted" branch above (D-275).
                            let released = idle_after.is_some_and(|d| last_activity.elapsed() >= d);
                            shared.set_status(&events, |st| {
                                st.initialized = true;
                                st.idle = released;
                                st.refreshing = false;
                                st.error = None;
                            });
                        }
                    }
                    (false, None) => lost_since = Some(Instant::now()),
                    (false, Some(t)) if t.elapsed() >= LIVENESS_GRACE => {
                        // Report it once, not every interval.
                        if !reported_gone {
                            crate::log_warn!("steam", "Steam is no longer running");
                            // Everything in flight belonged to a Steam that is gone.
                            abandon_in_flight(
                                &mut active,
                                &mut sync,
                                &mut unsub,
                                &events,
                                "Steam closed",
                            );
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
        }

        // No session, and Steam reported unavailable — a re-open that failed, for
        // instance a Refresh after Steam was closed while the session was released.
        // Nothing else could ever clear that: the check above needs a session, and
        // every command wrapper refuses to send while `initialized` is false, so the
        // on-demand re-open below was unreachable and the launcher said "Steam is not
        // running" with Steam up until it was restarted (D-236). Like the start-up
        // retry (D-125), but it only marks Steam available: the session re-opens on
        // use, so a released session stays released (D-077).
        //
        // The same probe notices Steam closing while the session is released. Nothing
        // did: the check above needs a session, so the status kept "connected,
        // released", the title bar kept its friends in DayZ and the rows their friend
        // markers, and no "Steam is not running" notice appeared until something failed
        // to re-open — the values D-222 keeps because "that session comes back", which
        // it cannot with Steam gone (D-275).
        if session.is_none() && last_liveness.elapsed() >= STEAM_RETRY {
            last_liveness = Instant::now();
            let available = shared
                .status
                .lock()
                .map(|st| st.initialized)
                .unwrap_or(false);
            let running = crate::steam::registry::detect().running;
            if !available && running {
                lost_since = None;
                crate::log_info!(
                    "steam",
                    "Steam is running again; the session re-opens on use"
                );
                shared.set_status(&events, |st| {
                    st.initialized = true;
                    st.idle = true;
                    st.error = None;
                });
            } else if available && !running {
                match lost_since {
                    None => lost_since = Some(Instant::now()),
                    Some(t) if t.elapsed() >= LIVENESS_GRACE => {
                        lost_since = None;
                        crate::log_warn!("steam", "Steam is no longer running");
                        shared.set_status(&events, |st| {
                            st.initialized = false;
                            st.idle = false;
                            st.refreshing = false;
                            st.error = Some("Steam is no longer running".into());
                        });
                    }
                    Some(_) => {}
                }
            } else if running {
                lost_since = None;
            }
        }

        let Some(s) = session.as_ref() else {
            // Nothing to pump without a session: wait for a command or the next check.
            // A 100 ms sleep woke the thread ten times a second for the whole release,
            // 36 000 times an hour, to find nothing to do (docs/05 §6, D-275).
            let wait = STEAM_RETRY
                .saturating_sub(last_liveness.elapsed())
                .max(Duration::from_millis(10));
            match rx.recv_timeout(wait) {
                Ok(cmd) => woke_by = Some(cmd),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            continue;
        };
        let (mms, ugc) = (&s.mms, &s.ugc);
        unreleased.retain(|q| {
            q.lock()
                .map(|mut q| matches!(q.release(), Err(ReleaseError::Refreshing)))
                .unwrap_or(false)
        });

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
                        let total: usize = r.results.iter().map(|p| p.total).sum();
                        let capped = r.results.iter().any(|p| p.capped);
                        // A list refresh that listed nothing was never answered: the
                        // partition timed out, or the master said
                        // `NoServersListedOnMasterServer`, its throttling answer (D-046).
                        // It used to report as a complete, successful refresh — the
                        // throttle and `last_refresh` were armed, every Steam vouch was
                        // withdrawn (D-233), and "0 from Steam" read as a result (D-236).
                        let unanswered = !lan_only && total == 0;
                        let complete = !unanswered && vouch_complete(&r.results, lan_only);
                        let done = RefreshDone {
                            source: if lan_only { "lan" } else { "steam" },
                            total,
                            responded: r.results.iter().map(|p| p.responded).sum(),
                            failed: r.results.iter().map(|p| p.failed).sum(),
                            inflated: r.results.iter().map(|p| p.inflated).sum(),
                            elapsed_ms: r.started.elapsed().as_millis() as u64,
                            capped,
                            stopped_early: r.stopped_early,
                            complete,
                            rejected: unanswered,
                            reason: unanswered.then_some("no-answer"),
                            partitions: std::mem::take(&mut r.results),
                        };
                        let _ = events.send(SteamEvent::Done(done));
                        // A LAN scan is not a list refresh and must not hold the next
                        // one back (D-160); an unanswered one fetched nothing to protect.
                        if !lan_only && !unanswered {
                            *shared.last_done.lock().unwrap_or_else(|e| e.into_inner()) =
                                Some(ServerRow::now_unix());
                        }
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
                        // A query Steam is still refreshing refuses `release`, and that is
                        // exactly the timeout case: the error was dropped, so the request
                        // kept pinging beside the next partition and was never freed. It
                        // is kept and released once Steam lets go of it (D-242).
                        let refused = p
                            .req
                            .lock()
                            .map(|mut q| matches!(q.release(), Err(ReleaseError::Refreshing)))
                            .unwrap_or(false);
                        if refused {
                            unreleased.push(Arc::clone(&p.req));
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
                        let capped = total >= STEAM_LIST_CAP;
                        if capped {
                            crate::log_info!(
                                "steam",
                                "partition {:?} hit Steam's {STEAM_LIST_CAP}-row cap",
                                p.filters
                            );
                            // One server per address for the same map, next (D-245).
                            if let Some(f) = collapsed(&p.filters) {
                                r.pending.push(f);
                                r.next_allowed = Instant::now() + PARTITION_GAP;
                            }
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
                            capped,
                        });
                    }
                }
            }
        }

        if let Some(job) = sync.as_mut() {
            if let Some(done) = tick_sync(ugc, job, &s.download_errors, &events) {
                let _ = events.send(SteamEvent::SyncDone(done));
                sync = None;
            }
        } else {
            // Results for downloads nobody is waiting on (Steam's own updates) must not
            // be counted against the next job.
            while s.download_errors.try_recv().is_ok() {}
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
/// `connect 1.2.3.4`. The first IPv4 wins; a port after a colon, or the number after a
/// `port` key on either side of the address, goes with it; no port means DayZ's
/// default game port 2302. Port 0 is no port: `1.2.3.4:0` joined `ip:0`, and a
/// `-port=2402` written before `-connect` fell back to 2302, which at a shared address
/// can be another server (D-275).
fn parse_connect(text: &str) -> Option<(std::net::Ipv4Addr, u16)> {
    let mut ip: Option<std::net::Ipv4Addr> = None;
    let mut port: Option<u16> = None;
    let mut early_port: Option<u16> = None;
    let mut prev_was_port = false;
    for tok in text.split(|c: char| c.is_whitespace() || c == '=' || c == ',' || c == ';') {
        let t = tok.trim_matches(|c| c == '+' || c == '-' || c == '"' || c == '\'');
        if ip.is_none() {
            if let Some((a, p)) = t.rsplit_once(':') {
                if let Ok(a) = a.parse() {
                    ip = Some(a);
                    port = p.parse().ok().filter(|p: &u16| *p != 0);
                    continue;
                }
            }
            if let Ok(a) = t.parse() {
                ip = Some(a);
            } else if prev_was_port {
                early_port = t.parse::<u16>().ok().filter(|p| *p != 0);
            }
        } else if port.is_none() && prev_was_port {
            // Only the number that follows a `port` key: any other bare number in the
            // string — `-cpuCount=8` — used to be taken as the port (D-245).
            if let Ok(p) = t.parse::<u16>() {
                if p != 0 {
                    port = Some(p);
                }
            }
        }
        prev_was_port = t.eq_ignore_ascii_case("port");
    }
    ip.filter(|a| !a.is_unspecified())
        .map(|a| (a, port.or(early_port).unwrap_or(2302)))
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

/// Whether a finished refresh may withdraw the vouch from servers it did not list: on the
/// `hasplayers` partition's own answer, finished and uncapped (D-236 (3)). It used to
/// need every partition uncapped and nothing stopped early, and since D-245 three map
/// partitions of the empty list cap at Steam's 10 000 on every full refresh, so only the
/// populated-only start refresh ever withdrew anything; a throttled empty partition
/// blocked it the same way though the populated answer was whole (D-271).
fn vouch_complete(results: &[PartitionResult], lan_only: bool) -> bool {
    let mut populated = results
        .iter()
        .filter(|p| p.filters.contains_key("hasplayers"))
        .peekable();
    !lan_only
        && populated.peek().is_some()
        && populated.all(|p| p.total > 0 && !p.capped && p.response != "NoAnswer")
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
        // The catch-all empty partition asks for one server per address (S-83).
        assert!(p.last().unwrap().contains_key(COLLAPSE_KEY));
    }

    #[test]
    fn a_capped_map_partition_does_not_keep_the_vouches() {
        let part = |key: &str, total: usize, capped: bool, response: &str| PartitionResult {
            filters: HashMap::from([(key.to_string(), String::new())]),
            total,
            responded: total,
            failed: 0,
            inflated: 0,
            elapsed_ms: 1,
            response: response.to_string(),
            capped,
        };
        let populated = part("hasplayers", 2_210, false, "ServerResponded");
        let capped_map = part("noplayers", STEAM_LIST_CAP, true, "ServerResponded");
        // Three empty map partitions at the cap no longer block a whole populated answer.
        assert!(vouch_complete(
            &[populated.clone(), capped_map.clone()],
            false
        ));
        // Stopped early after the populated partition answered: its answer is still whole.
        assert!(vouch_complete(std::slice::from_ref(&populated), false));
        // The populated answer itself must be whole, answered and non-empty.
        assert!(!vouch_complete(
            &[part("hasplayers", STEAM_LIST_CAP, true, "ServerResponded")],
            false
        ));
        assert!(!vouch_complete(
            &[part("hasplayers", 12, false, "NoAnswer")],
            false
        ));
        assert!(!vouch_complete(
            &[part("hasplayers", 0, false, "ServerResponded")],
            false
        ));
        // No populated partition, or a LAN scan: nothing to decide from.
        assert!(!vouch_complete(&[capped_map], false));
        assert!(!vouch_complete(&[populated], true));
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
        // Port 0 is no port, and a `-port` before the address still counts (D-275).
        assert_eq!(
            parse_connect("+connect 51.81.8.81:0"),
            Some((ip("51.81.8.81"), 2302))
        );
        assert_eq!(
            parse_connect("-port=2402 -connect=51.81.8.81"),
            Some((ip("51.81.8.81"), 2402))
        );
        // A port given both ways: the one that goes with the address wins.
        assert_eq!(
            parse_connect("-port=2402 +connect 51.81.8.81:2502"),
            Some((ip("51.81.8.81"), 2502))
        );
    }
}
