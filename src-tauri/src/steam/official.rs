//! Reading the official launcher's local data (docs/02 §7): `FavouriteServers.xml`
//! in `%LOCALAPPDATA%\DayZ Launcher`. One `<Server …/>` element per favourite with
//! attributes such as `QueryEndPoint="ip:port"`, `ConnectionEndPoint`, `Name`, `Map`.
//! The file is tiny and flat, so a dependency-free attribute scanner is enough.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficialFavourite {
    pub name: String,
    pub query_ip: String,
    pub query_port: u16,
    pub game_port: u16,
    pub map: String,
    pub max_players: i32,
    pub server_version: i32,
    pub tags: String,
    pub password: bool,
}

pub fn favourites_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(local)
            .join("DayZ Launcher")
            .join("FavouriteServers.xml"),
    )
}

pub fn read_favourites(path: &Path) -> std::io::Result<Vec<OfficialFavourite>> {
    let text = std::fs::read_to_string(path)?;
    Ok(parse_favourites(&text))
}

pub fn parse_favourites(xml: &str) -> Vec<OfficialFavourite> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<Server") {
        let after = &rest[start + "<Server".len()..];
        // Element must be followed by whitespace or '/' (not e.g. <ServerList>).
        if !after.starts_with(|c: char| c.is_whitespace() || c == '/') {
            rest = after;
            continue;
        }
        let Some(end) = after.find('>') else { break };
        let attrs = parse_attrs(&after[..end]);
        rest = &after[end + 1..];
        let Some((qip, qport)) = attrs.get("QueryEndPoint").and_then(|s| split_endpoint(s)) else {
            continue;
        };
        let game_port = attrs
            .get("ConnectionEndPoint")
            .and_then(|s| split_endpoint(s))
            .map(|(_, p)| p)
            .or_else(|| attrs.get("Port").and_then(|p| p.parse().ok()))
            .unwrap_or(0);
        out.push(OfficialFavourite {
            name: attrs.get("Name").cloned().unwrap_or_default(),
            query_ip: qip,
            query_port: qport,
            game_port,
            map: attrs.get("Map").cloned().unwrap_or_default(),
            max_players: attrs
                .get("MaxPlayers")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            server_version: attrs
                .get("ServerVersion")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            tags: attrs.get("Tags").cloned().unwrap_or_default(),
            password: attrs
                .get("RequirePassword")
                .map(|v| v == "1")
                .unwrap_or(false),
        });
    }
    out
}

fn split_endpoint(s: &str) -> Option<(String, u16)> {
    let (ip, port) = s.rsplit_once(':')?;
    Some((ip.to_string(), port.parse().ok()?))
}

/// `Key="value"` pairs; values are XML-unescaped.
fn parse_attrs(s: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    // ASCII tests only (D-151): `(byte as char).is_whitespace()` is true for 0xA0, which
    // is a UTF-8 continuation byte, so a name containing e.g. "ą" or a non-breaking space
    // could stop the scan mid-character and panic when slicing. Every delimiter here is
    // ASCII, and a continuation byte is never equal to one, so byte indices stay on
    // character boundaries.
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let key_start = i;
        while i < bytes.len()
            && bytes[i] != b'='
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'/'
        {
            i += 1;
        }
        let key = &s[key_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            if key.is_empty() {
                i += 1;
            }
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let quote = bytes[i];
        if quote != b'"' && quote != b'\'' {
            continue;
        }
        i += 1;
        let val_start = i;
        while i < bytes.len() && bytes[i] != quote {
            i += 1;
        }
        let val = &s[val_start..i.min(bytes.len())];
        i += 1;
        if !key.is_empty() {
            out.insert(key.to_string(), unescape(val));
        }
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIVE: &str = include_str!("../../tests/fixtures/FavouriteServers.xml");

    #[test]
    fn parses_live_fixture() {
        let f = parse_favourites(LIVE);
        assert_eq!(f.len(), 1);
        let s = &f[0];
        assert_eq!(
            s.name,
            "Bro-Nation PvP | Deathmatch | Designed for 2-10 players"
        );
        assert_eq!(
            (s.query_ip.as_str(), s.query_port, s.game_port),
            ("99.137.91.235", 5003, 5002)
        );
        assert_eq!(s.map, "chernarusplus");
        assert_eq!(s.max_players, 10);
        assert_eq!(s.server_version, 129_163_451);
        assert!(s.tags.contains("battleye") && !s.password);
    }

    #[test]
    fn handles_entities_and_multiple_entries() {
        let xml = r#"<?xml version="1.0"?><FavoriteServers>
            <Server Name="A &amp; B &quot;x&quot;" QueryEndPoint="1.2.3.4:27016" ConnectionEndPoint="1.2.3.4:2302" RequirePassword="1"/>
            <ServerList/>
            <Server Name='single' QueryEndPoint='5.6.7.8:2303' Port="2302" />
            <Server Name="no endpoint" />
        </FavoriteServers>"#;
        let f = parse_favourites(xml);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].name, "A & B \"x\"");
        assert!(f[0].password);
        assert_eq!((f[1].query_port, f[1].game_port), (2303, 2302));
    }

    /// A name with non-ASCII bytes used to stop the attribute scan mid-character and
    /// panic when slicing, which aborted the whole process (D-151).
    #[test]
    fn non_ascii_names_do_not_panic() {
        let xml = concat!(
            r#"<FavoriteServers>"#,
            "<Server Name=\"Polski\u{a0}serwer ą ę €\" QueryEndPoint=\"1.2.3.4:27016\" ConnectionEndPoint=\"1.2.3.4:2302\"/>",
            "<Server\u{a0}Name=\"ĄĘÓŁ\" QueryEndPoint=\"5.6.7.8:27017\"/>",
            r#"</FavoriteServers>"#,
        );
        let f = parse_favourites(xml);
        assert_eq!(f.len(), 2, "both servers parse");
        assert_eq!(f[0].name, "Polski\u{a0}serwer ą ę €");
        assert_eq!(f[0].query_port, 27016);
        assert_eq!(f[1].query_port, 27017);
    }
}
