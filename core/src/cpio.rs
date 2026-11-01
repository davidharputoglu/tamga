//! Format cpio « newc » (contenu des paquets .rpm).
//! cpio "newc" reader/writer used for RPM payloads.

use crate::tree::{normalize, Entry, Kind, Tree};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::io::{self, Read, Write};

const TRAILER: &str = "TRAILER!!!";

fn pad4(n: u64) -> u64 {
    (4 - (n % 4)) % 4
}

fn hex8(s: &[u8]) -> Result<u32> {
    let t = std::str::from_utf8(s)?;
    u32::from_str_radix(t, 16).with_context(|| format!("champ cpio invalide : {t:?}"))
}

/// Ignore exactement `n` octets.
fn skip(r: &mut dyn Read, n: u64) -> io::Result<()> {
    io::copy(&mut r.take(n), &mut io::sink())?;
    Ok(())
}

/// Infos d'un fichier tiré de l'archive (pour compléter avec l'en-tête rpm).
pub struct CpioItem {
    pub path: String,
    pub mode: u32,
    pub mtime: u64,
}

/// Lit une archive cpio newc dans l'arbre. Retourne la liste des éléments lus.
/// Les liens physiques (contenu porté par la dernière occurrence) sont gérés.
pub fn read_into(r: &mut dyn Read, tree: &mut Tree) -> Result<Vec<CpioItem>> {
    let mut items = Vec::new();
    // inode -> chemins en attente du contenu
    let mut pending: HashMap<u32, Vec<(String, u32, u64)>> = HashMap::new();
    loop {
        let mut h = [0u8; 110];
        r.read_exact(&mut h).context("archive cpio tronquée")?;
        if &h[0..6] != b"070701" && &h[0..6] != b"070702" {
            bail!("format cpio non pris en charge (newc attendu)");
        }
        let ino = hex8(&h[6..14])?;
        let mode = hex8(&h[14..22])?;
        let nlink = hex8(&h[38..46])?;
        let mtime = hex8(&h[46..54])? as u64;
        let filesize = hex8(&h[54..62])? as u64;
        let namesize = hex8(&h[94..102])? as u64;
        let mut name = vec![0u8; namesize as usize];
        r.read_exact(&mut name)?;
        skip(r, pad4(110 + namesize))?;
        let name = String::from_utf8_lossy(&name).trim_end_matches('\0').to_string();
        if name == TRAILER {
            break;
        }
        let ftype = mode & 0o170000;
        let perm = mode & 0o7777;
        let path = normalize(&name);
        match (ftype, path) {
            (0o040000, Some(p)) => {
                tree.insert(&p, Entry::dir(perm, mtime));
                items.push(CpioItem { path: p, mode, mtime });
            }
            (0o120000, Some(p)) => {
                let mut target = vec![0u8; filesize as usize];
                r.read_exact(&mut target)?;
                let t = String::from_utf8_lossy(&target).to_string();
                tree.insert(&p, Entry::symlink(&t, mtime));
                items.push(CpioItem { path: p, mode, mtime });
            }
            (0o100000, Some(p)) => {
                if filesize == 0 && nlink > 1 {
                    // Lien physique : le contenu viendra avec une autre occurrence.
                    pending.entry(ino).or_default().push((p.clone(), perm, mtime));
                } else {
                    let blob = tree.store.add_reader(&mut (&mut *r).take(filesize))?;
                    if blob.size != filesize {
                        bail!("archive cpio tronquée ({p})");
                    }
                    tree.insert(&p, Entry::file(blob.clone(), perm, mtime));
                    if nlink > 1 {
                        if let Some(others) = pending.remove(&ino) {
                            for (op, operm, omtime) in others {
                                tree.insert(&op, Entry::file(blob.clone(), operm, omtime));
                                items.push(CpioItem { path: op, mode: 0o100000 | operm, mtime: omtime });
                            }
                        }
                    }
                }
                items.push(CpioItem { path: p, mode, mtime });
            }
            _ => {
                // Fichiers spéciaux (périphériques, fifo) ou chemin vide : ignorés.
                skip(r, filesize)?;
            }
        }
        skip(r, pad4(filesize))?;
    }
    // Liens physiques sans contenu : fichiers vides.
    for (_, list) in pending {
        for (p, perm, mtime) in list {
            let blob = tree.store.add_bytes(&[])?;
            tree.insert(&p, Entry::file(blob, perm, mtime));
            items.push(CpioItem { path: p, mode: 0o100000 | perm, mtime });
        }
    }
    Ok(items)
}

/// Écrit une archive cpio newc. Retourne le nombre d'octets non compressés écrits.
pub struct Writer<W: Write> {
    w: W,
    written: u64,
    ino: u32,
}

impl<W: Write> Writer<W> {
    pub fn new(w: W) -> Self {
        Writer { w, written: 0, ino: 0 }
    }

    fn put(&mut self, b: &[u8]) -> Result<()> {
        self.w.write_all(b)?;
        self.written += b.len() as u64;
        Ok(())
    }

    fn header(&mut self, name: &str, mode: u32, nlink: u32, mtime: u64, size: u64, ino: u32) -> Result<()> {
        if size > u32::MAX as u64 {
            bail!("fichier trop gros pour le format cpio (> 4 Go) : {name}");
        }
        let namesize = name.len() as u64 + 1;
        let h = format!(
            "070701{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}",
            ino, mode, 0, 0, nlink, mtime as u32, size as u32, 0, 0, 0, 0, namesize as u32, 0
        );
        self.put(h.as_bytes())?;
        self.put(name.as_bytes())?;
        self.put(&[0])?;
        let pad = pad4(110 + namesize) as usize;
        self.put(&[0u8; 4][..pad])?;
        Ok(())
    }

    /// Ajoute une entrée de l'arbre sous le nom `./chemin`.
    pub fn add(&mut self, tree: &Tree, path: &str, e: &Entry) -> Result<()> {
        self.ino += 1;
        let ino = self.ino;
        let name = format!("./{path}");
        match &e.kind {
            Kind::Dir => self.header(&name, e.st_mode(), 2, e.mtime, 0, ino)?,
            Kind::Symlink(t) => {
                self.header(&name, e.st_mode(), 1, e.mtime, t.len() as u64, ino)?;
                self.put(t.as_bytes())?;
                let pad = pad4(t.len() as u64) as usize;
                self.put(&[0u8; 4][..pad])?;
            }
            Kind::File(b) => {
                self.header(&name, e.st_mode(), 1, e.mtime, b.size, ino)?;
                let mut f = tree.store.open(b)?;
                let n = io::copy(&mut f, &mut self.w)?;
                if n != b.size {
                    bail!("taille inattendue pour {path}");
                }
                self.written += n;
                let pad = pad4(b.size) as usize;
                self.put(&[0u8; 4][..pad])?;
            }
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<(W, u64)> {
        self.header(TRAILER, 0, 1, 0, 0, 0)?;
        // Alignement final sur 512 octets comme cpio(1) — sans incidence pour rpm.
        self.w.flush()?;
        Ok((self.w, self.written))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut t = Tree::new().unwrap();
        t.add_file_bytes("usr/bin/hello", b"#!/bin/sh\necho hi\n", 0o755, 1).unwrap();
        t.insert("usr/bin/hi", Entry::symlink("hello", 1));
        let mut buf = Vec::new();
        {
            let mut w = Writer::new(&mut buf);
            for (p, e) in t.entries.iter() {
                w.add(&t, p, e).unwrap();
            }
            w.finish().unwrap();
        }
        let mut t2 = Tree::new().unwrap();
        read_into(&mut &buf[..], &mut t2).unwrap();
        assert_eq!(t2.read("usr/bin/hello").unwrap(), b"#!/bin/sh\necho hi\n");
        assert!(matches!(&t2.get("usr/bin/hi").unwrap().kind, Kind::Symlink(s) if s == "hello"));
        assert_eq!(t2.get("usr/bin/hello").unwrap().mode, 0o755);
    }
}
