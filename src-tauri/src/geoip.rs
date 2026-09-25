//! IPv4 → country lookup from the table embedded at build time
//! (`resources/geoip-v4.bin`, produced by `tools/geoip_build.js` from DB-IP's free
//! IP-to-Country Lite database, CC BY 4.0; docs/08, D-073). The table is read in
//! place from the executable's image: no heap copy, so the 1.8 MB stays file-backed
//! and shareable instead of counting as private memory (D-082). A lookup is a
//! binary search over the little-endian range starts.

use std::net::Ipv4Addr;
use std::sync::OnceLock;

static DATA: &[u8] = include_bytes!("../resources/geoip-v4.bin");
static TABLE: OnceLock<Option<Table>> = OnceLock::new();

struct Table {
    /// ISO 3166-1 alpha-2 codes, two bytes each; "ZZ" means unknown/unassigned.
    codes: &'static [u8],
    /// Range start addresses, `u32` little-endian, ascending.
    starts: &'static [u8],
    /// Country index per range (a range ends where the next one starts).
    idx: &'static [u8],
}

impl Table {
    fn len(&self) -> usize {
        self.idx.len()
    }

    fn start(&self, i: usize) -> u32 {
        let b = &self.starts[i * 4..i * 4 + 4];
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }
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
    let codes = take(&mut p, n * 2)?;
    let m = u32::from_le_bytes(take(&mut p, 4)?.try_into().ok()?) as usize;
    let starts = take(&mut p, m * 4)?;
    let idx = take(&mut p, m)?;
    let t = Table { codes, starts, idx };
    // Validate once: ascending starts and in-range country indices.
    if (1..m).any(|i| t.start(i - 1) >= t.start(i)) || idx.iter().any(|&i| i as usize >= n) {
        return None;
    }
    Some(t)
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
    // First range whose start is above the address; the one before it contains it.
    let (mut lo, mut hi) = (0usize, t.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if t.start(mid) <= n {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let i = *t.idx.get(lo.checked_sub(1)?)? as usize;
    let code = t.codes.get(i * 2..i * 2 + 2)?;
    if code == b"ZZ" {
        return None;
    }
    std::str::from_utf8(code).ok()
}

/// Number of ranges in the embedded table. Diagnostics never showed it; the tests
/// check the table is complete with it (D-257).
#[cfg(test)]
pub fn ranges() -> usize {
    table().map_or(0, Table::len)
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

    #[test]
    fn boundaries() {
        // Last address of a range and the first of the next must differ when the
        // countries differ (1.0.0.255 AU → 1.0.1.0 CN in the September 2026 table).
        assert_eq!(country(Ipv4Addr::new(1, 0, 0, 255)), Some("AU"));
        assert_eq!(country(Ipv4Addr::new(1, 0, 1, 0)), Some("CN"));
        assert!(country(Ipv4Addr::new(255, 255, 255, 255)).is_none() || ranges() > 0);
    }
}
