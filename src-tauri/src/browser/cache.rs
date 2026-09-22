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
CREATE INDEX IF NOT EXISTS servers_last_seen ON servers(last_seen);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS server_mods (
  server_id TEXT NOT NULL,
  mod_id INTEGER NOT NULL,
  name TEXT NOT NULL,
  PRIMARY KEY (server_id, mod_id)
);
CREATE INDEX IF NOT EXISTS server_mods_mod ON server_mods(mod_id);
CREATE TABLE IF NOT EXISTS server_mods_at (
  server_id TEXT PRIMARY KEY,
  scanned_at INTEGER NOT NULL,
  mod_count INTEGER NOT NULL
);
";

const SELECT_COLUMNS: &str = "id, ip, game_port, query_port, name, map, description, players, max_players, bots, password, secure,
                    server_version, ping_ms, keywords, steam_id, last_seen, verified_players, steam_empty, verified_at, verdict";

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

/// Row counts and file sizes of the cache (D-115).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub servers: i64,
    pub favourites: i64,
    pub history: i64,
    pub population: i64,
    pub mod_lists: i64,
    pub db_bytes: u64,
    pub wal_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PopulationSample {
    pub ts: i64,
    pub players: i32,
    pub queue: i32,
}

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

    #[cfg(test)]
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
            conn.execute_batch("DROP TABLE IF EXISTS servers;")?;
            conn.execute_batch(SCHEMA)?;
            conn.execute(
                "INSERT INTO meta (key, value) VALUES ('schema_version', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![SCHEMA_VERSION],
            )?;
        } else {
            conn.execute_batch(SCHEMA)?;
        }
        conn.execute_batch(USER_SCHEMA)?;
        // Additive migration of a user table (D-083): the alert flag on favourites.
        let has_alert = conn
            .prepare("PRAGMA table_info(favourites)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(Result::ok)
            .any(|c| c == "alert");
        if !has_alert {
            conn.execute_batch(
                "ALTER TABLE favourites ADD COLUMN alert INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        Ok(Self { conn })
    }

    // ----- favourites --------------------------------------------------------

    /// `(id, added_at, alert)` newest first.
    pub fn favourites(&self) -> rusqlite::Result<Vec<(String, i64, bool)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, added_at, alert FROM favourites ORDER BY added_at DESC")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)))?;
        rows.collect()
    }

    /// Watch (or stop watching) a favourite for a free slot / a return online (D-083).
    pub fn favourite_alert_set(&self, id: &str, on: bool) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE favourites SET alert = ?2 WHERE id = ?1",
            params![id, i64::from(on)],
        )?;
        Ok(())
    }

    /// Watched favourites that have a cached row: `(id, ip, query_port, name)`.
    pub fn favourites_watched(&self) -> rusqlite::Result<Vec<(String, String, u16, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, s.ip, s.query_port, s.name FROM favourites f JOIN servers s ON s.id = f.id WHERE f.alert = 1",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u16, r.get(3)?))
        })?;
        rows.collect()
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
        self.checkpoint();
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
        self.checkpoint();
        Ok(())
    }

    /// Empties the join history: the user's confirmed "Clear list" on the Recent view
    /// (D-130), the only code path that deletes history rows. Returns the number
    /// removed and checkpoints so the deletion is as durable as the inserts.
    pub fn history_clear(&self) -> rusqlite::Result<usize> {
        let n = self.conn.execute("DELETE FROM history", [])?;
        self.checkpoint();
        Ok(n)
    }

    /// Folds the write-ahead log into the database file without blocking readers.
    pub fn checkpoint(&self) {
        let _ = self.conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
    }

    /// Row counts and file sizes for Diagnostics (D-115): a quick way to see whether
    /// user data went missing (Q22) without opening the database by hand.
    pub fn stats(&self) -> rusqlite::Result<CacheStats> {
        let count = |sql: &str| self.conn.query_row(sql, [], |r| r.get::<_, i64>(0));
        let path = self.conn.path().map(std::path::PathBuf::from);
        let size = |suffix: &str| {
            path.as_ref().map_or(0, |p| {
                let mut s = p.as_os_str().to_os_string();
                s.push(suffix);
                std::fs::metadata(s).map(|m| m.len()).unwrap_or(0)
            })
        };
        Ok(CacheStats {
            servers: count("SELECT COUNT(*) FROM servers")?,
            favourites: count("SELECT COUNT(*) FROM favourites")?,
            history: count("SELECT COUNT(*) FROM history")?,
            population: count("SELECT COUNT(*) FROM population")?,
            mod_lists: count("SELECT COUNT(*) FROM server_mods_at")?,
            db_bytes: size(""),
            wal_bytes: size("-wal"),
        })
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

    pub fn count(&self) -> rusqlite::Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM servers", [], |r| r.get(0))
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

    pub fn load_all(&self) -> rusqlite::Result<Vec<ServerRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM servers ORDER BY last_seen DESC"
        ))?;
        let rows = stmt.query_map([], Self::row_from)?;
        rows.collect()
    }

    pub fn get(&self, id: &str) -> rusqlite::Result<Option<ServerRow>> {
        self.conn
            .query_row(
                &format!("SELECT {SELECT_COLUMNS} FROM servers WHERE id = ?1"),
                params![id],
                Self::row_from,
            )
            .optional()
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
            let mut stmt = tx.prepare_cached(
                "UPDATE servers SET
                   verified_players = ?2, verified_at = ?3, verdict = ?4,
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

    /// Drops rows not confirmed for `max_age_secs` (and their mod lists); returns how many were removed.
    pub fn prune(&self, max_age_secs: i64) -> rusqlite::Result<usize> {
        let cutoff = ServerRow::now_unix() - max_age_secs;
        let n = self
            .conn
            .execute("DELETE FROM servers WHERE last_seen < ?1", params![cutoff])?;
        self.conn.execute_batch(
            "DELETE FROM server_mods WHERE server_id NOT IN (SELECT id FROM servers);
             DELETE FROM server_mods_at WHERE server_id NOT IN (SELECT id FROM servers);",
        )?;
        Ok(n)
    }

    // ----- mod lists (D-080) ---------------------------------------------------

    /// Replaces the stored mod list of one server (A2S_RULES DayZ payload).
    pub fn replace_server_mods(
        &mut self,
        id: &str,
        mods: &[(u64, String)],
        now: i64,
    ) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM server_mods WHERE server_id = ?1", params![id])?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT OR REPLACE INTO server_mods (server_id, mod_id, name) VALUES (?1, ?2, ?3)",
            )?;
            for (mid, name) in mods {
                ins.execute(params![id, *mid as i64, name])?;
            }
        }
        tx.execute(
            "INSERT INTO server_mods_at (server_id, scanned_at, mod_count) VALUES (?1, ?2, ?3)
             ON CONFLICT(server_id) DO UPDATE SET scanned_at = excluded.scanned_at, mod_count = excluded.mod_count",
            params![id, now, mods.len() as i64],
        )?;
        tx.commit()
    }

    /// Same as [`Self::replace_server_mods`] for many servers in one transaction (DZSA import).
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

    /// Unix seconds of each server's last mod scan.
    pub fn mods_scanned(&self) -> rusqlite::Result<HashMap<String, i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT server_id, scanned_at FROM server_mods_at")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        rows.collect()
    }

    /// Everything the browser needs for the mod filter: names with server counts, and
    /// the mod ids per scanned server (servers scanned as vanilla have an empty list).
    pub fn mods_index(&self) -> rusqlite::Result<ModsIndex> {
        let mut catalog = Vec::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT mod_id, MAX(name), COUNT(*) FROM server_mods GROUP BY mod_id ORDER BY 3 DESC, 2",
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
        assert_eq!(c.count().unwrap(), 2);
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
            c.prune(-1).unwrap(),
            2,
            "everything is older than 'now + 1 s'"
        );
        assert_eq!(c.count().unwrap(), 0);
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
        // Offline verification (no INFO): keeps ping/keywords, still records the verdict.
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
            (w.ping_ms, w.verdict.as_deref(), w.verified_at),
            (33, Some("offline"), Some(2_000))
        );
    }
}
