//! SQLite cache of the last known server list so the UI renders instantly on
//! start (docs/05 §3, budget "cold start to first painted list < 1 s").
//! rusqlite 0.40 with the bundled SQLite; WAL journal; one connection behind a mutex.
//! Pre-release schema policy: a version mismatch drops and recreates the table.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::a2s::DayzTags;

use super::verify::Verification;
use super::ServerRow;

const SCHEMA_VERSION: &str = "4";

/// User data tables are never dropped by a schema bump; only `servers` is.
const USER_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS favourites (
  id TEXT PRIMARY KEY,
  added_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS history (
  id TEXT NOT NULL,
  joined_at INTEGER NOT NULL,
  name TEXT NOT NULL,
  ip TEXT NOT NULL,
  game_port INTEGER NOT NULL,
  mods INTEGER NOT NULL,
  PRIMARY KEY (id, joined_at)
);
CREATE TABLE IF NOT EXISTS population (
  id TEXT NOT NULL,
  ts INTEGER NOT NULL,
  players INTEGER NOT NULL,
  queue INTEGER NOT NULL,
  PRIMARY KEY (id, ts)
);
CREATE INDEX IF NOT EXISTS population_ts ON population(ts);
";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS servers (
  id TEXT PRIMARY KEY,
  ip TEXT NOT NULL,
  game_port INTEGER NOT NULL,
  query_port INTEGER NOT NULL,
  name TEXT NOT NULL,
  map TEXT NOT NULL,
  description TEXT NOT NULL,
  players INTEGER NOT NULL,
  max_players INTEGER NOT NULL,
  bots INTEGER NOT NULL,
  password INTEGER NOT NULL,
  secure INTEGER NOT NULL,
  server_version INTEGER NOT NULL,
  ping_ms INTEGER NOT NULL,
  keywords TEXT NOT NULL,
  steam_id INTEGER NOT NULL,
  last_seen INTEGER NOT NULL,
  verified_players INTEGER,
  steam_empty INTEGER,
  verified_at INTEGER,
  verdict TEXT
);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS server_mods (
  server_id TEXT NOT NULL,
  mod_id INTEGER NOT NULL,
  name TEXT NOT NULL,
  PRIMARY KEY (server_id, mod_id)
);
-- Covering: the catalogue query is `SELECT mod_id, MAX(name), COUNT(*) … GROUP BY
-- mod_id`, so without `name` in the index every one of ~127 000 entries falls back
-- to a table lookup — measured 247 ms against 17 ms (D-175).
CREATE INDEX IF NOT EXISTS server_mods_mod ON server_mods(mod_id, name);
CREATE TABLE IF NOT EXISTS server_mods_at (
  server_id TEXT PRIMARY KEY,
  scanned_at INTEGER NOT NULL,
  mod_count INTEGER NOT NULL
);
";

const SELECT_COLUMNS: &str = "id, ip, game_port, query_port, name, map, description, players, max_players, bots, password, secure,
                    server_version, ping_ms, keywords, steam_id, last_seen, verified_players, steam_empty, verified_at, verdict";

/// Built once: `format!` per call measured a third of the whole lookup (D-175).
static GET_SQL: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| format!("SELECT {SELECT_COLUMNS} FROM servers WHERE id = ?1"));

/// What `row_counts` reports (D-193).
#[derive(Debug, Clone, Copy)]
pub struct RowCounts {
    pub servers: i64,
    pub favourites: i64,
    pub history: i64,
    pub population: i64,
}

pub struct Cache {
    conn: Connection,
}

/// One mod as seen across the scanned servers (D-080).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCatalogEntry {
    pub id: u64,
    /// `mod.cpp` name as the servers report it (names are not unique).
    pub name: String,
    pub servers: usize,
}

/// Mod ids of one scanned server.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerMods {
    pub id: String,
    pub mods: Vec<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModsIndex {
    pub catalog: Vec<ModCatalogEntry>,
    pub index: Vec<ServerMods>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub joined_at: i64,
    pub name: String,
    pub ip: String,
    pub game_port: u16,
    pub mods: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PopulationSample {
    pub ts: i64,
    pub players: i32,
    pub queue: i32,
}

/// A scanned mod list with the time it was scanned: `(scanned_at, [(workshop id, name)])`.
pub type ScannedMods = (i64, Vec<(u64, String)>);
impl Cache {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY;",
        )?;
        Self::migrate(conn)
    }

    /// A cache that exists only for this run. The tests use it, and so does start-up
    /// when the real file cannot be opened *or* moved aside: browsing still works
    /// because the list comes from Steam, and refusing to start instead loses the
    /// session for a lock that may clear in a minute (D-194).
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::migrate(Connection::open_in_memory()?)
    }

    fn migrate(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        let version: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if version.as_deref() != Some(SCHEMA_VERSION) {
            // The mod tables key off server ids, so they go with it: nothing else
            // sweeps orphans any more (D-175).
            conn.execute_batch(
                "DROP TABLE IF EXISTS servers;
                 DROP TABLE IF EXISTS server_mods;
                 DROP TABLE IF EXISTS server_mods_at;",
            )?;
            conn.execute_batch(SCHEMA)?;
            conn.execute(
                "INSERT INTO meta (key, value) VALUES ('schema_version', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![SCHEMA_VERSION],
            )?;
        } else {
            conn.execute_batch(SCHEMA)?;
        }
        // Index changes on a database that already exists (D-175): the old
        // `server_mods_mod` did not cover `name`, and `servers_last_seen` cost ~20 ms
        // per refresh to save 0.8 ms on the one query that used it.
        let mods_index_sql: Option<String> = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'server_mods_mod'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let needs_rebuild = !mods_index_sql
            .as_deref()
            .is_some_and(|sql| sql.contains("mod_id, name"));
        if needs_rebuild {
            conn.execute_batch(
                "DROP INDEX IF EXISTS server_mods_mod;
                 CREATE INDEX server_mods_mod ON server_mods(mod_id, name);",
            )?;
        }
        // Cheap whether or not it is there, and it is gone after the first run.
        conn.execute_batch("DROP INDEX IF EXISTS servers_last_seen;")?;
        conn.execute_batch(USER_SCHEMA)?;
        Ok(Self { conn })
    }

    // ----- favourites --------------------------------------------------------

    /// `(id, added_at)` newest first.
    pub fn favourites(&self) -> rusqlite::Result<Vec<(String, i64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, added_at FROM favourites ORDER BY added_at DESC")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// Marks many servers favourite in one transaction with one checkpoint. Per
    /// favourite, `favourite_set` costs a transaction *and* a WAL checkpoint: 50 of
    /// them measured 111.8 ms against 6.7 ms this way (D-175).
    pub fn favourites_set_many(&mut self, ids: &[String]) -> rusqlite::Result<()> {
        {
            let tx = self.conn.transaction()?;
            {
                let mut stmt = tx.prepare_cached(
                    "INSERT INTO favourites (id, added_at) VALUES (?1, ?2) ON CONFLICT(id) DO NOTHING",
                )?;
                let now = ServerRow::now_unix();
                for id in ids {
                    stmt.execute(params![id, now])?;
                }
            }
            tx.commit()?;
        }
        self.checkpoint_durable();
        Ok(())
    }

    pub fn favourite_set(&self, id: &str, on: bool) -> rusqlite::Result<()> {
        if on {
            self.conn.execute(
                "INSERT INTO favourites (id, added_at) VALUES (?1, ?2) ON CONFLICT(id) DO NOTHING",
                params![id, ServerRow::now_unix()],
            )?;
        } else {
            self.conn
                .execute("DELETE FROM favourites WHERE id = ?1", params![id])?;
        }
        // The one user table with no logging at all, and the one whose disappearance
        // started Q22 (D-194).
        crate::log_info!(
            "cache",
            "favourite {}: {id}",
            if on { "added" } else { "removed" }
        );
        self.checkpoint_durable();
        Ok(())
    }

    // ----- history -----------------------------------------------------------

    pub fn history_add(&self, row: &ServerRow, mods: usize) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO history (id, joined_at, name, ip, game_port, mods) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id, joined_at) DO NOTHING",
            params![row.id, ServerRow::now_unix(), row.name, row.ip, row.game_port as i64, mods as i64],
        )?;
        // A join is rare and precious: move it from the write-ahead log into the main
        // file at once, so a lost or truncated `-wal` cannot take it with it (Q22).
        self.checkpoint_durable();
        Ok(())
    }

    /// Empties the join history: the user's confirmed "Clear list" on the Recent view
    /// (D-130), the only code path that deletes history rows. Returns the number
    /// removed and checkpoints so the deletion is as durable as the inserts.
    pub fn history_clear(&self) -> rusqlite::Result<usize> {
        let n = self.conn.execute("DELETE FROM history", [])?;
        crate::log_info!("cache", "history cleared by the user: {n} row(s)");
        self.checkpoint_durable();
        Ok(n)
    }

    /// Folds the write-ahead log into the database file without blocking readers.
    ///
    /// A PASSIVE checkpoint does nothing while any reader holds the log, and it says
    /// so in its result row rather than by failing — `(busy, log_pages,
    /// checkpointed_pages)` — so the row has to be read. Q22 is an unexplained loss of
    /// committed rows, and "the checkpoint quietly copied nothing" is one of the few
    /// explanations left, so both outcomes are on the record (D-187).
    pub fn checkpoint(&self) -> bool {
        let result = self
            .conn
            .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            });
        match result {
            Ok((busy, log_pages, checkpointed)) => {
                if checkpointed < log_pages {
                    // `busy` was measured at 0 while nothing was copied, with a reader
                    // holding an older snapshot — so "blocked by a reader" cannot be
                    // conditioned on it, and a reader is the likely cause either way
                    // (D-194).
                    crate::log_warn!(
                        "cache",
                        "checkpoint copied {checkpointed} of {log_pages} page(s) (busy={busy}); the rest stays in the write-ahead log"
                    );
                }
                checkpointed >= log_pages
            }
            Err(e) => {
                crate::log_warn!("cache", "checkpoint failed: {e}");
                false
            }
        }
    }

    /// For the two writes whose durability the user would notice: a favourite and a
    /// join. A PASSIVE checkpoint can copy nothing at all — measured at
    /// `(busy 0, log 12, checkpointed 0)` with one reader on an older snapshot — and
    /// then the row exists only in the write-ahead log, which is exactly the state
    /// Q22 kept producing. FULL waits for the readers instead of stepping around
    /// them, bounded by the connection's busy timeout so a stuck reader costs a few
    /// seconds and a log line rather than the thread (D-194).
    /// Checkpoints and then truncates the write-ahead log, for the way out.
    ///
    /// Tauri leaves through `process::exit`, so the `Connection` is never dropped and
    /// SQLite never truncates. Nothing is at risk — the pages have been copied — but
    /// the file keeps its high-water mark: 6.34 MB of stale bytes measured here on a
    /// machine with no process running, costing 0.71 ms of every cold start
    /// (`load_all` 5.93 ms with it against 5.22 without). TRUNCATE measured 20.8 ms,
    /// paid once, at exit, where nobody is waiting (D-223).
    pub fn checkpoint_truncate(&self) {
        self.checkpoint_durable();
        let _ = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                r.get::<_, i64>(0)
            });
    }

    pub fn checkpoint_durable(&self) {
        if self.checkpoint() {
            return;
        }
        // rusqlite's default busy timeout is five seconds, and every caller here holds
        // the one cache mutex that the server list, every Steam batch and every
        // verification publish also need — measured at 5.03 s with a reader on an
        // older snapshot, after which 402 of 907 pages were *still* in the log. So it
        // blocked the UI for five seconds and did not even get what it blocked for.
        // A quarter of a second and a log line is the honest trade (D-197).
        let _ = self
            .conn
            .busy_timeout(std::time::Duration::from_millis(250));
        match self.conn.query_row("PRAGMA wal_checkpoint(FULL)", [], |r| {
            Ok((r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
        }) {
            Ok((log_pages, checkpointed)) if checkpointed < log_pages => crate::log_warn!(
                "cache",
                "full checkpoint still left {} of {log_pages} page(s) in the write-ahead log",
                log_pages - checkpointed
            ),
            Ok(_) => {}
            Err(e) => crate::log_warn!("cache", "full checkpoint failed: {e}"),
        }
        let _ = self.conn.busy_timeout(std::time::Duration::from_secs(5));
    }

    pub fn history(&self, limit: usize) -> rusqlite::Result<Vec<HistoryEntry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, joined_at, name, ip, game_port, mods FROM history ORDER BY joined_at DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok(HistoryEntry {
                id: r.get(0)?,
                joined_at: r.get(1)?,
                name: r.get(2)?,
                ip: r.get(3)?,
                game_port: r.get::<_, i64>(4)? as u16,
                mods: r.get::<_, i64>(5)? as usize,
            })
        })?;
        rows.collect()
    }

    // ----- population samples ------------------------------------------------

    pub fn population_add(&mut self, samples: &[(String, i64, i32, i32)]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("INSERT INTO population (id, ts, players, queue) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(id, ts) DO NOTHING")?;
            for (id, ts, players, queue) in samples {
                stmt.execute(params![id, ts, players, queue])?;
            }
        }
        tx.commit()
    }

    pub fn population(&self, id: &str, since: i64) -> rusqlite::Result<Vec<PopulationSample>> {
        let mut stmt = self.conn.prepare(
            "SELECT ts, players, queue FROM population WHERE id = ?1 AND ts >= ?2 ORDER BY ts",
        )?;
        let rows = stmt.query_map(params![id, since], |r| {
            Ok(PopulationSample {
                ts: r.get(0)?,
                players: r.get(1)?,
                queue: r.get(2)?,
            })
        })?;
        rows.collect()
    }

    pub fn population_prune(&self, max_age_secs: i64) -> rusqlite::Result<usize> {
        self.conn.execute(
            "DELETE FROM population WHERE ts < ?1",
            params![ServerRow::now_unix() - max_age_secs],
        )
    }

    fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<ServerRow> {
        let keywords: String = r.get(14)?;
        let server_version: i32 = r.get(12)?;
        let ip: String = r.get(1)?;
        Ok(ServerRow {
            id: r.get(0)?,
            country: ServerRow::country_for(&ip),
            ip,
            game_port: r.get::<_, i64>(2)? as u16,
            query_port: r.get::<_, i64>(3)? as u16,
            name: r.get(4)?,
            map: r.get(5)?,
            description: r.get(6)?,
            players: r.get(7)?,
            max_players: r.get(8)?,
            bots: r.get(9)?,
            password: r.get::<_, i64>(10)? != 0,
            secure: r.get::<_, i64>(11)? != 0,
            server_version,
            version: ServerRow::version_string(server_version),
            ping_ms: r.get::<_, i64>(13)? as u32,
            tags: DayzTags::parse(&keywords),
            keywords,
            steam_id: r.get::<_, i64>(15)? as u64,
            last_seen: r.get(16)?,
            verified_players: r.get(17)?,
            steam_empty: r.get::<_, Option<i64>>(18)?.map(|v| v != 0),
            verified_at: r.get(19)?,
            verdict: r.get(20)?,
        })
    }

    /// Every cached row, unordered: the browser puts them in a map and sorts by the
    /// column the user picked, so sorting here only cost a temp b-tree (D-160).
    pub fn load_all(&self) -> rusqlite::Result<Vec<ServerRow>> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!("SELECT {SELECT_COLUMNS} FROM servers"))?;
        let rows = stmt.query_map([], Self::row_from)?;
        rows.collect()
    }

    /// Addresses a mod scan should query. Building a full `ServerRow` for all ~19 000
    /// cached servers to keep the 13 % that qualify measured 19 ms and ~7 MB of
    /// strings that were thrown away immediately (D-175); this reads the eight
    /// columns the rules actually use and joins the last scan time in one pass.
    ///
    /// The rules match the browser exactly: modded, not rule-R0 inflated, with real
    /// players (a verified head-count, or Steam's own `hasplayers` answer when it has
    /// not been verified yet), and not scanned inside `max_age_secs`.
    pub fn scan_targets(
        &self,
        force: bool,
        now: i64,
        max_age_secs: i64,
    ) -> rusqlite::Result<Vec<(String, std::net::SocketAddr)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.id, s.ip, s.query_port, s.keywords, s.players, s.verified_players,
                    s.steam_empty, a.scanned_at
             FROM servers s LEFT JOIN server_mods_at a ON a.server_id = s.id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u16>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i32>(4)?,
                r.get::<_, Option<i32>>(5)?,
                r.get::<_, Option<bool>>(6)?,
                r.get::<_, Option<i64>>(7)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, ip, port, keywords, players, verified, steam_empty, scanned_at) = row?;
            if !keywords.split(',').any(|t| t.trim() == "mod") {
                continue;
            }
            if steam_empty == Some(true) && players > 0 {
                continue; // rule R0: Steam says empty, INFO claims players
            }
            if !verified.map_or(steam_empty == Some(false) && players > 0, |v| v > 0) {
                continue;
            }
            if !force && scanned_at.is_some_and(|at| now - at <= max_age_secs) {
                continue;
            }
            let Ok(addr) = ip.parse::<std::net::IpAddr>() else {
                continue;
            };
            out.push((id, std::net::SocketAddr::new(addr, port)));
        }
        Ok(out)
    }

    pub fn get(&self, id: &str) -> rusqlite::Result<Option<ServerRow>> {
        // Called up to 120 times per on-demand verification, so the SQL is compiled
        // once rather than per row (D-160).
        let mut stmt = self.conn.prepare_cached(GET_SQL.as_str())?;
        stmt.query_row(params![id], Self::row_from).optional()
    }

    /// Insert-or-replace a batch inside one transaction. `None` values for the
    /// verification columns never erase stored ones.
    pub fn upsert(&mut self, rows: &[ServerRow]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO servers (id, ip, game_port, query_port, name, map, description, players, max_players, bots,
                                      password, secure, server_version, ping_ms, keywords, steam_id, last_seen,
                                      verified_players, steam_empty, verified_at, verdict)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
                 ON CONFLICT(id) DO UPDATE SET
                   ip=excluded.ip, game_port=excluded.game_port, query_port=excluded.query_port, name=excluded.name,
                   map=excluded.map, description=excluded.description, players=excluded.players,
                   max_players=excluded.max_players, bots=excluded.bots, password=excluded.password, secure=excluded.secure,
                   server_version=excluded.server_version, ping_ms=excluded.ping_ms, keywords=excluded.keywords,
                   steam_id=excluded.steam_id, last_seen=excluded.last_seen,
                   verified_players=COALESCE(excluded.verified_players, servers.verified_players),
                   steam_empty=COALESCE(excluded.steam_empty, servers.steam_empty),
                   verified_at=COALESCE(excluded.verified_at, servers.verified_at),
                   verdict=COALESCE(excluded.verdict, servers.verdict)",
            )?;
            for s in rows {
                stmt.execute(params![
                    s.id,
                    s.ip,
                    s.game_port as i64,
                    s.query_port as i64,
                    s.name,
                    s.map,
                    s.description,
                    s.players,
                    s.max_players,
                    s.bots,
                    s.password as i64,
                    s.secure as i64,
                    s.server_version,
                    s.ping_ms as i64,
                    s.keywords,
                    s.steam_id as i64,
                    s.last_seen,
                    s.verified_players,
                    s.steam_empty.map(i64::from),
                    s.verified_at,
                    s.verdict,
                ])?;
            }
        }
        tx.commit()
    }

    /// Applies verification results: verdict columns plus the fresh INFO numbers when present.
    pub fn apply_verifications(&mut self, results: &[Verification]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            // A check that could not count keeps the last count it did (D-160): a single
            // dropped PLAYER datagram used to write NULL over a good head-count, after
            // which the UI fell back to the server's own — possibly inflated — number
            // with no marking at all. The verdict still records why it could not be
            // refreshed, and `verified_at` only advances when there is a fresh count.
            let mut stmt = tx.prepare_cached(
                "UPDATE servers SET
                   verified_players = COALESCE(?2, verified_players),
                   verified_at = CASE WHEN ?2 IS NULL THEN verified_at ELSE ?3 END,
                   verdict = ?4,
                   players = ?5, max_players = ?6,
                   ping_ms = COALESCE(?7, ping_ms), keywords = COALESCE(?8, keywords),
                   last_seen = CASE WHEN ?7 IS NULL THEN last_seen ELSE ?3 END
                 WHERE id = ?1",
            )?;
            for v in results {
                stmt.execute(params![
                    v.id,
                    v.verified,
                    v.verified_at,
                    v.verdict.as_str(),
                    v.reported,
                    v.max_players,
                    v.ping_ms.map(i64::from),
                    v.keywords,
                ])?;
            }
        }
        tx.commit()
    }

    /// Withdraws Steam's vouch from every server the populated partition did not
    /// return this time.
    ///
    /// `steam_empty = 0` meant "Steam listed this server as having players" *at some*
    /// refresh, and nothing ever withdrew it: the automatic refresh asks for
    /// `hasplayers` only, so a server that dropped out of that list simply kept its
    /// vouch, for up to the 30-day prune. 2 669 vouched rows in the live cache were
    /// absent from the latest refresh, 19 of them among the 27 rows the vouch was
    /// rescuing from an "unverifiable" verdict. A vouch is now good for one refresh;
    /// a server Steam does not list this time goes back to unknown (D-233).
    pub fn unvouch_unseen(&self, refresh_started: i64) -> rusqlite::Result<usize> {
        self.conn.execute(
            "UPDATE servers SET steam_empty = NULL WHERE steam_empty = 0 AND last_seen < ?1",
            params![refresh_started],
        )
    }

    /// Drops rows not confirmed for `max_age_secs` (and their mod lists); returns the
    /// ids removed, so the front end can drop the same rows from its own map (Q24,
    /// D-235) instead of holding them until the next start.
    pub fn prune(&self, max_age_secs: i64) -> rusqlite::Result<Vec<String>> {
        let cutoff = ServerRow::now_unix() - max_age_secs;
        // Never prune a favourite (D-159): dropping the row emptied the Favourites
        // view for any server that was offline, or simply absent from the populated
        // partition, for 30 days.
        let ids = self
            .conn
            .prepare(
                "DELETE FROM servers
                 WHERE last_seen < ?1 AND id NOT IN (SELECT id FROM favourites)
                 RETURNING id",
            )?
            .query_map(params![cutoff], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let n = ids.len();
        // `prune` is the only DELETE on `servers`, so nothing can be orphaned unless
        // it deleted something — and the sweep measured 23.6 ms over 127 000 mod rows
        // every completed refresh (D-175). The one other way to orphan them is a
        // schema bump, which drops `servers`; `migrate` clears the mod tables there.
        if n > 0 {
            self.conn.execute_batch(
                "DELETE FROM server_mods WHERE server_id NOT IN (SELECT id FROM servers);
                 DELETE FROM server_mods_at WHERE server_id NOT IN (SELECT id FROM servers);",
            )?;
            // Row loss has been a mystery before (Q22), so every deletion is recorded.
            crate::log_info!("cache", "pruned {n} server(s) unseen since {cutoff}");
        }
        Ok(ids)
    }

    // ----- mod lists (D-080) ---------------------------------------------------

    /// The mod list last read from this server, newest scan wins. Used when a launch
    /// cannot reach the server for a fresh list (D-190).
    pub fn mods_for(&self, id: &str) -> rusqlite::Result<Vec<(u64, String)>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT mod_id, name FROM server_mods WHERE server_id = ?1")?;
        let rows = stmt.query_map(params![id], |r| {
            Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?))
        })?;
        rows.collect()
    }

    /// Replaces the stored mod lists of many servers in one transaction (DZSA import).
    pub fn replace_server_mods_many(
        &mut self,
        list: &[(String, Vec<(u64, String)>)],
        now: i64,
    ) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut del = tx.prepare_cached("DELETE FROM server_mods WHERE server_id = ?1")?;
            let mut ins = tx.prepare_cached(
                "INSERT OR REPLACE INTO server_mods (server_id, mod_id, name) VALUES (?1, ?2, ?3)",
            )?;
            let mut at = tx.prepare_cached(
                "INSERT INTO server_mods_at (server_id, scanned_at, mod_count) VALUES (?1, ?2, ?3)
                 ON CONFLICT(server_id) DO UPDATE SET scanned_at = excluded.scanned_at, mod_count = excluded.mod_count",
            )?;
            for (id, mods) in list {
                del.execute(params![id])?;
                for (mid, name) in mods {
                    ins.execute(params![id, *mid as i64, name])?;
                }
                at.execute(params![id, now, mods.len() as i64])?;
            }
        }
        tx.commit()
    }

    /// The mod list a scan recorded for one server, with the time it was scanned.
    ///
    /// The join dialog asks the server itself; this is what it falls back to when the
    /// server will not answer RULES, because "launching without mods" on a modded
    /// server is a kick, not a join (D-209).
    pub fn server_mods(&self, id: &str) -> rusqlite::Result<Option<ScannedMods>> {
        let scanned_at: Option<i64> = self
            .conn
            .query_row(
                "SELECT scanned_at FROM server_mods_at WHERE server_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(scanned_at) = scanned_at else {
            return Ok(None);
        };
        let mut stmt = self
            .conn
            .prepare_cached("SELECT mod_id, name FROM server_mods WHERE server_id = ?1")?;
        let rows = stmt.query_map(params![id], |r| {
            Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?))
        })?;
        let mut mods = Vec::new();
        for row in rows {
            mods.push(row?);
        }
        Ok(Some((scanned_at, mods)))
    }

    /// Everything the browser needs for the mod filter: names with server counts, and
    /// the mod ids per scanned server (servers scanned as vanilla have an empty list).
    pub fn mods_index(&self) -> rusqlite::Result<ModsIndex> {
        let mut catalog = Vec::new();
        {
            let mut stmt = self.conn.prepare(
                // `mod_id > 0`: id 0 is an unpublished, server-side-only mod. It ranked
                // 154th of 10 593 here, so it sat inside the 300 the dropdown offers -
                // and picking it set `filters.mod = 0`, which the filter reads as "any
                // mod", so the list did not change and the control snapped back to
                // "Any mod" (D-221).
                "SELECT mod_id, MAX(name), COUNT(*) FROM server_mods WHERE mod_id > 0 GROUP BY mod_id ORDER BY 3 DESC, 2",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(ModCatalogEntry {
                    id: r.get::<_, i64>(0)? as u64,
                    name: r.get(1)?,
                    servers: r.get::<_, i64>(2)? as usize,
                })
            })?;
            for row in rows {
                catalog.push(row?);
            }
        }
        let mut by_server: HashMap<String, Vec<u64>> = HashMap::new();
        {
            let mut stmt = self.conn.prepare("SELECT server_id FROM server_mods_at")?;
            for id in stmt.query_map([], |r| r.get::<_, String>(0))? {
                by_server.entry(id?).or_default();
            }
            let mut stmt = self
                .conn
                .prepare("SELECT server_id, mod_id FROM server_mods ORDER BY server_id")?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
            })?;
            for row in rows {
                let (sid, mid) = row?;
                by_server.entry(sid).or_default().push(mid);
            }
        }
        let index = by_server
            .into_iter()
            .map(|(id, mods)| ServerMods { id, mods })
            .collect();
        Ok(ModsIndex { catalog, index })
    }

    /// Row counts for the four tables that hold anything the user would miss.
    ///
    /// Q22 and Q25 are both "the rows are gone and nothing records when": logged once
    /// at open, a recurrence has a before-and-after instead of a guess (D-193).
    pub fn row_counts(&self) -> rusqlite::Result<RowCounts> {
        let one = |sql: &str| -> rusqlite::Result<i64> {
            self.conn.query_row(sql, [], |r| r.get::<_, i64>(0))
        };
        Ok(RowCounts {
            servers: one("SELECT count(*) FROM servers")?,
            favourites: one("SELECT count(*) FROM favourites")?,
            history: one("SELECT count(*) FROM history")?,
            population: one("SELECT count(*) FROM population")?,
        })
    }

    pub fn get_meta(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::verify::Verdict;

    fn row(id: u16, players: i32) -> ServerRow {
        ServerRow {
            id: ServerRow::id_for("51.81.8.81", id),
            ip: "51.81.8.81".into(),
            game_port: 2402,
            query_port: id,
            name: "WILDLANDZ".into(),
            map: "GreenCounty".into(),
            description: "PvP".into(),
            players,
            max_players: 50,
            bots: 0,
            password: false,
            secure: true,
            server_version: 129_163_709,
            version: "1.29.163709".into(),
            ping_ms: 108,
            keywords: "battleye,no3rd,lqs0,15:12".into(),
            tags: DayzTags::parse("battleye,no3rd,lqs0,15:12"),
            steam_id: 90_293_138_942_181_394,
            last_seen: ServerRow::now_unix(),
            verified_players: None,
            steam_empty: None,
            verified_at: None,
            verdict: None,
            country: None,
        }
    }

    #[test]
    fn upsert_load_roundtrip_and_prune() {
        let mut c = Cache::open_in_memory().unwrap();
        assert_eq!(
            c.get_meta("schema_version").unwrap().as_deref(),
            Some(SCHEMA_VERSION)
        );
        c.upsert(&[row(27017, 0), row(27018, 5)]).unwrap();
        assert_eq!(c.row_counts().unwrap().servers, 2);
        let loaded = c.load_all().unwrap();
        let w = loaded.iter().find(|r| r.query_port == 27017).unwrap();
        assert_eq!(w.version, "1.29.163709");
        assert!(w.tags.battleye && w.tags.first_person_only);
        assert_eq!(w.steam_id, 90_293_138_942_181_394);
        assert_eq!(w.steam_empty, None);

        let mut updated = row(27017, 7);
        updated.verified_players = Some(3);
        updated.steam_empty = Some(true);
        c.upsert(&[updated]).unwrap();
        let again = row(27017, 9); // None must not erase the stored verification columns
        c.upsert(&[again]).unwrap();
        let w = c
            .get(&ServerRow::id_for("51.81.8.81", 27017))
            .unwrap()
            .unwrap();
        assert_eq!(
            (w.players, w.verified_players, w.steam_empty),
            (9, Some(3), Some(true))
        );
        assert!(w.inflated(), "Steam says empty, INFO says 9");

        c.set_meta("last_refresh", "123").unwrap();
        assert_eq!(c.get_meta("last_refresh").unwrap().as_deref(), Some("123"));
        assert_eq!(c.get_meta("missing").unwrap(), None);
        assert_eq!(
            c.prune(-1).unwrap().len(),
            2,
            "everything is older than 'now + 1 s'"
        );
        assert_eq!(c.row_counts().unwrap().servers, 0);
    }

    /// A favourite must survive the 30-day prune (D-159): losing the row empties the
    /// Favourites view for anything that has been offline for a month.
    #[test]
    fn prune_keeps_favourites() {
        let mut c = Cache::open_in_memory().unwrap();
        let keep = row(27017, 0);
        let drop_me = row(27019, 0);
        c.upsert(&[keep.clone(), drop_me.clone()]).unwrap();
        c.favourite_set(&keep.id, true).unwrap();

        assert_eq!(
            c.prune(-1).unwrap(),
            vec![drop_me.id.clone()],
            "only the unfavourited row goes, and its id is reported"
        );
        assert_eq!(c.row_counts().unwrap().servers, 1);
        assert_eq!(c.favourites().unwrap().len(), 1);
    }

    #[test]
    fn favourites_history_and_population() {
        let mut c = Cache::open_in_memory().unwrap();
        let r = row(27017, 3);
        c.upsert(std::slice::from_ref(&r)).unwrap();
        c.favourite_set(&r.id, true).unwrap();
        c.favourite_set(&r.id, true).unwrap();
        assert_eq!(c.favourites().unwrap().len(), 1);
        c.favourite_set(&r.id, false).unwrap();
        assert!(c.favourites().unwrap().is_empty());

        c.history_add(&r, 12).unwrap();
        let h = c.history(10).unwrap();
        assert_eq!(
            (h[0].id.as_str(), h[0].mods, h[0].game_port),
            (r.id.as_str(), 12, 2402)
        );
        assert_eq!(c.history_clear().unwrap(), 1);
        assert!(c.history(10).unwrap().is_empty());
        assert_eq!(c.history_clear().unwrap(), 0, "clearing twice is harmless");

        let now = ServerRow::now_unix();
        c.population_add(&[
            (r.id.clone(), now - 7200, 10, 0),
            (r.id.clone(), now - 3600, 20, 2),
            (r.id.clone(), now - 3600, 99, 9),
        ])
        .unwrap();
        let p = c.population(&r.id, now - 86_400).unwrap();
        assert_eq!(p.len(), 2, "duplicate timestamp ignored");
        assert_eq!((p[1].players, p[1].queue), (20, 2));
        assert_eq!(c.population_prune(5000).unwrap(), 1);
        assert_eq!(c.population(&r.id, 0).unwrap().len(), 1);
    }

    #[test]
    fn verification_updates_counts_and_keeps_stale_info() {
        let mut c = Cache::open_in_memory().unwrap();
        c.upsert(&[row(27017, 40)]).unwrap();
        let id = ServerRow::id_for("51.81.8.81", 27017);
        c.apply_verifications(&[Verification {
            id: id.clone(),
            verdict: Verdict::Inflated,
            reported: 41,
            verified: Some(0),
            max_players: 50,
            ping_ms: Some(33),
            keywords: Some("battleye,16:00".into()),
            tags: None,
            verified_at: 1_000,
            reason: "test".into(),
        }])
        .unwrap();
        let w = c.get(&id).unwrap().unwrap();
        assert_eq!(
            (w.players, w.verified_players, w.verdict.as_deref()),
            (41, Some(0), Some("inflated"))
        );
        assert_eq!(
            (w.ping_ms, w.tags.time_string().as_deref()),
            (33, Some("16:00"))
        );
        // Offline verification (no INFO): keeps ping and keywords, records the verdict,
        // and — since it produced no count — keeps the last count and the time it was
        // taken (D-160). Overwriting them with NULL made the UI fall back to the
        // server's own number with nothing to say it was unverified.
        c.apply_verifications(&[Verification {
            id: id.clone(),
            verdict: Verdict::Offline,
            reported: 41,
            verified: None,
            max_players: 50,
            ping_ms: None,
            keywords: None,
            tags: None,
            verified_at: 2_000,
            reason: "test".into(),
        }])
        .unwrap();
        let w = c.get(&id).unwrap().unwrap();
        assert_eq!(
            (
                w.ping_ms,
                w.verdict.as_deref(),
                w.verified_at,
                w.verified_players
            ),
            (33, Some("offline"), Some(1_000), Some(0)),
            "verdict updates; the count and its timestamp survive a check that could not count"
        );
    }
}
