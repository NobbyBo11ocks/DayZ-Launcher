//! A2S_INFO response (`0x49`), docs/03 §3.

use serde::Serialize;

use super::packet::Kind;
use super::reader::Reader;
use super::tags::DayzTags;
use super::{A2sError, A2sResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub protocol: u8,
    pub name: String,
    pub map: String,
    pub folder: String,
    /// For DayZ this is the server description from `serverDZ.cfg`.
    pub game: String,
    /// The EDF GameID low 24 bits when present, else the 16-bit AppID field
    /// (which reads 0 for DayZ because 221100 overflows).
    pub app_id: u32,
    pub players: u8,
    pub max_players: u8,
    pub bots: u8,
    pub server_type: char,
    pub environment: char,
    pub password: bool,
    pub vac: bool,
    pub version: String,
    pub edf: u8,
    /// Game port for `-connect`/`-port` (EDF 0x80).
    pub game_port: Option<u16>,
    pub steam_id: Option<u64>,
    pub spectator_port: Option<u16>,
    pub spectator_name: Option<String>,
    pub keywords: Option<String>,
    pub game_id: Option<u64>,
    pub tags: DayzTags,
}

pub fn parse(payload: &[u8]) -> A2sResult<Info> {
    let mut r = Reader::new(payload);
    let t = r.u8()?;
    if t != Kind::Info.response_type() {
        return Err(A2sError::UnexpectedType {
            expected: Kind::Info.response_type(),
            got: t,
        });
    }
    let protocol = r.u8()?;
    let name = r.cstr()?;
    let map = r.cstr()?;
    let folder = r.cstr()?;
    let game = r.cstr()?;
    let app_id16 = r.u16()?;
    let players = r.u8()?;
    let max_players = r.u8()?;
    let bots = r.u8()?;
    let server_type = r.u8()? as char;
    let environment = r.u8()? as char;
    let password = r.u8()? != 0;
    let vac = r.u8()? != 0;
    let version = r.cstr()?;
    let edf = if r.is_empty() { 0 } else { r.u8()? };

    let mut info = Info {
        protocol,
        name,
        map,
        folder,
        game,
        app_id: app_id16 as u32,
        players,
        max_players,
        bots,
        server_type,
        environment,
        password,
        vac,
        version,
        edf,
        game_port: None,
        steam_id: None,
        spectator_port: None,
        spectator_name: None,
        keywords: None,
        game_id: None,
        tags: DayzTags::default(),
    };
    if edf & 0x80 != 0 {
        info.game_port = Some(r.u16()?);
    }
    if edf & 0x10 != 0 {
        info.steam_id = Some(r.u64()?);
    }
    if edf & 0x40 != 0 {
        info.spectator_port = Some(r.u16()?);
        info.spectator_name = Some(r.cstr()?);
    }
    if edf & 0x20 != 0 {
        let k = r.cstr()?;
        info.tags = DayzTags::parse(&k);
        info.keywords = Some(k);
    }
    if edf & 0x01 != 0 {
        let gid = r.u64()?;
        info.game_id = Some(gid);
        info.app_id = (gid & 0x00FF_FFFF) as u32;
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2s::packet::{classify, Datagram};

    const WILDLANDZ: &[u8] = include_bytes!("../../tests/fixtures/a2s/wildlandz.info.bin");

    #[test]
    fn decodes_live_fixture() {
        let Datagram::Single(payload) = classify(WILDLANDZ).unwrap() else { panic!("single") };
        let i = parse(payload).unwrap();
        assert_eq!(i.protocol, 17);
        assert_eq!(i.name, "WILDLANDZ Green County Greatness");
        assert_eq!(i.map, "GreenCounty");
        assert_eq!(i.folder, "dayz");
        assert_eq!(i.game, "PvP | Survival | No Bases");
        assert_eq!(i.app_id, 221_100, "from GameID, not the overflowed 16-bit field");
        assert_eq!(i.max_players, 50);
        assert_eq!(i.server_type, 'd');
        assert_eq!(i.environment, 'w');
        assert!(!i.password && i.vac);
        assert_eq!(i.version, "1.29.163709");
        assert_eq!(i.edf, 0xB1);
        assert_eq!(i.game_port, Some(2402));
        assert_eq!(i.steam_id, Some(90_293_138_942_181_394));
        assert_eq!(i.game_id, Some(221_100));
        assert!(i.tags.battleye && i.tags.first_person_only && i.tags.private_hive && i.tags.modded);
        assert_eq!(i.tags.shard.as_deref(), Some("ABC123"));
        assert_eq!(i.tags.time_multiplier, Some(4.0));
    }

    const KINGOFGAMES: &[u8] = include_bytes!("../../tests/fixtures/a2s/kingofgames.info.bin");

    #[test]
    fn decodes_populated_official_style_server() {
        let Datagram::Single(payload) = classify(KINGOFGAMES).unwrap() else { panic!("single") };
        let i = parse(payload).unwrap();
        assert!(i.name.starts_with("[EU] King of Games"));
        assert_eq!(i.map, "chernarusplus");
        assert_eq!((i.players, i.max_players), (2, 100));
        assert_eq!(i.game_port, Some(2302));
        assert_eq!(i.edf, 0xB1);
        assert_eq!(i.app_id, 221_100);
        let t = &i.tags;
        assert!(t.battleye && t.external && t.private_hive && t.modded && t.allowed_file_patching);
        assert!(!t.first_person_only);
        assert_eq!(t.shard.as_deref(), Some("123ABC"));
        assert_eq!(t.queue, Some(0));
        assert_eq!(t.time_multiplier, Some(6.0));
        assert_eq!(t.night_multiplier, Some(64.0));
        assert_eq!(t.time_string().as_deref(), Some("08:25"));
        assert!(t.unknown.is_empty(), "unknown tags: {:?}", t.unknown);
    }

    #[test]
    fn rejects_wrong_type() {
        assert!(matches!(parse(&[0x45, 0, 0]), Err(A2sError::UnexpectedType { expected: 0x49, got: 0x45 })));
    }
}
