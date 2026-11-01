//! Lecture / écriture d'archives tar depuis / vers l'arbre virtuel.
//! Tar helpers for the virtual tree.

use crate::tree::{normalize, Entry, Kind, Tree};
use anyhow::{Context, Result};
use std::io::{Read, Write};

/// Ce qu'on fait d'une entrée tar lue.
pub enum Sink {
    /// Mettre dans l'arbre des fichiers installés.
    Tree,
    /// Garder à part (fichier de contrôle : .PKGINFO, control…), avec son contenu.
    Keep,
    /// Ignorer.
    Skip,
}

/// Lit une archive tar. `classify` décide du sort de chaque chemin normalisé.
/// Retourne les fichiers « Keep » (chemin, contenu) et le nombre d'entrées spéciales ignorées.
pub fn read_tar_into(
    r: &mut dyn Read,
    tree: &mut Tree,
    mut classify: impl FnMut(&str) -> Sink,
) -> Result<(Vec<(String, Vec<u8>)>, usize)> {
    let mut ar = tar::Archive::new(r);
    let mut kept = Vec::new();
    let mut skipped_special = 0usize;
    for ent in ar.entries().context("lecture de l'archive tar")? {
        let mut ent = ent.context("entrée tar illisible")?;
        let raw = String::from_utf8_lossy(&ent.path_bytes()).to_string();
        let Some(path) = normalize(&raw) else { continue };
        let h = ent.header();
        let et = h.entry_type();
        match classify(&path) {
            Sink::Skip => continue,
            Sink::Keep => {
                if et.is_file() {
                    let mut data = Vec::new();
                    ent.read_to_end(&mut data)?;
                    kept.push((path, data));
                }
                continue;
            }
            Sink::Tree => {}
        }
        let mode = h.mode().unwrap_or(0o644) & 0o7777;
        let uid = h.uid().unwrap_or(0) as u32;
        let gid = h.gid().unwrap_or(0) as u32;
        let user = h.username().ok().flatten().unwrap_or("").to_string();
        let group = h.groupname().ok().flatten().unwrap_or("").to_string();
        let mtime = h.mtime().unwrap_or(0);
        let mut entry = if et.is_dir() {
            Entry::dir(mode, mtime)
        } else if et.is_symlink() {
            let target = ent
                .link_name_bytes()
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .unwrap_or_default();
            Entry::symlink(&target, mtime)
        } else if et.is_hard_link() {
            let target = ent.link_name_bytes().map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
            let Some(tp) = normalize(&target) else { continue };
            match tree.get(&tp).map(|e| e.kind.clone()) {
                Some(Kind::File(b)) => Entry::file(b, mode, mtime),
                _ => {
                    skipped_special += 1;
                    continue;
                }
            }
        } else if et.is_file() || et == tar::EntryType::Continuous {
            let blob = tree.store.add_reader(&mut ent)?;
            Entry::file(blob, mode, mtime)
        } else {
            // périphériques, fifo, en-têtes pax isolés…
            if !et.is_pax_global_extensions() && !et.is_pax_local_extensions() && !et.is_gnu_longname() && !et.is_gnu_longlink() {
                skipped_special += 1;
            }
            continue;
        };
        if entry.is_symlink() {
            entry.mode = 0o777;
        }
        entry.uid = uid;
        entry.gid = gid;
        entry.user = user;
        entry.group = group;
        tree.insert(&path, entry);
    }
    Ok((kept, skipped_special))
}

/// Options d'écriture d'un tar.
pub struct TarOpts {
    /// Préfixer les chemins par « ./ » (style Debian).
    pub dot_prefix: bool,
    /// Ajouter une entrée « ./ » pour la racine.
    pub root_entry: bool,
    /// Forcer root:root (uid/gid 0) quand le propriétaire est inconnu.
    pub numeric_owner_fallback: bool,
}

fn header_for(e: &Entry, opts: &TarOpts) -> tar::Header {
    let mut h = tar::Header::new_gnu();
    h.set_mode(e.mode & 0o7777);
    h.set_mtime(e.mtime);
    let (uid, gid) = if opts.numeric_owner_fallback { (e.uid, e.gid) } else { (e.uid, e.gid) };
    h.set_uid(uid as u64);
    h.set_gid(gid as u64);
    let _ = h.set_username(e.user_name());
    let _ = h.set_groupname(e.group_name());
    h
}

/// Ajoute une entrée de l'arbre au tar.
pub fn append_entry<W: Write>(b: &mut tar::Builder<W>, tree: &Tree, path: &str, e: &Entry, opts: &TarOpts) -> Result<()> {
    let name = if opts.dot_prefix { format!("./{path}") } else { path.to_string() };
    let mut h = header_for(e, opts);
    match &e.kind {
        Kind::Dir => {
            h.set_entry_type(tar::EntryType::Directory);
            h.set_size(0);
            b.append_data(&mut h, format!("{name}/"), std::io::empty())?;
        }
        Kind::Symlink(t) => {
            h.set_entry_type(tar::EntryType::Symlink);
            h.set_size(0);
            b.append_link(&mut h, &name, t)?;
        }
        Kind::File(blob) => {
            h.set_entry_type(tar::EntryType::Regular);
            h.set_size(blob.size);
            let f = tree.store.open(blob)?;
            b.append_data(&mut h, &name, f)?;
        }
    }
    Ok(())
}

/// Écrit l'entrée racine « ./ ».
pub fn append_root<W: Write>(b: &mut tar::Builder<W>, mtime: u64) -> Result<()> {
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Directory);
    h.set_mode(0o755);
    h.set_mtime(mtime);
    h.set_uid(0);
    h.set_gid(0);
    let _ = h.set_username("root");
    let _ = h.set_groupname("root");
    h.set_size(0);
    b.append_data(&mut h, "./", std::io::empty())?;
    Ok(())
}

/// Ajoute un fichier en mémoire (fichier de contrôle).
pub fn append_bytes<W: Write>(b: &mut tar::Builder<W>, name: &str, data: &[u8], mode: u32, mtime: u64) -> Result<()> {
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Regular);
    h.set_mode(mode);
    h.set_mtime(mtime);
    h.set_uid(0);
    h.set_gid(0);
    let _ = h.set_username("root");
    let _ = h.set_groupname("root");
    h.set_size(data.len() as u64);
    b.append_data(&mut h, name, data)?;
    Ok(())
}

/// Heure actuelle (secondes Unix).
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
