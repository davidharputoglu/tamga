//! Reconnaissance du type de fichier (par son contenu, pas seulement son nom).
//! File type detection by content.

use crate::compress::{self, Codec};
use crate::model::Format;
use serde::Serialize;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// Résultat de la reconnaissance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum Detected {
    /// Paquet convertible.
    Package(Format),
    /// Script .sh qui n'a pas été créé par TAMGA.
    ForeignScript,
    /// Paquet source rpm.
    SourceRpm,
    /// AppImage (non pris en charge).
    AppImage,
    /// Archive simple (.tar.*, .zip, .rar) sans métadonnées de paquet.
    Archive,
    /// Format inconnu.
    Unknown,
}

fn read_head(path: &Path, n: usize) -> std::io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut buf = vec![0u8; n];
    let mut got = 0;
    while got < n {
        let k = f.read(&mut buf[got..])?;
        if k == 0 {
            break;
        }
        got += k;
    }
    buf.truncate(got);
    Ok(buf)
}

/// Le premier élément d'une archive tar compressée est-il .PKGINFO ?
fn tar_has_pkginfo(path: &Path) -> bool {
    let Ok(f) = File::open(path) else { return false };
    let Ok(dec) = compress::auto_decoder(BufReader::new(f)) else { return false };
    let mut ar = tar::Archive::new(dec);
    let Ok(entries) = ar.entries() else { return false };
    for e in entries.take(400) {
        let Ok(e) = e else { return false };
        let p = String::from_utf8_lossy(&e.path_bytes()).to_string();
        if p.trim_start_matches("./") == ".PKGINFO" {
            return true;
        }
    }
    false
}

pub fn detect(path: &Path) -> Detected {
    let Ok(head) = read_head(path, 64 * 1024) else { return Detected::Unknown };
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if head.starts_with(b"!<arch>\ndebian-binary") {
        return Detected::Package(Format::Deb);
    }
    if head.starts_with(&[0xed, 0xab, 0xee, 0xdb]) {
        if head.len() > 8 && head[6..8] == [0, 1] {
            return Detected::SourceRpm;
        }
        return Detected::Package(Format::Rpm);
    }
    if head.starts_with(b"\x7fELF") {
        if head.len() > 10 && &head[8..10] == b"AI" {
            return Detected::AppImage;
        }
        return Detected::Unknown;
    }
    if head.starts_with(b"#!") {
        if crate::shell::is_tamga_sh(&head) {
            return Detected::Package(Format::Sh);
        }
        return Detected::ForeignScript;
    }
    let arch_name = name.ends_with(".pacman") || name.contains(".pkg.tar");
    match Codec::sniff(&head) {
        Codec::None => {}
        _ if arch_name => return Detected::Package(Format::Arch),
        _ => {
            if tar_has_pkginfo(path) {
                return Detected::Package(Format::Arch);
            }
            return Detected::Archive;
        }
    }
    // tar non compressé : « ustar » à l'octet 257
    if head.len() > 262 && &head[257..262] == b"ustar" {
        if tar_has_pkginfo(path) {
            return Detected::Package(Format::Arch);
        }
        return Detected::Archive;
    }
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"Rar!") || name.ends_with(".zip") || name.ends_with(".rar") {
        return Detected::Archive;
    }
    Detected::Unknown
}
