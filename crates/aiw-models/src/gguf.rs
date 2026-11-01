//! Minimal GGUF (v2/v3) metadata reader: reads the key/value header and skips large arrays
//! (tokenizer vocabularies) without loading them.

use std::collections::BTreeMap;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum GgufValue {
    U64(u64),
    I64(i64),
    F64(f64),
    Bool(bool),
    Str(String),
    /// Arrays are summarised: element type id and length.
    Array {
        elem_type: u32,
        len: u64,
    },
}

#[derive(Debug, Clone, Default)]
pub struct GgufInfo {
    pub version: u32,
    pub tensor_count: u64,
    pub metadata: BTreeMap<String, GgufValue>,
}

impl GgufInfo {
    pub fn str(&self, k: &str) -> Option<&str> {
        match self.metadata.get(k) {
            Some(GgufValue::Str(s)) => Some(s),
            _ => None,
        }
    }
    pub fn u64(&self, k: &str) -> Option<u64> {
        match self.metadata.get(k) {
            Some(GgufValue::U64(v)) => Some(*v),
            Some(GgufValue::I64(v)) if *v >= 0 => Some(*v as u64),
            _ => None,
        }
    }
    pub fn architecture(&self) -> Option<&str> {
        self.str("general.architecture")
    }
    pub fn context_length(&self) -> Option<u64> {
        self.architecture().and_then(|a| self.u64(&format!("{a}.context_length")))
    }
    pub fn quant(&self) -> Option<&'static str> {
        self.u64("general.file_type").and_then(ftype_name)
    }
    pub fn has_vision(&self) -> bool {
        self.metadata.keys().any(|k| k.starts_with("clip.") || k.contains("vision"))
    }
}

/// llama.cpp `llama_ftype` → conventional quantization label.
pub fn ftype_name(t: u64) -> Option<&'static str> {
    Some(match t {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        7 => "Q8_0",
        8 => "Q5_0",
        9 => "Q5_1",
        10 => "Q2_K",
        11 => "Q3_K_S",
        12 => "Q3_K_M",
        13 => "Q3_K_L",
        14 => "Q4_K_S",
        15 => "Q4_K_M",
        16 => "Q5_K_S",
        17 => "Q5_K_M",
        18 => "Q6_K",
        19 => "IQ2_XXS",
        20 => "IQ2_XS",
        21 => "Q2_K_S",
        22 => "IQ3_XS",
        23 => "IQ3_XXS",
        24 => "IQ1_S",
        25 => "IQ4_NL",
        26 => "IQ3_S",
        27 => "IQ3_M",
        28 => "IQ2_S",
        29 => "IQ2_M",
        30 => "IQ4_XS",
        31 => "IQ1_M",
        32 => "BF16",
        36 => "TQ1_0",
        37 => "TQ2_0",
        38 => "MXFP4",
        _ => return None,
    })
}

fn rd<const N: usize, R: Read>(r: &mut R) -> std::io::Result<[u8; N]> {
    let mut b = [0u8; N];
    r.read_exact(&mut b)?;
    Ok(b)
}
fn u32le<R: Read>(r: &mut R) -> std::io::Result<u32> {
    Ok(u32::from_le_bytes(rd::<4, _>(r)?))
}
fn u64le<R: Read>(r: &mut R) -> std::io::Result<u64> {
    Ok(u64::from_le_bytes(rd::<8, _>(r)?))
}
fn string<R: Read>(r: &mut R) -> std::io::Result<String> {
    let n = u64le(r)?;
    if n > 1 << 24 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "string too long"));
    }
    let mut b = vec![0u8; n as usize];
    r.read_exact(&mut b)?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

fn scalar_size(t: u32) -> Option<i64> {
    Some(match t {
        0 | 1 | 7 => 1,
        2 | 3 => 2,
        4..=6 => 4,
        10..=12 => 8,
        _ => return None,
    })
}

fn value<R: Read + Seek>(r: &mut R, t: u32) -> std::io::Result<GgufValue> {
    Ok(match t {
        0 => GgufValue::U64(rd::<1, _>(r)?[0] as u64),
        1 => GgufValue::I64(rd::<1, _>(r)?[0] as i8 as i64),
        2 => GgufValue::U64(u16::from_le_bytes(rd::<2, _>(r)?) as u64),
        3 => GgufValue::I64(i16::from_le_bytes(rd::<2, _>(r)?) as i64),
        4 => GgufValue::U64(u32le(r)? as u64),
        5 => GgufValue::I64(i32::from_le_bytes(rd::<4, _>(r)?) as i64),
        6 => GgufValue::F64(f32::from_le_bytes(rd::<4, _>(r)?) as f64),
        7 => GgufValue::Bool(rd::<1, _>(r)?[0] != 0),
        8 => GgufValue::Str(string(r)?),
        9 => {
            let et = u32le(r)?;
            let len = u64le(r)?;
            match scalar_size(et) {
                Some(sz) => {
                    r.seek(SeekFrom::Current(sz * len as i64))?;
                }
                None if et == 8 => {
                    for _ in 0..len {
                        let n = u64le(r)?;
                        r.seek(SeekFrom::Current(n as i64))?;
                    }
                }
                None => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "nested arrays unsupported")),
            }
            GgufValue::Array { elem_type: et, len }
        }
        10 => GgufValue::U64(u64le(r)?),
        11 => GgufValue::I64(i64::from_le_bytes(rd::<8, _>(r)?)),
        12 => GgufValue::F64(f64::from_le_bytes(rd::<8, _>(r)?)),
        other => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("unknown GGUF type {other}"))),
    })
}

pub fn read_gguf(path: &Path) -> std::io::Result<GgufInfo> {
    let mut r = BufReader::with_capacity(1 << 16, std::fs::File::open(path)?);
    read_gguf_from(&mut r)
}

pub fn read_gguf_from<R: Read + Seek>(r: &mut R) -> std::io::Result<GgufInfo> {
    if &rd::<4, _>(r)? != b"GGUF" {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "not a GGUF file"));
    }
    let version = u32le(r)?;
    if !(2..=3).contains(&version) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("unsupported GGUF version {version}")));
    }
    let tensor_count = u64le(r)?;
    let kv = u64le(r)?;
    let mut metadata = BTreeMap::new();
    for _ in 0..kv.min(100_000) {
        let k = string(r)?;
        let t = u32le(r)?;
        let v = value(r, t)?;
        metadata.insert(k, v);
    }
    Ok(GgufInfo { version, tensor_count, metadata })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    fn s(buf: &mut Vec<u8>, x: &str) {
        buf.extend((x.len() as u64).to_le_bytes());
        buf.extend(x.as_bytes());
    }

    /// Builds a tiny synthetic GGUF header.
    pub fn synthetic() -> Vec<u8> {
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend(5u64.to_le_bytes());
        s(&mut b, "general.architecture");
        b.extend(8u32.to_le_bytes());
        s(&mut b, "qwen3");
        s(&mut b, "general.name");
        b.extend(8u32.to_le_bytes());
        s(&mut b, "Qwen3 Test");
        s(&mut b, "qwen3.context_length");
        b.extend(4u32.to_le_bytes());
        b.extend(40960u32.to_le_bytes());
        s(&mut b, "tokenizer.ggml.tokens");
        b.extend(9u32.to_le_bytes());
        b.extend(8u32.to_le_bytes());
        b.extend(3u64.to_le_bytes());
        for t in ["a", "bb", "ccc"] {
            s(&mut b, t);
        }
        s(&mut b, "general.file_type");
        b.extend(4u32.to_le_bytes());
        b.extend(15u32.to_le_bytes());
        b
    }

    #[test]
    fn reads_synthetic_header_and_skips_arrays() {
        let info = read_gguf_from(&mut Cursor::new(synthetic())).unwrap();
        assert_eq!(info.version, 3);
        assert_eq!(info.architecture(), Some("qwen3"));
        assert_eq!(info.context_length(), Some(40960));
        assert_eq!(info.quant(), Some("Q4_K_M"));
        assert_eq!(info.metadata["tokenizer.ggml.tokens"], GgufValue::Array { elem_type: 8, len: 3 });
    }

    #[test]
    fn rejects_non_gguf() {
        assert!(read_gguf_from(&mut Cursor::new(b"NOPE....".to_vec())).is_err());
    }
}
