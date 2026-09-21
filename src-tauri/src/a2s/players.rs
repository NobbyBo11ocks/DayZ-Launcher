//! A2S_PLAYER response (`0x44`), docs/03 §5. DayZ answered the empty test server
//! with count 0; whether names appear on populated servers is open question Q3.

use serde::Serialize;

use super::packet::Kind;
use super::reader::Reader;
use super::{A2sError, A2sResult};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub index: u8,
    pub name: String,
    pub score: i32,
    pub duration_secs: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Players {
    pub count: u8,
    pub players: Vec<Player>,
}

pub fn parse(payload: &[u8]) -> A2sResult<Players> {
    let mut r = Reader::new(payload);
    let t = r.u8()?;
    if t != Kind::Players.response_type() {
        return Err(A2sError::UnexpectedType {
            expected: Kind::Players.response_type(),
            got: t,
        });
    }
    let count = r.u8()?;
    let mut players = Vec::with_capacity(count as usize);
    for _ in 0..count {
        if r.is_empty() {
            break; // some engines announce more entries than they send
        }
        players.push(Player {
            index: r.u8()?,
            name: r.cstr()?,
            score: r.i32()?,
            duration_secs: r.f32()?,
        });
    }
    Ok(Players { count, players })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2s::packet::{classify, Datagram};

    const WILDLANDZ: &[u8] = include_bytes!("../../tests/fixtures/a2s/wildlandz.player.bin");

    #[test]
    fn decodes_live_fixture() {
        let Datagram::Single(payload) = classify(WILDLANDZ).unwrap() else { panic!("single") };
        let p = parse(payload).unwrap();
        assert_eq!(p.players.len() as u8, p.count);
    }

    const KINGOFGAMES: &[u8] = include_bytes!("../../tests/fixtures/a2s/kingofgames.player.bin");
    const VOLATILE: &[u8] = include_bytes!("../../tests/fixtures/a2s/volatile.player.bin");

    /// DayZ returns one entry per connected player with an empty name and score 0
    /// but a real connection duration (D-034, closes Q3).
    #[test]
    fn populated_servers_return_anonymous_entries_with_durations() {
        for (bytes, expected) in [(KINGOFGAMES, 2u8), (VOLATILE, 4u8)] {
            let Datagram::Single(payload) = classify(bytes).unwrap() else { panic!("single") };
            let p = parse(payload).unwrap();
            assert_eq!(p.count, expected);
            assert_eq!(p.players.len(), expected as usize);
            assert!(p.players.iter().all(|x| x.name.is_empty() && x.score == 0 && x.duration_secs > 60.0));
        }
    }

    #[test]
    fn decodes_synthetic_entry() {
        let mut b = vec![0x44, 1, 0];
        b.extend_from_slice(b"Bob\0");
        b.extend_from_slice(&5i32.to_le_bytes());
        b.extend_from_slice(&120.5f32.to_le_bytes());
        let p = parse(&b).unwrap();
        assert_eq!(p.players[0].name, "Bob");
        assert_eq!(p.players[0].score, 5);
        assert_eq!(p.players[0].duration_secs, 120.5);
    }
}
