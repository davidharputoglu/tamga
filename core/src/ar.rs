//! Lecture et écriture des archives « ar » (enveloppe des paquets .deb).
//! Minimal `ar` archive reader/writer (outer container of .deb files).

use anyhow::{bail, Context, Result};
use std::io::{Read, Seek, SeekFrom, Write};

pub const MAGIC: &[u8; 8] = b"!<arch>\n";

#[derive(Debug, Clone)]
pub struct Member {
    pub name: String,
    /// Position du contenu dans le fichier.
    pub offset: u64,
    pub size: u64,
}

/// Liste les membres d'une archive ar (formats GNU et BSD courts).
pub fn list<R: Read + Seek>(r: &mut R) -> Result<Vec<Member>> {
    r.seek(SeekFrom::Start(0))?;
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic).context("archive ar trop courte")?;
    if &magic != MAGIC {
        bail!("ce fichier n'est pas une archive ar");
    }
    let mut members = Vec::new();
    let mut pos = 8u64;
    loop {
        let mut hdr = [0u8; 60];
        match r.read_exact(&mut hdr) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        if &hdr[58..60] != b"`\n" {
            bail!("en-tête ar invalide à la position {pos}");
        }
        let raw_name = String::from_utf8_lossy(&hdr[0..16]).trim_end().to_string();
        let size: u64 = String::from_utf8_lossy(&hdr[48..58]).trim().parse().context("taille ar invalide")?;
        let mut data_off = pos + 60;
        let mut data_size = size;
        let mut name = raw_name.trim_end_matches('/').to_string();
        // Nom long façon BSD : « #1/longueur » suivi du nom dans les données.
        if let Some(len) = raw_name.strip_prefix("#1/") {
            let len: u64 = len.trim().parse().unwrap_or(0);
            let mut n = vec![0u8; len as usize];
            r.read_exact(&mut n)?;
            name = String::from_utf8_lossy(&n).trim_end_matches('\0').to_string();
            data_off += len;
            data_size -= len;
        }
        if !name.is_empty() && name != "/" && name != "//" {
            members.push(Member { name, offset: data_off, size: data_size });
        }
        pos += 60 + size + (size % 2);
        r.seek(SeekFrom::Start(pos))?;
    }
    Ok(members)
}

/// Écrit une archive ar avec des membres en mémoire ou des fichiers.
pub struct Writer<W: Write> {
    w: W,
}

impl<W: Write> Writer<W> {
    pub fn new(mut w: W) -> Result<Self> {
        w.write_all(MAGIC)?;
        Ok(Writer { w })
    }

    fn header(&mut self, name: &str, size: u64, mtime: u64) -> Result<()> {
        if name.len() > 16 {
            bail!("nom de membre ar trop long : {name}");
        }
        let h = format!("{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", name, mtime, 0, 0, "100644", size);
        debug_assert_eq!(h.len(), 60);
        self.w.write_all(h.as_bytes())?;
        Ok(())
    }

    pub fn add_bytes(&mut self, name: &str, data: &[u8], mtime: u64) -> Result<()> {
        self.header(name, data.len() as u64, mtime)?;
        self.w.write_all(data)?;
        if data.len() % 2 == 1 {
            self.w.write_all(b"\n")?;
        }
        Ok(())
    }

    pub fn add_reader(&mut self, name: &str, r: &mut dyn Read, size: u64, mtime: u64) -> Result<()> {
        self.header(name, size, mtime)?;
        let copied = std::io::copy(r, &mut self.w)?;
        if copied != size {
            bail!("taille inattendue pour {name} : {copied} au lieu de {size}");
        }
        if size % 2 == 1 {
            self.w.write_all(b"\n")?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<W> {
        self.w.flush()?;
        Ok(self.w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrip() {
        let mut w = Writer::new(Vec::new()).unwrap();
        w.add_bytes("debian-binary", b"2.0\n", 0).unwrap();
        w.add_bytes("odd", b"abc", 0).unwrap();
        let buf = w.finish().unwrap();
        let mut c = Cursor::new(buf.clone());
        let m = list(&mut c).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].name, "debian-binary");
        assert_eq!(&buf[m[1].offset as usize..m[1].offset as usize + 3], b"abc");
    }
}
