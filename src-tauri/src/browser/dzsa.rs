//! Fallback server list from the DZSA Launcher's public API (D-089, docs/08 S-60),
//! used when Steam is not available. One ~24 MB JSON document with ~18 500 servers
//! including their mod lists; fetched only on demand or when Steam fails to
//! initialise, never on a schedule. Rows carry no Steam count (`steam_empty = None`),
//! so the usual A2S verification decides trust afterwards.

use serde::Deserialize;

use super::ServerRow;
use crate::a2s::DayzTags;

pub const DZSA_URL: &str = "https://dayzsalauncher.com/api/v1/launcher/servers/dayz";
/// The API answers a browser User-Agent; the default reqwest one is refused (docs/03).
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120 Safari/537.36";

#[derive(Debug, Deserialize)]
struct Document {
    #[serde(default)]
    result: Vec<DzsaServer>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Endpoint {
    ip: String,
    port: u16,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct DzsaMod {
    name: String,
    steam_workshop_id: u64,
}

/// Only the fields the row needs; everything else in the document is ignored.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct DzsaServer {
    endpoint: Endpoint,
    game_port: u16,
    name: String,
    map: String,
    players: i32,
    max_players: i32,
    password: bool,
    version: String,
    vac: bool,
    battl_eye: bool,
    first_person_only: bool,
    shard: String,
    time_acceleration: f64,
    time: String,
    mods: Vec<DzsaMod>,
}

/// A server row plus its mod list as DZSA reports it.
pub struct DzsaRow {
    pub row: ServerRow,
    pub mods: Vec<(u64, String)>,
}

/// Downloads and converts the whole list. Blocking network work happens inside
/// reqwest's own runtime; the JSON is deserialised once into the typed structs.
pub async fn fetch() -> Result<Vec<DzsaRow>, String> {
    // No redirects and https only, like the news client (D-160): reqwest would otherwise
    // follow a redirect from this third party to any host, plain http included, and
    // seed the cache from it (D-245).
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(90))
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let resp = client
        .get(DZSA_URL)
        .send()
        .await
        .map_err(|e| format!("DZSA request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("DZSA answered HTTP {}", resp.status()));
    }
    // The live list is ~24 MB; the cap is generous but finite (D-160).
    let bytes = crate::http::body_capped(resp, 64 * 1024 * 1024, "DZSA server list").await?;
    let doc: Document = serde_json::from_slice(&bytes).map_err(|e| format!("DZSA JSON: {e}"))?;
    let now = ServerRow::now_unix();
    Ok(doc
        .result
        .into_iter()
        .filter_map(|s| convert(s, now))
        .collect())
}

/// Rebuilds the A2S keyword string from DZSA's fields so `DayzTags::parse` and every
/// filter see the same shape as a Steam row (docs/03 §keywords).
fn keywords(s: &DzsaServer) -> String {
    let mut k: Vec<String> = Vec::with_capacity(6);
    if s.battl_eye {
        k.push("battleye".into());
    }
    if s.first_person_only {
        k.push("no3rd".into());
    }
    if s.shard.eq_ignore_ascii_case("private") {
        k.push("privHive".into());
    }
    if s.time_acceleration > 0.0 {
        k.push(format!("etm{:.6}", s.time_acceleration));
    }
    if !s.mods.is_empty() {
        k.push("mod".into());
    }
    if s.time.len() == 5 && s.time.as_bytes()[2] == b':' {
        k.push(s.time.clone());
    }
    k.join(",")
}

fn convert(s: DzsaServer, now: i64) -> Option<DzsaRow> {
    // Every consumer parses the address again and refuses a bad one, so a non-address
    // here only ever made a dead row; it is dropped at the door instead (D-245).
    if s.endpoint.ip.parse::<std::net::Ipv4Addr>().is_err() || s.endpoint.port == 0 {
        return None;
    }
    let ip = s.endpoint.ip.clone();
    let keywords = keywords(&s);
    let row = ServerRow {
        id: ServerRow::id_for(&ip, s.endpoint.port),
        country: ServerRow::country_for(&ip),
        ip,
        game_port: if s.game_port > 0 {
            s.game_port
        } else {
            s.endpoint.port.saturating_sub(1)
        },
        query_port: s.endpoint.port,
        name: s.name.clone(),
        map: s.map.clone(),
        description: String::new(),
        players: s.players,
        max_players: s.max_players,
        bots: 0,
        password: s.password,
        secure: s.vac,
        server_version: ServerRow::version_int(&s.version),
        version: s.version.clone(),
        ping_ms: 0,
        tags: DayzTags::parse(&keywords),
        keywords,
        steam_id: 0,
        last_seen: now,
        verified_players: None,
        steam_empty: None,
        verified_at: None,
        verdict: None,
    };
    let mods = s
        .mods
        .into_iter()
        .filter(|m| m.steam_workshop_id > 0)
        .map(|m| (m.steam_workshop_id, m.name))
        .collect();
    Some(DzsaRow { row, mods })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_a_document_row() {
        let json = r#"{"status":"ok","result":[{"gamePort":2302,"endpoint":{"ip":"185.216.144.79","port":2303},
          "name":"Planeta aceituna","map":"chernarusplus","players":3,"maxPlayers":10,"password":true,
          "version":"1.29.163709","vac":true,"battlEye":true,"firstPersonOnly":true,"shard":"private",
          "timeAcceleration":4,"time":"05:22","mods":[{"name":"Item Info","steamWorkshopId":2888277755}]}]}"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        let rows: Vec<DzsaRow> = doc
            .result
            .into_iter()
            .filter_map(|s| convert(s, 1))
            .collect();
        assert_eq!(rows.len(), 1);
        let r = &rows[0].row;
        assert_eq!(r.id, "185.216.144.79:2303");
        assert_eq!(r.game_port, 2302);
        assert_eq!(r.server_version, 129_163_709);
        assert!(r.tags.battleye && r.tags.first_person_only && r.tags.private_hive);
        assert_eq!(r.tags.time_minutes, Some(5 * 60 + 22));
        assert!(r.tags.modded);
        assert_eq!(rows[0].mods, vec![(2_888_277_755, "Item Info".to_string())]);
        assert_eq!(r.steam_empty, None);
    }
}
