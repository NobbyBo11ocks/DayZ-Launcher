//! IPv4 → country lookup from the table embedded at build time
//! (`resources/geoip-v4.bin`, produced by `tools/geoip_build.js` from DB-IP's free
//! IP-to-Country Lite database, CC BY 4.0; docs/08, D-073). Parsed once on first use;
//! a lookup is a binary search over ~350k range starts.

use std::net::Ipv4Addr;
use std::sync::OnceLock;

static DATA: &[u8] = include_bytes!("../resources/geoip-v4.bin");
static TABLE: OnceLock<Option<Table>> = OnceLock::new();

struct Table {
    /// ISO 3166-1 alpha-2 codes; "ZZ" means unknown/unassigned.
    codes: Vec<[u8; 2]>,
    starts: Vec<u32>,
    idx: &'static [u8],
}

fn parse(data: &'static [u8]) -> Option<Table> {
    let mut p = 0usize;
    let take = |p: &mut usize, n: usize| -> Option<&'static [u8]> {
        let s = data.get(*p..*p + n)?;
        *p += n;
        Some(s)
    };
    if take(&mut p, 6)? != b"DZGEO1" {
        return None;
    }
    let n = u16::from_le_bytes(take(&mut p, 2)?.try_into().ok()?) as usize;
    let codes: Vec<[u8; 2]> = take(&mut p, n * 2)?.as_chunks::<2>().0.to_vec();
    let m = u32::from_le_bytes(take(&mut p, 4)?.try_into().ok()?) as usize;
    let starts: Vec<u32> = take(&mut p, m * 4)?
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    let idx = take(&mut p, m)?;
    if starts.windows(2).any(|w| w[0] >= w[1]) || idx.iter().any(|&i| i as usize >= n) {
        return None;
    }
    Some(Table { codes, starts, idx })
}

fn table() -> Option<&'static Table> {
    TABLE.get_or_init(|| parse(DATA)).as_ref()
}

/// Two-letter country code for a public IPv4 address, `None` when unknown ("ZZ"),
/// private/reserved, or when the table failed to load.
pub fn country(ip: Ipv4Addr) -> Option<&'static str> {
    if ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() {
        return None;
    }
    let t = table()?;
    let n = u32::from(ip);
    let pos = t.starts.partition_point(|&s| s <= n);
    let i = *t.idx.get(pos.checked_sub(1)?)? as usize;
    let code = t.codes.get(i)?;
    if code == b"ZZ" {
        return None;
    }
    std::str::from_utf8(code).ok()
}

/// Number of ranges in the embedded table (Diagnostics).
pub fn ranges() -> usize {
    table().map_or(0, |t| t.starts.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_loads() {
        assert!(ranges() > 100_000, "embedded table missing or truncated");
    }

    #[test]
    fn known_addresses() {
        // First DB-IP rows: 1.0.0.0/24 is APNIC (AU), 1.0.1.0–1.0.3.255 CN; Google DNS is US.
        assert_eq!(country(Ipv4Addr::new(1, 0, 0, 1)), Some("AU"));
        assert_eq!(country(Ipv4Addr::new(1, 0, 2, 9)), Some("CN"));
        assert_eq!(country(Ipv4Addr::new(8, 8, 8, 8)), Some("US"));
    }

    #[test]
    fn private_and_unknown() {
        assert_eq!(country(Ipv4Addr::new(192, 168, 1, 1)), None);
        assert_eq!(country(Ipv4Addr::new(127, 0, 0, 1)), None);
        assert_eq!(country(Ipv4Addr::new(0, 1, 2, 3)), None); // 0.0.0.0/8 is "ZZ"
    }
}
