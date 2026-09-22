//! Request builders, datagram classification and split-packet reassembly
//! (Valve "Server queries", S-15; docs/03 §2).

use super::reader::Reader;
use super::{A2sError, A2sResult};

pub const HEADER_SINGLE: u32 = 0xFFFF_FFFF;
pub const HEADER_SPLIT: u32 = 0xFFFF_FFFE;
pub const TYPE_CHALLENGE: u8 = 0x41;
pub const MAX_DATAGRAM: usize = 65_535;
/// Upper bound for a reassembled response; DayZ RULES for 40 mods is ~3 KB.
pub const MAX_RESPONSE: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Info,
    Rules,
    Players,
}

impl Kind {
    /// Receive-buffer size for one datagram of this kind. DayZ answers RULES in a
    /// single unsplit 5.3 KB datagram and a hostile server may send far more, so
    /// RULES keeps the full 64 KiB; INFO is a few hundred bytes and PLAYER for a
    /// full 255-slot server is ~10 KB, so a 16 KiB buffer covers both with room to
    /// spare and stops the verification pass zeroing 64 KiB per query (D-160).
    pub fn max_datagram(self) -> usize {
        match self {
            Kind::Rules => MAX_DATAGRAM,
            Kind::Info | Kind::Players => 16 * 1024,
        }
    }

    pub fn response_type(self) -> u8 {
        match self {
            Kind::Info => 0x49,
            Kind::Rules => 0x45,
            Kind::Players => 0x44,
        }
    }

    /// Request bytes. INFO appends the challenge only when it has one; RULES and
    /// PLAYER always carry a 4-byte challenge slot (`FF FF FF FF` on first contact).
    pub fn request(self, challenge: Option<[u8; 4]>) -> Vec<u8> {
        let mut v = Vec::with_capacity(32);
        v.extend_from_slice(&HEADER_SINGLE.to_le_bytes());
        match self {
            Kind::Info => {
                v.push(0x54);
                v.extend_from_slice(b"Source Engine Query\0");
                if let Some(c) = challenge {
                    v.extend_from_slice(&c);
                }
            }
            Kind::Rules | Kind::Players => {
                v.push(if self == Kind::Rules { 0x56 } else { 0x55 });
                v.extend_from_slice(&challenge.unwrap_or([0xFF; 4]));
            }
        }
        v
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Datagram<'a> {
    /// Payload after the `FF FF FF FF` header (starts with the type byte).
    Single(&'a [u8]),
    Split {
        id: u32,
        total: u8,
        number: u8,
        size: u16,
        body: &'a [u8],
    },
}

pub fn classify(d: &[u8]) -> A2sResult<Datagram<'_>> {
    let mut r = Reader::new(d);
    match r.u32()? {
        HEADER_SINGLE => Ok(Datagram::Single(&d[4..])),
        HEADER_SPLIT => {
            let id = r.u32()?;
            if id & 0x8000_0000 != 0 {
                return Err(A2sError::Compressed);
            }
            let total = r.u8()?;
            let number = r.u8()?;
            let size = r.u16()?;
            let body = &d[r.pos()..];
            if total == 0 || number >= total {
                return Err(A2sError::SplitMismatch("fragment index"));
            }
            Ok(Datagram::Split {
                id,
                total,
                number,
                size,
                body,
            })
        }
        other => Err(A2sError::BadHeader(other)),
    }
}

/// Collects fragments of one split response. Duplicates are ignored; a fragment
/// from a different response id resets the collector.
#[derive(Debug, Default)]
pub struct Reassembler {
    id: Option<u32>,
    parts: Vec<Option<Vec<u8>>>,
    have: usize,
    bytes: usize,
}

impl Reassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the complete payload (single-packet header stripped) once every
    /// fragment has arrived.
    pub fn push(
        &mut self,
        id: u32,
        total: u8,
        number: u8,
        body: &[u8],
    ) -> A2sResult<Option<Vec<u8>>> {
        if self.id != Some(id) {
            self.id = Some(id);
            self.parts = vec![None; total as usize];
            self.have = 0;
            self.bytes = 0;
        }
        if self.parts.len() != total as usize {
            return Err(A2sError::SplitMismatch("fragment count changed"));
        }
        let slot = &mut self.parts[number as usize];
        if slot.is_none() {
            self.bytes += body.len();
            if self.bytes > MAX_RESPONSE {
                return Err(A2sError::Malformed("response larger than 64 KiB"));
            }
            *slot = Some(body.to_vec());
            self.have += 1;
        }
        if self.have < self.parts.len() {
            return Ok(None);
        }
        let mut full = Vec::with_capacity(self.bytes);
        for p in self.parts.iter().flatten() {
            full.extend_from_slice(p);
        }
        // Reassembled data normally begins with the single-packet header again.
        if full.len() >= 4
            && u32::from_le_bytes([full[0], full[1], full[2], full[3]]) == HEADER_SINGLE
        {
            full.drain(..4);
        }
        Ok(Some(full))
    }
}

/// Splits a single-packet payload into a challenge or a typed response.
pub fn challenge_of(payload: &[u8]) -> Option<[u8; 4]> {
    if payload.len() >= 5 && payload[0] == TYPE_CHALLENGE {
        Some([payload[1], payload[2], payload[3], payload[4]])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_request_matches_spec() {
        let r = Kind::Info.request(None);
        assert_eq!(&r[..5], &[0xFF, 0xFF, 0xFF, 0xFF, 0x54]);
        assert_eq!(&r[5..], b"Source Engine Query\0");
        let r2 = Kind::Info.request(Some([1, 2, 3, 4]));
        assert_eq!(&r2[r2.len() - 4..], &[1, 2, 3, 4]);
    }

    #[test]
    fn rules_and_player_requests() {
        assert_eq!(
            Kind::Rules.request(None),
            vec![0xFF, 0xFF, 0xFF, 0xFF, 0x56, 0xFF, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(
            Kind::Players.request(Some([9, 8, 7, 6])),
            vec![0xFF, 0xFF, 0xFF, 0xFF, 0x55, 9, 8, 7, 6]
        );
    }

    #[test]
    fn classify_single_and_split() {
        assert_eq!(
            classify(&[0xFF, 0xFF, 0xFF, 0xFF, 0x49, 1]).unwrap(),
            Datagram::Single(&[0x49, 1])
        );
        let d = [
            0xFE, 0xFF, 0xFF, 0xFF, 0x10, 0x00, 0x00, 0x00, 2, 1, 0xE0, 0x04, 0xAA,
        ];
        assert_eq!(
            classify(&d).unwrap(),
            Datagram::Split {
                id: 0x10,
                total: 2,
                number: 1,
                size: 0x04E0,
                body: &[0xAA]
            }
        );
        assert!(matches!(
            classify(&[0xFE, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0x80, 1, 0, 0, 0]),
            Err(A2sError::Compressed)
        ));
        assert!(matches!(
            classify(&[1, 2, 3, 4]),
            Err(A2sError::BadHeader(0x0403_0201))
        ));
    }

    #[test]
    fn reassembler_orders_and_strips_header() {
        let mut r = Reassembler::new();
        assert_eq!(r.push(7, 2, 1, b"world").unwrap(), None);
        assert_eq!(
            r.push(7, 2, 1, b"world").unwrap(),
            None,
            "duplicate ignored"
        );
        let full = r
            .push(
                7,
                2,
                0,
                &[0xFF, 0xFF, 0xFF, 0xFF, b'h', b'e', b'l', b'l', b'o', b' '],
            )
            .unwrap()
            .unwrap();
        assert_eq!(full, b"hello world");
    }

    #[test]
    fn challenge_detection() {
        assert_eq!(challenge_of(&[0x41, 1, 2, 3, 4]), Some([1, 2, 3, 4]));
        assert_eq!(challenge_of(&[0x49, 1, 2, 3, 4]), None);
    }
}
