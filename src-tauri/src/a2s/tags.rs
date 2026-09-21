//! DayZ keyword tags carried in A2S_INFO's `keywords` string (docs/03 §3, S-37).
//! Live example: `battleye,no3rd,external,privHive,shardABC123,lqs0,etm4.000000,entm6.000000,mod,15:12`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayzTags {
    pub battleye: bool,
    pub first_person_only: bool,
    pub external: bool,
    pub private_hive: bool,
    pub modded: bool,
    pub dlc: bool,
    /// Server permits `-filePatching` clients (`allowedFilePatching`, seen live D-033).
    pub allowed_file_patching: bool,
    pub shard: Option<String>,
    /// Players waiting in the login queue (`lqs<N>`).
    pub queue: Option<u32>,
    /// Day time acceleration (`etm<f>`).
    pub time_multiplier: Option<f32>,
    /// Night time acceleration (`entm<f>`).
    pub night_multiplier: Option<f32>,
    /// In-game clock as minutes since midnight (`HH:MM`).
    pub time_minutes: Option<u16>,
    pub unknown: Vec<String>,
}

impl DayzTags {
    pub fn parse(keywords: &str) -> Self {
        let mut t = DayzTags::default();
        for tag in keywords.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match tag {
                "battleye" => t.battleye = true,
                "no3rd" => t.first_person_only = true,
                "external" => t.external = true,
                "privHive" => t.private_hive = true,
                "mod" => t.modded = true,
                "isDLC" => t.dlc = true,
                "allowedFilePatching" => t.allowed_file_patching = true,
                _ => {
                    if let Some(s) = tag.strip_prefix("shard") {
                        t.shard = Some(s.to_string());
                    } else if let Some(n) = tag.strip_prefix("lqs").and_then(|s| s.parse().ok()) {
                        t.queue = Some(n);
                    } else if let Some(f) = tag.strip_prefix("entm").and_then(|s| s.parse().ok()) {
                        t.night_multiplier = Some(f);
                    } else if let Some(f) = tag.strip_prefix("etm").and_then(|s| s.parse().ok()) {
                        t.time_multiplier = Some(f);
                    } else if let Some(m) = parse_clock(tag) {
                        t.time_minutes = Some(m);
                    } else {
                        t.unknown.push(tag.to_string());
                    }
                }
            }
        }
        t
    }

    /// `HH:MM` for display.
    pub fn time_string(&self) -> Option<String> {
        self.time_minutes.map(|m| format!("{:02}:{:02}", m / 60, m % 60))
    }
}

fn parse_clock(s: &str) -> Option<u16> {
    let (h, m) = s.split_once(':')?;
    let h: u16 = h.parse().ok()?;
    let m: u16 = m.parse().ok()?;
    (h < 24 && m < 60).then_some(h * 60 + m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_keywords() {
        let t = DayzTags::parse("battleye,no3rd,external,privHive,shardABC123,lqs0,etm4.000000,entm6.000000,mod,15:12");
        assert!(t.battleye && t.first_person_only && t.external && t.private_hive && t.modded);
        assert!(!t.dlc);
        assert_eq!(t.shard.as_deref(), Some("ABC123"));
        assert_eq!(t.queue, Some(0));
        assert_eq!(t.time_multiplier, Some(4.0));
        assert_eq!(t.night_multiplier, Some(6.0));
        assert_eq!(t.time_minutes, Some(15 * 60 + 12));
        assert_eq!(t.time_string().as_deref(), Some("15:12"));
        assert!(t.unknown.is_empty());
    }

    #[test]
    fn official_style_keywords() {
        let t = DayzTags::parse("battleye,shard001,lqs3,etm12.000000,entm4.000000,isDLC,03:07,weird");
        assert_eq!(t.shard.as_deref(), Some("001"));
        assert_eq!(t.queue, Some(3));
        assert!(t.dlc && !t.first_person_only);
        assert_eq!(t.time_string().as_deref(), Some("03:07"));
        assert_eq!(t.unknown, vec!["weird".to_string()]);
    }
}
