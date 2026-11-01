//! Détection et (dé)compression des flux : gzip, xz, zstd, bzip2, lzma.
//! Stream (de)compression helpers.

use anyhow::{bail, Result};
use std::io::{BufRead, BufReader, Read, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    None,
    Gzip,
    Xz,
    Zstd,
    Bzip2,
    Lzma,
}

impl Codec {
    /// Devine la compression d'après les premiers octets.
    pub fn sniff(head: &[u8]) -> Codec {
        if head.starts_with(&[0x1f, 0x8b]) {
            Codec::Gzip
        } else if head.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]) {
            Codec::Xz
        } else if head.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
            Codec::Zstd
        } else if head.starts_with(b"BZh") {
            Codec::Bzip2
        } else if head.len() >= 3 && head[0] == 0x5d && head[1] == 0x00 && head[2] == 0x00 {
            Codec::Lzma
        } else {
            Codec::None
        }
    }

    /// Devine la compression d'après une extension de nom (ex. « data.tar.xz »).
    pub fn from_name(name: &str) -> Codec {
        let n = name.trim_end_matches('/');
        if n.ends_with(".gz") || n.ends_with(".tgz") {
            Codec::Gzip
        } else if n.ends_with(".xz") || n.ends_with(".txz") {
            Codec::Xz
        } else if n.ends_with(".zst") || n.ends_with(".zstd") || n.ends_with(".tzst") {
            Codec::Zstd
        } else if n.ends_with(".bz2") || n.ends_with(".tbz2") {
            Codec::Bzip2
        } else if n.ends_with(".lzma") {
            Codec::Lzma
        } else {
            Codec::None
        }
    }

    pub fn rpm_name(self) -> &'static str {
        match self {
            Codec::Gzip => "gzip",
            Codec::Xz => "xz",
            Codec::Zstd => "zstd",
            Codec::Bzip2 => "bzip2",
            Codec::Lzma => "lzma",
            Codec::None => "",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Codec::Gzip => ".gz",
            Codec::Xz => ".xz",
            Codec::Zstd => ".zst",
            Codec::Bzip2 => ".bz2",
            Codec::Lzma => ".lzma",
            Codec::None => "",
        }
    }
}

/// Enveloppe un lecteur dans le bon décompresseur (détection automatique).
pub fn auto_decoder<'a, R: Read + 'a>(r: R) -> Result<Box<dyn Read + 'a>> {
    let mut br = BufReader::with_capacity(256 * 1024, r);
    let head = br.fill_buf()?.to_vec();
    decoder(Codec::sniff(&head), br)
}

pub fn decoder<'a, R: BufRead + 'a>(codec: Codec, r: R) -> Result<Box<dyn Read + 'a>> {
    Ok(match codec {
        Codec::None => Box::new(r),
        Codec::Gzip => Box::new(flate2::bufread::MultiGzDecoder::new(r)),
        Codec::Xz => Box::new(xz2::bufread::XzDecoder::new_multi_decoder(r)),
        Codec::Zstd => Box::new(zstd::stream::read::Decoder::with_buffer(r)?),
        Codec::Bzip2 => Box::new(bzip2::bufread::MultiBzDecoder::new(r)),
        Codec::Lzma => {
            let stream = xz2::stream::Stream::new_lzma_decoder(u64::MAX)?;
            Box::new(xz2::bufread::XzDecoder::new_stream(r, stream))
        }
    })
}

/// Un compresseur qu'il faut terminer explicitement avec `finish()`.
pub trait Finish: Write {
    fn finish_box(self: Box<Self>) -> Result<()>;
}

struct GzW<W: Write>(flate2::write::GzEncoder<W>);
impl<W: Write> Write for GzW<W> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}
impl<W: Write> Finish for GzW<W> {
    fn finish_box(self: Box<Self>) -> Result<()> {
        let mut w = self.0.finish()?;
        w.flush()?;
        Ok(())
    }
}

struct XzW<W: Write>(xz2::write::XzEncoder<W>);
impl<W: Write> Write for XzW<W> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}
impl<W: Write> Finish for XzW<W> {
    fn finish_box(self: Box<Self>) -> Result<()> {
        let mut w = self.0.finish()?;
        w.flush()?;
        Ok(())
    }
}

struct ZstdW<'a, W: Write>(zstd::stream::write::Encoder<'a, W>);
impl<W: Write> Write for ZstdW<'_, W> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}
impl<W: Write> Finish for ZstdW<'_, W> {
    fn finish_box(self: Box<Self>) -> Result<()> {
        let mut w = self.0.finish()?;
        w.flush()?;
        Ok(())
    }
}

struct PlainW<W: Write>(W);
impl<W: Write> Write for PlainW<W> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}
impl<W: Write> Finish for PlainW<W> {
    fn finish_box(mut self: Box<Self>) -> Result<()> {
        self.0.flush()?;
        Ok(())
    }
}

fn threads() -> u32 {
    std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1).clamp(1, 8)
}

/// Crée un compresseur. `level` : None = niveau par défaut raisonnable.
pub fn encoder<'a, W: Write + 'a>(codec: Codec, w: W, level: Option<u32>) -> Result<Box<dyn Finish + 'a>> {
    Ok(match codec {
        Codec::None => Box::new(PlainW(w)),
        Codec::Gzip => Box::new(GzW(flate2::write::GzEncoder::new(w, flate2::Compression::new(level.unwrap_or(9))))),
        Codec::Xz => {
            let preset = level.unwrap_or(6);
            let stream = xz2::stream::MtStreamBuilder::new()
                .preset(preset)
                .threads(threads())
                .check(xz2::stream::Check::Crc64)
                .encoder()
                .or_else(|_| xz2::stream::Stream::new_easy_encoder(preset, xz2::stream::Check::Crc64))?;
            Box::new(XzW(xz2::write::XzEncoder::new_stream(w, stream)))
        }
        Codec::Zstd => {
            let mut enc = zstd::stream::write::Encoder::new(w, level.unwrap_or(12) as i32)?;
            let _ = enc.multithread(threads());
            enc.include_checksum(true)?;
            Box::new(ZstdW(enc))
        }
        Codec::Bzip2 | Codec::Lzma => bail!("compression {codec:?} non prise en charge en écriture"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all() {
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        for c in [Codec::Gzip, Codec::Xz, Codec::Zstd, Codec::None] {
            let mut out = Vec::new();
            {
                let mut e = encoder(c, &mut out, None).unwrap();
                e.write_all(&data).unwrap();
                e.finish_box().unwrap();
            }
            assert_eq!(Codec::sniff(&out), c);
            let mut back = Vec::new();
            auto_decoder(&out[..]).unwrap().read_to_end(&mut back).unwrap();
            assert_eq!(back, data, "{c:?}");
        }
    }
}
