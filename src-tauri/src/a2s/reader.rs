//! Bounds-checked little-endian cursor over a byte slice. Every read returns
//! `Truncated` instead of panicking, so a hostile server cannot crash the app.

use super::{A2sError, A2sResult};

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub fn bytes(&mut self, n: usize) -> A2sResult<&'a [u8]> {
        if self.remaining() < n {
            return Err(A2sError::Truncated {
                need: n,
                at: self.pos,
            });
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn u8(&mut self) -> A2sResult<u8> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u16(&mut self) -> A2sResult<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self) -> A2sResult<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn i32(&mut self) -> A2sResult<i32> {
        Ok(self.u32()? as i32)
    }

    pub fn u64(&mut self) -> A2sResult<u64> {
        let b = self.bytes(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub fn f32(&mut self) -> A2sResult<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// Little-endian unsigned integer of `n` bytes (1–8).
    pub fn uint_n(&mut self, n: usize) -> A2sResult<u64> {
        if n == 0 || n > 8 {
            return Err(A2sError::Malformed("integer width"));
        }
        let b = self.bytes(n)?;
        Ok(b.iter().rev().fold(0u64, |acc, &x| (acc << 8) | x as u64))
    }

    /// NUL-terminated byte string (terminator consumed, not returned).
    pub fn cstr_bytes(&mut self) -> A2sResult<&'a [u8]> {
        let rest = &self.buf[self.pos..];
        let end = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or(A2sError::Truncated {
                need: 1,
                at: self.buf.len(),
            })?;
        let s = &rest[..end];
        self.pos += end + 1;
        Ok(s)
    }

    pub fn cstr(&mut self) -> A2sResult<String> {
        Ok(String::from_utf8_lossy(self.cstr_bytes()?).into_owned())
    }

    /// Length-prefixed (u8) string as DayZ packs mod names, signatures and the description.
    pub fn pstr(&mut self) -> A2sResult<String> {
        let n = self.u8()? as usize;
        Ok(String::from_utf8_lossy(self.bytes(n)?).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_bounds() {
        let mut r = Reader::new(&[0x01, 0x02, 0x03, b'h', b'i', 0, 0x78, 0x56, 0x34, 0x12]);
        assert_eq!(r.u8().unwrap(), 1);
        assert_eq!(r.u16().unwrap(), 0x0302);
        assert_eq!(r.cstr().unwrap(), "hi");
        assert_eq!(r.u32().unwrap(), 0x1234_5678);
        assert!(r.is_empty());
        assert!(matches!(
            r.u8(),
            Err(A2sError::Truncated { need: 1, at: 10 })
        ));
    }

    #[test]
    fn uint_n_little_endian() {
        let mut r = Reader::new(&[0x04, 0xB0, 0xEF, 0x5C]);
        assert_eq!(r.uint_n(4).unwrap(), 1_559_212_036);
    }
}
