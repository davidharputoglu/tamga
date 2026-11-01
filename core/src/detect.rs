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
    /// Archive .zip ou .rar (pas un paquet Linux).
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

/// Ce que contient une archive tar (compressée ou non).
#[derive(Default)]
struct TarScan {
    is_tar: bool,
    pkginfo: bool,
    slack: bool,
}

fn scan_tar(path: &Path) -> TarScan {
    let mut s = TarScan::default();
    let Ok(f) = File::open(path) else { return s };
    let Ok(dec) = compress::auto_decoder(BufReader::new(f)) else { return s };
    let mut ar = tar::Archive::new(dec);
    let Ok(entries) = ar.entries() else { return s };
    for e in entries.take(3000) {
        let Ok(e) = e else { break };
        s.is_tar = true;
        let p = String::from_utf8_lossy(&e.path_bytes()).to_string();
        let p = p.trim_start_matches("./");
        if p == ".PKGINFO" {
            s.pkginfo = true;
            break;
        }
        if p == "install/slack-desc" || p == "install/doinst.sh" {
            s.slack = true;
            break;
        }
    }
    s
}

/// Décompresse le début d'un fichier (pour reconnaître un paquet Solaris .pkg.gz…).
fn decompressed_head(path: &Path, n: usize) -> Vec<u8> {
    let Ok(f) = File::open(path) else { return vec![] };
    let Ok(dec) = compress::auto_decoder(BufReader::new(f)) else { return vec![] };
    let mut buf = Vec::new();
    let _ = dec.take(n as u64).read_to_end(&mut buf);
    buf
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
    if crate::svr4::is_datastream(&head) {
        return Detected::Package(Format::Solaris);
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
    // Stampede : archive bzip2/gzip suivie d'un bloc d'informations.
    if name.ends_with(".slp") && crate::slp::looks_like_slp(path) {
        return Detected::Package(Format::Slp);
    }
    let compressed = Codec::sniff(&head) != Codec::None;
    let plain_tar = head.len() > 262 && &head[257..262] == b"ustar";
    if compressed || plain_tar {
        if compressed && crate::svr4::is_datastream(&decompressed_head(path, 64)) {
            return Detected::Package(Format::Solaris);
        }
        if name.ends_with(".pacman") || name.contains(".pkg.tar") {
            return Detected::Package(Format::Arch);
        }
        let scan = scan_tar(path);
        if scan.pkginfo {
            return Detected::Package(Format::Arch);
        }
        if scan.slack {
            return Detected::Package(Format::Slack);
        }
        if scan.is_tar {
            let base = crate::slack::strip_tar_ext(&name);
            let slack_ext = crate::slack::SLACK_EXTS.iter().any(|e| name.ends_with(e));
            if slack_ext && crate::slack::parse_slack_name(base).is_some() {
                return Detected::Package(Format::Slack);
            }
            return Detected::Package(Format::Tar);
        }
        if name.ends_with(".zip") || name.ends_with(".rar") {
            return Detected::Archive;
        }
        return Detected::Unknown;
    }
    // Vieux tar sans signature « ustar » (format v7) : on essaie de le lire.
    if crate::slack::TAR_EXTS.iter().any(|e| name.ends_with(e)) && scan_tar(path).is_tar {
        return Detected::Package(Format::Tar);
    }
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"Rar!") || name.ends_with(".zip") || name.ends_with(".rar") {
        return Detected::Archive;
    }
    Detected::Unknown
}
