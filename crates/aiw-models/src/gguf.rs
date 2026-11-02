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
    /// Estimated f16 KV cache for `ctx` tokens, when the header carries the attention shape.
    pub fn kv_cache_bytes(&self, ctx: u64) -> Option<u64> {
        let a = self.architecture()?;
        let layers = self.u64(&format!("{a}.block_count"))?;
        let heads = self.u64(&format!("{a}.attention.head_count"))?;
        let kv_heads = self.u64(&format!("{a}.attention.head_count_kv")).unwrap_or(heads);
        let head_dim = match self.u64(&format!("{a}.attention.key_length")) {
            Some(d) => d,
            None => self.u64(&format!("{a}.embedding_length"))?.checked_div(heads)?,
        };
        // K and V, 2 bytes per f16 element.
        layers.checked_mul(kv_heads)?.checked_mul(head_dim)?.checked_mul(ctx)?.checked_mul(4)
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

fn invalid(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())
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
                    // A corrupted header can carry any length: never overflow, never seek backwards.
                    let bytes =
                        (sz as u64).checked_mul(len).filter(|b| *b <= i64::MAX as u64).ok_or_else(|| invalid("array length overflows"))?;
                    r.seek(SeekFrom::Current(bytes as i64))?;
                }
                None if et == 8 => {
                    for _ in 0..len {
                        let n = u64le(r)?;
                        let n = i64::try_from(n).map_err(|_| invalid("string length overflows"))?;
                        r.seek(SeekFrom::Current(n))?;
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

    /// One key holding an array of `elem_type` with a corrupted `len`.
    fn corrupted(elem_type: u32, len: u64) -> Vec<u8> {
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend(1u64.to_le_bytes());
        s(&mut b, "tokenizer.ggml.scores");
        b.extend(9u32.to_le_bytes());
        b.extend(elem_type.to_le_bytes());
        b.extend(len.to_le_bytes());
        b
    }

    #[test]
    fn corrupted_array_length_is_an_error_not_a_panic() {
        // f32 elements: 4 * u64::MAX overflows, and any huge length must not wrap to a negative seek.
        for len in [u64::MAX, u64::MAX / 4 + 1, i64::MAX as u64, 1 << 62] {
            assert!(read_gguf_from(&mut Cursor::new(corrupted(6, len))).is_err(), "len {len}");
        }
        // Strings with an absurd per-item length.
        let mut b = corrupted(8, 2);
        b.extend(u64::MAX.to_le_bytes());
        assert!(read_gguf_from(&mut Cursor::new(b)).is_err());
        // A plausible length that runs past the end of the file is an error as well.
        assert!(read_gguf_from(&mut Cursor::new(corrupted(6, 1_000_000))).is_ok_and(|i| i.metadata.len() == 1));
    }

    #[test]
    fn estimates_kv_cache_from_header() {
        let mut i = GgufInfo::default();
        i.metadata.insert("general.architecture".into(), GgufValue::Str("llama".into()));
        i.metadata.insert("llama.block_count".into(), GgufValue::U64(32));
        i.metadata.insert("llama.attention.head_count".into(), GgufValue::U64(32));
        i.metadata.insert("llama.attention.head_count_kv".into(), GgufValue::U64(8));
        i.metadata.insert("llama.embedding_length".into(), GgufValue::U64(4096));
        // 32 layers * 8 kv heads * 128 dim * 4096 tokens * 4 bytes (K+V, f16) = 512 MiB
        assert_eq!(i.kv_cache_bytes(4096), Some(512 << 20));
        i.metadata.remove("llama.block_count");
        assert_eq!(i.kv_cache_bytes(4096), None);
    }
}
