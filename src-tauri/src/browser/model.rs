//! One server as the UI sees it. Built from Steamworks `GameServerItem` (M3) and
//! later refined by direct A2S queries (ping, verified player head-count, mods).

use serde::{Deserialize, Serialize};

use crate::a2s::DayzTags;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub bots: i32,
    pub password: bool,
    pub secure: bool,
    /// Steam's integer form, e.g. 129163709.
    pub server_version: i32,
    /// `1.29.163709`
    pub version: String,
    pub ping_ms: u32,
    pub keywords: String,
    #[serde(default)]
    pub tags: DayzTags,
    pub steam_id: u64,
    /// Unix seconds when this row was last confirmed by a query.
    pub last_seen: i64,
    /// Head-count from A2S_PLAYER (M4+), `None` until verified.
    #[serde(default)]
    pub verified_players: Option<i32>,
    /// Steam's master server reported zero authenticated players (row came from a
    /// `noplayers` partition). `Some(true)` with `players > 0` means the A2S count
    /// is inflated (docs/11 rule R0, D-045). `None` when the partition was neutral.
    #[serde(default)]
    pub steam_empty: Option<bool>,
    /// Unix seconds of the last PLAYER verification.
    #[serde(default)]
    pub verified_at: Option<i64>,
    /// Verdict string from `verify::Verdict::as_str` (rules R2–R5), if verified.
    #[serde(default)]
    pub verdict: Option<String>,
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
        }
    }

    /// `1.29.163709` → `129163709` (inverse of [`Self::version_string`]); 0 when unparsable.
    pub fn version_int(v: &str) -> i32 {
        let mut parts = v.trim().split('.');
        let (Some(a), Some(b), Some(c)) = (parts.next(), parts.next(), parts.next()) else {
            return 0;
        };
        match (a.parse::<i32>(), b.parse::<i32>(), c.parse::<i32>()) {
            (Ok(a), Ok(b), Ok(c)) if b < 100 && c < 1_000_000 => a * 100_000_000 + b * 1_000_000 + c,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_int_to_string() {
        assert_eq!(ServerRow::version_string(129_163_709), "1.29.163709");
        assert_eq!(ServerRow::version_string(129_163_451), "1.29.163451", "official launcher FavouriteServers.xml value");
        assert_eq!(ServerRow::version_string(0), "");
        assert_eq!(ServerRow::version_int("1.29.163709"), 129_163_709);
        assert_eq!(ServerRow::version_string(ServerRow::version_int("1.30.164014")), "1.30.164014");
        assert_eq!(ServerRow::version_int("garbage"), 0);
    }
}
