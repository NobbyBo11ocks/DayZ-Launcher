//! A2S_RULES response (`0x45`) and the DayZ Server Browser Protocol v2 payload
//! smuggled through it (docs/03 §4; S-12 WoozyMasta a3sb, S-14 dayzquery, S-42 live).
//!
//! Fragments: rule pairs whose key is exactly two bytes `[index (1-based), total]`
//! with `index <= total`. Concatenate by index, unescape
//! (`01 01`→`01`, `01 02`→`00`, `01 03`→`FF`), then decode:
//! version u8, overflow u8, dlcFlags u16, dlcHash u32×popcount, modCount u8,
//! mods { hash u32, idLen u8 & 0x0F, id LE, name pstr }, sigCount u8, sigs pstr[],
//! description pstr (optional).

use std::collections::BTreeMap;

use serde::Serialize;

use super::packet::Kind;
use super::reader::Reader;
use super::{A2sError, A2sResult};

pub const DAYZ_PROTOCOL_VERSION: u8 = 2;

/// DayZ DLC bit → name (S-12 `dlc.go`).
pub const DAYZ_DLC: [(u16, &str); 4] = [(0x1, "Livonia"), (0x2, "Frost Line"), (0x4, "Badlands"), (0x8, "Survivor GameZ")];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mod {
    pub hash: u32,
    pub workshop_id: u64,
    pub id_len: u8,
    /// `mod.cpp` `name`, not the Workshop title.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dlc {
    pub bit: u16,
    pub name: &'static str,
    pub hash: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayzRules {
    pub protocol_version: u8,
    pub overflow: u8,
    pub dlc_flags: u16,
    pub dlc: Vec<Dlc>,
    pub mods: Vec<Mod>,
    pub signatures: Vec<String>,
    pub description: Option<String>,
    /// Bytes left after the description; non-zero means the layout drifted.
    pub trailing: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rules {
    pub rule_count: u16,
    pub fragment_count: usize,
    pub plain: BTreeMap<String, String>,
    pub dayz: Option<DayzRules>,
}

impl Rules {
    pub fn plain_u32(&self, key: &str) -> Option<u32> {
        self.plain.get(key)?.parse().ok()
    }
}

pub fn parse(payload: &[u8]) -> A2sResult<Rules> {
    let mut r = Reader::new(payload);
    let t = r.u8()?;
    if t != Kind::Rules.response_type() {
        return Err(A2sError::UnexpectedType {
            expected: Kind::Rules.response_type(),
            got: t,
        });
    }
    let rule_count = r.u16()?;
    let mut plain = BTreeMap::new();
    let mut fragments: Vec<(u8, u8, &[u8])> = Vec::new();
    for _ in 0..rule_count {
        let key = r.cstr_bytes()?;
        let value = r.cstr_bytes()?;
        if key.len() == 2 && key[0] <= key[1] {
            fragments.push((key[0], key[1], value));
        } else {
            plain.insert(
                String::from_utf8_lossy(key).into_owned(),
                String::from_utf8_lossy(value).into_owned(),
            );
        }
    }
    let dayz = if fragments.is_empty() {
        None
    } else {
        fragments.sort_by_key(|f| f.0);
        let total = fragments[0].1 as usize;
        if fragments.len() != total || fragments.iter().any(|f| f.1 as usize != total) {
            return Err(A2sError::SplitMismatch("dayz rule fragments"));
        }
        let raw: Vec<u8> = fragments.iter().flat_map(|f| f.2.iter().copied()).collect();
        Some(decode_dayz(&unescape(&raw))?)
    };
    Ok(Rules {
        rule_count,
        fragment_count: fragments.len(),
        plain,
        dayz,
    })
}

pub fn unescape(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == 0x01 && i + 1 < raw.len() {
            match raw[i + 1] {
                0x01 => {
                    out.push(0x01);
                    i += 2;
                    continue;
                }
                0x02 => {
                    out.push(0x00);
                    i += 2;
                    continue;
                }
                0x03 => {
                    out.push(0xFF);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        out.push(raw[i]);
        i += 1;
    }
    out
}

pub fn decode_dayz(p: &[u8]) -> A2sResult<DayzRules> {
    let mut r = Reader::new(p);
    let protocol_version = r.u8()?;
    if protocol_version != DAYZ_PROTOCOL_VERSION {
        return Err(A2sError::Malformed("dayz protocol version"));
    }
    let overflow = r.u8()?;
    let dlc_flags = r.u16()?;
    let mut dlc = Vec::new();
    for bit in 0..16u16 {
        let mask = 1u16 << bit;
        if dlc_flags & mask != 0 {
            let name = DAYZ_DLC.iter().find(|(m, _)| *m == mask).map_or("Unknown DLC", |(_, n)| n);
            dlc.push(Dlc {
                bit: mask,
                name,
                hash: r.u32()?,
            });
        }
    }
    let mod_count = r.u8()?;
    let mut mods = Vec::with_capacity(mod_count as usize);
    for _ in 0..mod_count {
        let hash = r.u32()?;
        let id_len = r.u8()?;
        let width = (id_len & 0x0F) as usize;
        if width == 0 || width > 8 {
            return Err(A2sError::Malformed("mod id width"));
        }
        let workshop_id = r.uint_n(width)?;
        let name = r.pstr()?;
        mods.push(Mod {
            hash,
            workshop_id,
            id_len,
            name,
        });
    }
    let sig_count = r.u8()?;
    let mut signatures = Vec::with_capacity(sig_count as usize);
    for _ in 0..sig_count {
        signatures.push(r.pstr()?);
    }
    let description = if r.is_empty() { None } else { Some(r.pstr()?) };
    Ok(DayzRules {
        protocol_version,
        overflow,
        dlc_flags,
        dlc,
        mods,
        signatures,
        description,
        trailing: r.remaining(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2s::packet::{classify, Datagram};

    const WILDLANDZ: &[u8] = include_bytes!("../../tests/fixtures/a2s/wildlandz.rules.0.bin");

    #[test]
    fn unescape_table() {
        assert_eq!(unescape(&[0x41, 0x01, 0x01, 0x01, 0x02, 0x01, 0x03, 0x42, 0x01]), vec![0x41, 0x01, 0x00, 0xFF, 0x42, 0x01]);
        assert_eq!(unescape(&[0x01, 0x07]), vec![0x01, 0x07], "unknown escape passes through");
    }

    #[test]
    fn decodes_live_fixture_byte_for_byte() {
        let Datagram::Single(payload) = classify(WILDLANDZ).unwrap() else { panic!("single") };
        let rules = parse(payload).unwrap();
        assert_eq!(rules.rule_count, 13);
        assert_eq!(rules.fragment_count, 4);
        assert_eq!(rules.plain.get("island").map(String::as_str), Some("GreenCounty"));
        assert_eq!(rules.plain.get("platform").map(String::as_str), Some("win"));
        assert_eq!(rules.plain_u32("requiredVersion"), Some(129));
        assert_eq!(rules.plain.len(), 9);

        let d = rules.dayz.expect("dayz payload");
        assert_eq!(d.protocol_version, 2);
        assert_eq!(d.overflow, 0);
        assert_eq!(d.dlc_flags, 0);
        assert!(d.dlc.is_empty());
        assert_eq!(d.mods.len(), 12);
        assert_eq!(d.trailing, 0, "every byte accounted for");

        let ids: Vec<u64> = d.mods.iter().map(|m| m.workshop_id).collect();
        assert_eq!(
            ids,
            vec![
                3_747_295_862,
                3_669_560_078,
                3_669_559_886,
                3_669_559_715,
                2_857_994_912,
                3_702_420_204,
                1_819_514_788,
                2_276_010_135,
                2_545_327_648,
                1_828_439_124,
                1_559_212_036,
                2_128_098_372
            ]
        );
        let cf = d.mods.iter().find(|m| m.workshop_id == 1_559_212_036).unwrap();
        assert_eq!(cf.name, "Community Framework");
        assert_eq!(cf.hash, 0x68AF_C6C1);
        assert_eq!(cf.id_len, 4);
        assert_eq!(d.mods[0].name, "Summer In America");
        assert_eq!(d.signatures.len(), 12);
        assert_eq!(d.signatures[0], "CooltrainV3");
        assert_eq!(d.signatures[11], "Zenarchist");
        assert_eq!(d.description.as_deref(), Some("PvP | Survival | No Bases"));
    }

    const KINGOFGAMES: &[u8] = include_bytes!("../../tests/fixtures/a2s/kingofgames.rules.0.bin");
    const VOLATILE: &[u8] = include_bytes!("../../tests/fixtures/a2s/volatile.rules.0.bin");

    #[test]
    fn decodes_single_datagram_with_121_mods() {
        assert_eq!(KINGOFGAMES.len(), 4587, "DayZ answers in one datagram well above 1400 bytes (D-033)");
        let Datagram::Single(payload) = classify(KINGOFGAMES).unwrap() else { panic!("single") };
        let r = parse(payload).unwrap();
        assert_eq!(r.rule_count, 43);
        assert_eq!(r.fragment_count, 34);
        assert_eq!(r.plain.len(), 9);
        let d = r.dayz.unwrap();
        assert_eq!(d.mods.len(), 121);
        assert_eq!(d.mods[0].name, "GF_Barrels");
        assert_eq!(d.mods[0].workshop_id, 3_023_834_511);
        assert_eq!(d.mods[120].workshop_id, 1_559_212_036);
        assert_eq!(d.mods[120].hash, 0x68AF_C6C1, "same CF hash as the WILDLANDZ capture: it is a content hash (D-035)");
        assert!(d.mods.iter().all(|m| m.id_len == 4));
        assert_eq!(d.signatures.len(), 95);
        assert_eq!(d.signatures[0], "ADM");
        assert_eq!(d.signatures[94], "Zenarchist");
        assert_eq!(d.description.as_deref(), Some(""), "empty description is still length-prefixed");
        assert_eq!(d.trailing, 0);
    }

    #[test]
    fn decodes_single_datagram_with_105_mods_and_197_keys() {
        assert_eq!(VOLATILE.len(), 5309);
        let Datagram::Single(payload) = classify(VOLATILE).unwrap() else { panic!("single") };
        let r = parse(payload).unwrap();
        assert_eq!(r.rule_count, 49);
        assert_eq!(r.fragment_count, 40);
        let d = r.dayz.unwrap();
        assert_eq!(d.mods.len(), 105);
        assert_eq!(d.mods[0].name, "Volatile Heli Pack");
        assert_eq!(d.mods[0].workshop_id, 3_270_576_624);
        assert_eq!(d.signatures.len(), 197);
        assert_eq!(d.signatures[0], "28MLRPMusic");
        assert_eq!(d.signatures[196], "zmg_psac");
        assert!(d.description.as_deref().unwrap().starts_with("Survival at its Finest"));
        assert_eq!(d.trailing, 0);
    }

    #[test]
    fn rejects_wrong_version() {
        assert!(matches!(decode_dayz(&[3, 0, 0, 0, 0]), Err(A2sError::Malformed("dayz protocol version"))));
    }
}
