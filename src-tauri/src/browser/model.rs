//! One server as the UI sees it. Built from Steamworks `GameServerItem` (M3) and
//! later refined by direct A2S queries (ping, verified player head-count, mods).

use serde::Serialize;

use crate::a2s::DayzTags;

/// Sent to the UI, never read back from it: no `Deserialize` (D-257).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerRow {
    /// `ip:queryPort`, stable identity.
    pub id: String,
    pub ip: String,
    pub game_port: u16,
    pub query_port: u16,
    pub name: String,
    pub map: String,
    pub description: String,
    /// As reported in A2S_INFO. Often inflated (D-038); `verified_players` wins when present.
    pub players: i32,
    pub max_players: i32,
    // Stored and queried, never rendered: 19 000 rows cross IPC on every start, so
    // each unread field is ~19 000 copies of nothing (D-160).
    // Read by the front end since D-233: every fake-population tool found writes the
    // bots byte equal to its fabricated count (5 093 of 5 093 Steam-says-empty rows),
    // and honest servers that declare AI do not match their player count.
    pub bots: i32,
    pub password: bool,
    #[serde(skip_serializing)]
    pub secure: bool,
    /// Steam's integer form, e.g. 129163709.
    pub server_version: i32,
    /// `1.29.163709`
    pub version: String,
    pub ping_ms: u32,
    /// The raw A2S tag string; the UI reads the parsed `tags` instead.
    #[serde(skip_serializing)]
    pub keywords: String,
    pub tags: DayzTags,
    /// Steam's id for the server. Identity is `ip:queryPort` (docs/05 §4), so
    /// nothing in the UI uses this.
    #[serde(skip_serializing)]
    pub steam_id: u64,
    /// Unix seconds when this row was last confirmed by a query.
    #[serde(skip_serializing)]
    pub last_seen: i64,
    /// Head-count from A2S_PLAYER (M4+), `None` until verified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_players: Option<i32>,
    /// Steam's master server reported zero authenticated players (row came from a
    /// `noplayers` partition). `Some(true)` with `players > 0` means the A2S count
    /// is inflated (docs/11 rule R0, D-045). `None` when the partition was neutral.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steam_empty: Option<bool>,
    /// Unix seconds of the last PLAYER verification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<i64>,
    /// Verdict string from `verify::Verdict::as_str` (rules R2–R5), if verified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    /// ISO 3166-1 alpha-2 country of `ip` from the embedded GeoIP table (D-073);
    /// derived, not stored in the cache.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
}

impl ServerRow {
    /// Rule R0: Steam says nobody is authenticated on this server, yet A2S_INFO claims players.
    /// The UI mirrors this plus the verified head-count in `types.ts` (`trustedPlayers`).
    pub fn inflated(&self) -> bool {
        self.steam_empty == Some(true) && self.players > 0
    }

    pub fn id_for(ip: &str, query_port: u16) -> String {
        format!("{ip}:{query_port}")
    }

    /// Country code for an IPv4 string, `None` for unknown, private or unparsable addresses.
    pub fn country_for(ip: &str) -> Option<String> {
        crate::geoip::country(ip.parse().ok()?).map(str::to_owned)
    }

    /// Row from a direct A2S_INFO reply (direct connect, favourites import). Steam's
    /// authenticated count is unknown for such rows (`steam_empty = None`), so the
    /// PLAYER verification decides trust.
    pub fn from_info(ip: &str, query_port: u16, info: &crate::a2s::Info, ping_ms: u32) -> Self {
        let keywords = info.keywords.clone().unwrap_or_default();
        Self {
            id: Self::id_for(ip, query_port),
            ip: ip.to_string(),
            game_port: info.game_port.unwrap_or(query_port.saturating_sub(1)),
            query_port,
            name: info.name.clone(),
            map: info.map.clone(),
            description: info.game.clone(),
            players: info.players as i32,
            max_players: info.max_players as i32,
            bots: info.bots as i32,
            password: info.password,
            secure: info.vac,
            server_version: Self::version_int(&info.version),
            version: info.version.clone(),
            ping_ms,
            tags: info.tags.clone(),
            keywords,
            steam_id: info.steam_id.unwrap_or(0),
            last_seen: Self::now_unix(),
            verified_players: None,
            steam_empty: None,
            verified_at: None,
            verdict: None,
            country: Self::country_for(ip),
        }
    }

    /// `1.29.163709` → `129163709` (inverse of [`Self::version_string`]); 0 when unparsable.
    pub fn version_int(v: &str) -> i32 {
        let mut parts = v.trim().split('.');
        let (Some(a), Some(b), Some(c)) = (parts.next(), parts.next(), parts.next()) else {
            return 0;
        };
        match (a.parse::<i32>(), b.parse::<i32>(), c.parse::<i32>()) {
            // The D-197 guard bounded the *parts* and not the arithmetic, so it never
            // fixed anything: `a * 100_000_000` passes i32::MAX at a = 22, and the sum
            // overflows from "21.48.0" up. Release has overflow-checks off, so a server
            // advertising "99.99.999999" became 1_410_065_407 -> "14.10.65407", a version
            // nobody runs, which the join dialog then used to tell the user they would be
            // rejected; a dev build panicked inside the DZSA import instead. Both inputs
            // are server-controlled. Widen to i64 and let the conversion decide (D-219).
            (Ok(a), Ok(b), Ok(c)) if (0..100).contains(&a) && b < 100 && c < 1_000_000 => {
                let v = i64::from(a) * 100_000_000 + i64::from(b) * 1_000_000 + i64::from(c);
                i32::try_from(v).unwrap_or(0)
            }
            _ => 0,
        }
    }

    /// Steam packs DayZ versions as `MMmmBBBBBB` → `129163709` = `1.29.163709`.
    pub fn version_string(v: i32) -> String {
        if v <= 0 {
            return String::new();
        }
        let major = v / 100_000_000;
        let minor = (v / 1_000_000) % 100;
        let build = v % 1_000_000;
        format!("{major}.{minor}.{build}")
    }

    pub fn now_unix() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

/// The start-up list in columns (D-284). As objects every row repeats its field names:
/// 535 bytes a row, two thirds of them names and punctuation, 20 MB at 40 000 rows and
/// 36 MB at 71 000, parsed in one WebView task and held twice over by the host while
/// it was written. Here the names go once, each row is an array in `ROW_KEYS` order and
/// its tags an array in `TAG_KEYS` order. A value the object form leaves out is `null`,
/// which the reader skips, so both forms decode to the same row
/// (`compact_rows_decode_to_the_object_form`).
pub const ROW_KEYS: &[&str] = &[
    "id",
    "ip",
    "gamePort",
    "queryPort",
    "name",
    "map",
    "description",
    "players",
    "maxPlayers",
    "bots",
    "password",
    "serverVersion",
    "version",
    "pingMs",
    "tags",
    "verifiedPlayers",
    "steamEmpty",
    "verifiedAt",
    "verdict",
    "country",
];
pub const TAG_KEYS: &[&str] = &[
    "battleye",
    "firstPersonOnly",
    "privateHive",
    "modded",
    "dlc",
    "allowedFilePatching",
    "queue",
    "timeMultiplier",
    "nightMultiplier",
    "timeMinutes",
];

/// Rows written as `ROW_KEYS` arrays.
pub struct CompactRows(pub Vec<ServerRow>);

impl Serialize for CompactRows {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = s.serialize_seq(Some(self.0.len()))?;
        for r in &self.0 {
            seq.serialize_element(&CompactRow(r))?;
        }
        seq.end()
    }
}

struct CompactRow<'a>(&'a ServerRow);

impl Serialize for CompactRow<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;
        let r = self.0;
        let mut t = s.serialize_tuple(ROW_KEYS.len())?;
        t.serialize_element(&r.id)?;
        t.serialize_element(&r.ip)?;
        t.serialize_element(&r.game_port)?;
        t.serialize_element(&r.query_port)?;
        t.serialize_element(&r.name)?;
        t.serialize_element(&r.map)?;
        t.serialize_element(&r.description)?;
        t.serialize_element(&r.players)?;
        t.serialize_element(&r.max_players)?;
        t.serialize_element(&r.bots)?;
        t.serialize_element(&r.password)?;
        t.serialize_element(&r.server_version)?;
        t.serialize_element(&r.version)?;
        t.serialize_element(&r.ping_ms)?;
        t.serialize_element(&CompactTags(&r.tags))?;
        t.serialize_element(&r.verified_players)?;
        t.serialize_element(&r.steam_empty)?;
        t.serialize_element(&r.verified_at)?;
        t.serialize_element(&r.verdict)?;
        t.serialize_element(&r.country)?;
        t.end()
    }
}

struct CompactTags<'a>(&'a DayzTags);

impl Serialize for CompactTags<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;
        let g = self.0;
        let mut t = s.serialize_tuple(TAG_KEYS.len())?;
        t.serialize_element(&g.battleye)?;
        t.serialize_element(&g.first_person_only)?;
        t.serialize_element(&g.private_hive)?;
        t.serialize_element(&g.modded)?;
        t.serialize_element(&g.dlc)?;
        t.serialize_element(&g.allowed_file_patching)?;
        t.serialize_element(&g.queue)?;
        t.serialize_element(&g.time_multiplier)?;
        t.serialize_element(&g.night_multiplier)?;
        t.serialize_element(&g.time_minutes)?;
        t.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D-284: the compact start-up form decodes, the way the store reads it, to exactly
    /// the object every other event sends, for rows with every optional value absent and
    /// every one present.
    #[test]
    fn compact_rows_decode_to_the_object_form() {
        let bare = ServerRow {
            id: "1.2.3.4:2303".into(),
            ip: "1.2.3.4".into(),
            game_port: 2302,
            query_port: 2303,
            name: "Bare".into(),
            map: "chernarusplus".into(),
            description: String::new(),
            players: 0,
            max_players: 60,
            bots: 0,
            password: false,
            secure: true,
            server_version: 0,
            version: String::new(),
            ping_ms: 0,
            keywords: String::new(),
            tags: DayzTags::default(),
            steam_id: 0,
            last_seen: 0,
            verified_players: None,
            steam_empty: None,
            verified_at: None,
            verdict: None,
            country: None,
        };
        let mut full = bare.clone();
        full.id = "5.6.7.8:27016".into();
        full.name = "Full \"quoted\" name".into();
        full.tags = DayzTags::parse(
            "battleye,no3rd,external,privHive,shardABC,lqs3,etm4.000000,entm6.500000,mod,isDLC,allowedFilePatching,15:12",
        );
        full.verified_players = Some(42);
        full.steam_empty = Some(false);
        full.verified_at = Some(1_790_000_000);
        full.verdict = Some("verified".into());
        full.country = Some("DE".into());
        let rows = vec![bare, full];
        let compact = serde_json::to_value(CompactRows(rows.clone())).unwrap();
        let tag_at = ROW_KEYS.iter().position(|k| *k == "tags").unwrap();
        for (i, row) in rows.iter().enumerate() {
            let a = compact[i].as_array().unwrap();
            assert_eq!(a.len(), ROW_KEYS.len());
            let mut o = serde_json::Map::new();
            for (j, k) in ROW_KEYS.iter().enumerate() {
                if a[j].is_null() {
                    continue;
                }
                if j == tag_at {
                    let mut t = serde_json::Map::new();
                    for (m, tk) in TAG_KEYS.iter().enumerate() {
                        if !a[j][m].is_null() {
                            t.insert((*tk).into(), a[j][m].clone());
                        }
                    }
                    o.insert((*k).into(), serde_json::Value::Object(t));
                } else {
                    o.insert((*k).into(), a[j].clone());
                }
            }
            assert_eq!(
                serde_json::Value::Object(o),
                serde_json::to_value(row).unwrap(),
                "row {i}"
            );
        }
    }

    #[test]
    fn version_int_to_string() {
        assert_eq!(ServerRow::version_string(129_163_709), "1.29.163709");
        assert_eq!(
            ServerRow::version_string(129_163_451),
            "1.29.163451",
            "official launcher FavouriteServers.xml value"
        );
        assert_eq!(ServerRow::version_string(0), "");
        assert_eq!(ServerRow::version_int("1.29.163709"), 129_163_709);
        assert_eq!(
            ServerRow::version_string(ServerRow::version_int("1.30.164014")),
            "1.30.164014"
        );
        assert_eq!(ServerRow::version_int("garbage"), 0);
        // Every one of these is a string a remote server can put in A2S_INFO or the
        // DZSA JSON. Before D-219 the first overflowed i32, and in release (no
        // overflow-checks) the rest wrapped to plausible-looking version numbers.
        assert_eq!(ServerRow::version_int("21.47.483647"), 2_147_483_647);
        assert_eq!(ServerRow::version_int("21.48.0"), 0);
        assert_eq!(ServerRow::version_int("22.0.0"), 0);
        assert_eq!(ServerRow::version_int("99.99.999999"), 0);
    }
}
