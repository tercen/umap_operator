//! Streaming TSON writer.
//!
//! TSON is Tercen's binary JSON (see `tercen/rustson` `spec.rs`): a version string, then one value.
//! Values: type byte + payload; strings are NUL-terminated (`STRING_TYPE`), lists and maps carry
//! a `u32` element count, typed lists carry a `u32` element count followed by raw little-endian
//! data, and string lists carry a `u32` **byte** count followed by NUL-terminated strings.
//!
//! `rustson::encode` needs the whole value in memory; an FCS result can be several GB, so this
//! writer emits the same bytes straight to a `Write` (a `BufWriter<File>`), column by column.
//! Byte-compatible with rustson (checked in the unit test below).
use std::io::{self, Write};

pub const NULL_TYPE: u8 = 0;
pub const STRING_TYPE: u8 = 1;
pub const INTEGER_TYPE: u8 = 2;
pub const DOUBLE_TYPE: u8 = 3;
pub const BOOL_TYPE: u8 = 4;
pub const LIST_TYPE: u8 = 10;
pub const MAP_TYPE: u8 = 11;
pub const LIST_INT32_TYPE: u8 = 105;
pub const LIST_FLOAT64_TYPE: u8 = 111;
pub const LIST_STRING_TYPE: u8 = 112;
pub const VERSION: &str = "1.1.0";

pub struct TsonWriter<W: Write> {
    w: W,
    pub bytes: u64,
}

impl<W: Write> TsonWriter<W> {
    /// Start a document: writes the version string.
    pub fn new(w: W) -> io::Result<Self> {
        let mut t = Self { w, bytes: 0 };
        t.str(VERSION)?;
        Ok(t)
    }
    pub fn into_inner(self) -> W {
        self.w
    }
    fn put(&mut self, b: &[u8]) -> io::Result<()> {
        self.bytes += b.len() as u64;
        self.w.write_all(b)
    }
    fn u8(&mut self, v: u8) -> io::Result<()> {
        self.put(&[v])
    }
    fn u32(&mut self, v: u32) -> io::Result<()> {
        self.put(&v.to_le_bytes())
    }
    /// TSON lengths are `u32`. Both limits below are reachable from a user's file (a huge
    /// cohort, or a channel name copied straight out of TEXT), and the release profile is
    /// `panic = abort`, so a panic here would kill the container with no message. They are
    /// errors instead.
    fn len(&mut self, n: usize) -> io::Result<()> {
        if n > u32::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "TSON list of {n} exceeds the format's u32 limit ({}); \
                     split the import or use which.lines",
                    u32::MAX
                ),
            ));
        }
        self.u32(n as u32)
    }
    /// TSON strings are NUL-terminated, so a NUL inside one would truncate it and shift every
    /// following value. Channel names come from the file, so this is checked in release too.
    fn cstr(&mut self, s: &str) -> io::Result<()> {
        if s.as_bytes().contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("string contains a NUL byte and cannot be written as TSON: {s:?}"),
            ));
        }
        self.put(s.as_bytes())?;
        self.u8(0)
    }
    pub fn null(&mut self) -> io::Result<()> {
        self.u8(NULL_TYPE)
    }
    pub fn str(&mut self, s: &str) -> io::Result<()> {
        self.u8(STRING_TYPE)?;
        self.cstr(s)
    }
    pub fn i32(&mut self, v: i32) -> io::Result<()> {
        self.u8(INTEGER_TYPE)?;
        self.put(&v.to_le_bytes())
    }
    pub fn f64(&mut self, v: f64) -> io::Result<()> {
        self.u8(DOUBLE_TYPE)?;
        self.put(&v.to_le_bytes())
    }
    pub fn bool(&mut self, v: bool) -> io::Result<()> {
        self.u8(BOOL_TYPE)?;
        self.u8(v as u8)
    }
    /// Begin a list of `n` values; the caller then writes exactly `n` values.
    pub fn list(&mut self, n: usize) -> io::Result<()> {
        self.u8(LIST_TYPE)?;
        self.len(n)
    }
    /// Begin a map of `n` entries; the caller then writes `n` × (`key`, value).
    pub fn map(&mut self, n: usize) -> io::Result<()> {
        self.u8(MAP_TYPE)?;
        self.len(n)
    }
    /// Map key (a plain string, same encoding as a string value).
    pub fn key(&mut self, k: &str) -> io::Result<()> {
        self.str(k)
    }
    /// Begin an f64 typed list of `n` elements; follow with `f64_chunk` calls totalling `n`.
    pub fn f64_list_header(&mut self, n: usize) -> io::Result<()> {
        self.u8(LIST_FLOAT64_TYPE)?;
        self.len(n)
    }
    pub fn f64_chunk(&mut self, v: &[f64]) -> io::Result<()> {
        if cfg!(target_endian = "little") {
            // SAFETY: f64 has no padding; reinterpreting as bytes is sound and LE matches TSON.
            let bytes = unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 8) };
            self.put(bytes)
        } else {
            for x in v {
                self.put(&x.to_le_bytes())?;
            }
            Ok(())
        }
    }
    pub fn f64_list(&mut self, v: &[f64]) -> io::Result<()> {
        self.f64_list_header(v.len())?;
        self.f64_chunk(v)
    }
    pub fn i32_list_header(&mut self, n: usize) -> io::Result<()> {
        self.u8(LIST_INT32_TYPE)?;
        self.len(n)
    }
    pub fn i32_chunk(&mut self, v: &[i32]) -> io::Result<()> {
        if cfg!(target_endian = "little") {
            // SAFETY: i32 has no padding; reinterpreting as bytes is sound and LE matches TSON.
            let bytes = unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 4) };
            self.put(bytes)
        } else {
            for x in v {
                self.put(&x.to_le_bytes())?;
            }
            Ok(())
        }
    }
    /// Append already-encoded little-endian values to the current list.
    ///
    /// Used to pour a column that was spilled to a temporary file into the result without
    /// decoding it again: the spill file holds exactly the bytes TSON wants.
    pub fn raw(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.put(bytes)
    }
    pub fn i32_list(&mut self, v: &[i32]) -> io::Result<()> {
        self.i32_list_header(v.len())?;
        self.i32_chunk(v)
    }
    /// String list: byte count, then each string NUL-terminated.
    pub fn str_list<S: AsRef<str>>(&mut self, v: &[S]) -> io::Result<()> {
        self.u8(LIST_STRING_TYPE)?;
        let n: usize = v.iter().map(|s| s.as_ref().len() + 1).sum();
        self.len(n)?;
        for s in v {
            self.cstr(s.as_ref())?;
        }
        Ok(())
    }
    /// Same as `str_list` but with a generator, for very long repeated columns (no Vec<String>).
    pub fn str_list_iter<'a, I>(&mut self, n_bytes: usize, it: I) -> io::Result<()>
    where
        I: Iterator<Item = &'a str>,
    {
        self.u8(LIST_STRING_TYPE)?;
        self.len(n_bytes)?;
        for s in it {
            self.cstr(s)?;
        }
        Ok(())
    }
    pub fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_rustson_encoding() {
        // Build the same value with rustson and with the streaming writer; bytes must agree.
        use rustson::Value as V;
        let mut m = std::collections::HashMap::new();
        m.insert("kind".to_string(), V::STR("Table".into()));
        let v = V::MAP(m);
        let expect = rustson::encode(&v).unwrap();
        let mut buf = Vec::new();
        {
            let mut w = TsonWriter::new(&mut buf).unwrap();
            w.map(1).unwrap();
            w.key("kind").unwrap();
            w.str("Table").unwrap();
        }
        assert_eq!(buf, expect);

        let v = V::LST(vec![
            V::LSTF64(vec![1.5, -2.0]),
            V::LSTI32(vec![7]),
            V::I32(3),
            V::F64(0.25),
            V::BOOL(true),
            V::NULL,
            V::LSTSTR(vec!["a".to_string(), "bc".to_string()].into()),
        ]);
        let expect = rustson::encode(&v).unwrap();
        let mut buf = Vec::new();
        {
            let mut w = TsonWriter::new(&mut buf).unwrap();
            w.list(7).unwrap();
            w.f64_list(&[1.5, -2.0]).unwrap();
            w.i32_list(&[7]).unwrap();
            w.i32(3).unwrap();
            w.f64(0.25).unwrap();
            w.bool(true).unwrap();
            w.null().unwrap();
            w.str_list(&["a", "bc"]).unwrap();
        }
        assert_eq!(buf, expect);
        // and it round-trips through the rustson decoder
        let back = rustson::decode_bytes(&buf).unwrap();
        assert_eq!(rustson::encode(&back).unwrap(), expect);
    }
}
