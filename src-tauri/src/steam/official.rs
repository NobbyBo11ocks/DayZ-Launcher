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
    Ok(parse_favourites(&decode(&std::fs::read(path)?)))
}

/// The official launcher declares `encoding="windows-1252"` and means it: "Café" is
/// the single byte 0xE9, which `read_to_string` refused as invalid UTF-8 — one such
/// name anywhere failed the whole import (D-239). UTF-8 is taken as it is; anything
/// else is Windows-1252, which maps every byte to a character. Every delimiter the
/// scanner looks for is ASCII in both.
fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| cp1252(b)).collect(),
    }
}

fn cp1252(b: u8) -> char {
    // 0x80–0x9F are the only bytes where Windows-1252 differs from Latin-1; the five
    // it leaves undefined keep their C1 code points.
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž',
        '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}',
        'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => HIGH[usize::from(b - 0x80)],
        _ => char::from(b),
    }
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

/// XML entities, in one pass. Characters outside Windows-1252 — every Cyrillic name —
/// arrive as character references (`&#1057;`), which were left on screen as they
/// were; and chained `replace` calls decoded `&amp;lt;` twice (D-239).
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        // The longest reference that can decode is `&#x10FFFF;` — eleven characters —
        // so the scan for its `;` stops there: `find` over the whole tail made a run of
        // bare `&` quadratic (D-245). Control characters never come from a name.
        let end = tail
            .char_indices()
            .take(12)
            .find(|&(_, c)| c == ';')
            .map(|(i, _)| i);
        let decoded = end.and_then(|end| {
            let entity = &tail[1..end];
            let c = match entity {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "amp" => Some('&'),
                _ => entity.strip_prefix('#').and_then(|n| {
                    match n.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => n.parse().ok(),
                    }
                    .and_then(char::from_u32)
                }),
            };
            c.filter(|c| !c.is_control()).map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
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

    /// What the official launcher actually writes (`encoding="windows-1252"`): a
    /// Latin-1 letter as its single byte, and anything outside the code page as a
    /// character reference. Either one failed or garbled the import (D-239).
    #[test]
    fn windows_1252_bytes_and_character_references() {
        let mut xml =
            br#"<?xml version="1.0" encoding="windows-1252"?><FavoriteServers><Server Name="Caf"#
                .to_vec();
        xml.push(0xE9);
        xml.push(0x96); // en dash in Windows-1252
        xml.extend_from_slice(
            br#" &#1057;&#x435;&#1088;&#1074;&#1077;&#1088; &amp;lt;" QueryEndPoint="1.2.3.4:27016"/></FavoriteServers>"#,
        );
        let f = parse_favourites(&decode(&xml));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].name, "Café– Сервер &lt;");
        // UTF-8 stays UTF-8, with or without a byte-order mark.
        let utf8 = "\u{feff}<Server Name=\"Café\" QueryEndPoint=\"1.2.3.4:27016\"/>";
        assert_eq!(parse_favourites(&decode(utf8.as_bytes()))[0].name, "Café");
        // A stray ampersand is kept rather than eaten.
        assert_eq!(unescape("A & B &bogus; &#xZZ;"), "A & B &bogus; &#xZZ;");
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
