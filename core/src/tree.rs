//! Arbre de fichiers virtuel + stockage des contenus sur disque.
//! Virtual file tree + on-disk content store.
//!
//! Les métadonnées (chemins, droits, propriétaires, liens) restent en mémoire,
//! le contenu des fichiers est rangé dans un dossier temporaire. Ainsi les
//! droits « root », setuid, etc. sont conservés exactement, même si TAMGA
//! tourne en simple utilisateur, et la mémoire reste faible pour les gros paquets.

use anyhow::{bail, Context, Result};
use md5::{Digest as _, Md5};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Contenu d'un fichier stocké sur disque.
#[derive(Clone, Debug)]
pub struct Blob {
    pub id: u64,
    pub size: u64,
    pub md5: [u8; 16],
    pub sha256: [u8; 32],
}

impl Blob {
    pub fn md5_hex(&self) -> String {
        hex::encode(self.md5)
    }
    pub fn sha256_hex(&self) -> String {
        hex::encode(self.sha256)
    }
}

/// Dossier temporaire contenant les contenus des fichiers.
pub struct BlobStore {
    dir: tempfile::TempDir,
    next: AtomicU64,
}

impl BlobStore {
    pub fn new() -> Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("tamga-")
            .tempdir()
            .context("création du dossier temporaire")?;
        Ok(Self { dir, next: AtomicU64::new(0) })
    }

    pub fn dir(&self) -> &std::path::Path {
        self.dir.path()
    }

    /// Copie un flux dans le stockage en calculant les empreintes.
    pub fn add_reader(&self, r: &mut dyn Read) -> Result<Blob> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let path = self.dir.path().join(id.to_string());
        let mut out = BufWriter::with_capacity(256 * 1024, File::create(&path)?);
        let mut md5 = Md5::new();
        let mut sha = Sha256::new();
        let mut buf = vec![0u8; 256 * 1024];
        let mut size = 0u64;
        loop {
            let n = match r.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            };
            md5.update(&buf[..n]);
            sha.update(&buf[..n]);
            out.write_all(&buf[..n])?;
            size += n as u64;
        }
        out.flush()?;
        Ok(Blob { id, size, md5: md5.finalize().into(), sha256: sha.finalize().into() })
    }

    pub fn add_bytes(&self, data: &[u8]) -> Result<Blob> {
        let mut r = data;
        self.add_reader(&mut r)
    }

    pub fn path(&self, b: &Blob) -> PathBuf {
        self.dir.path().join(b.id.to_string())
    }

    pub fn open(&self, b: &Blob) -> Result<File> {
        File::open(self.path(b)).with_context(|| format!("lecture du contenu #{}", b.id))
    }

    pub fn read_all(&self, b: &Blob) -> Result<Vec<u8>> {
        Ok(std::fs::read(self.path(b))?)
    }
}

#[derive(Clone, Debug)]
pub enum Kind {
    Dir,
    File(Blob),
    Symlink(String),
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub kind: Kind,
    /// Bits de permission (0o7777), sans le type de fichier.
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub user: String,
    pub group: String,
    pub mtime: u64,
    /// Fichier de configuration (préservé lors des mises à jour).
    pub config: bool,
}

impl Entry {
    pub fn dir(mode: u32, mtime: u64) -> Self {
        Entry { kind: Kind::Dir, mode, uid: 0, gid: 0, user: "root".into(), group: "root".into(), mtime, config: false }
    }
    pub fn file(blob: Blob, mode: u32, mtime: u64) -> Self {
        Entry { kind: Kind::File(blob), mode, uid: 0, gid: 0, user: "root".into(), group: "root".into(), mtime, config: false }
    }
    pub fn symlink(target: &str, mtime: u64) -> Self {
        Entry { kind: Kind::Symlink(target.to_string()), mode: 0o777, uid: 0, gid: 0, user: "root".into(), group: "root".into(), mtime, config: false }
    }
    pub fn is_dir(&self) -> bool {
        matches!(self.kind, Kind::Dir)
    }
    pub fn is_file(&self) -> bool {
        matches!(self.kind, Kind::File(_))
    }
    pub fn is_symlink(&self) -> bool {
        matches!(self.kind, Kind::Symlink(_))
    }
    pub fn size(&self) -> u64 {
        match &self.kind {
            Kind::File(b) => b.size,
            Kind::Symlink(t) => t.len() as u64,
            Kind::Dir => 0,
        }
    }
    /// Mode complet façon `st_mode` (type + permissions).
    pub fn st_mode(&self) -> u32 {
        let t = match self.kind {
            Kind::Dir => 0o040000,
            Kind::File(_) => 0o100000,
            Kind::Symlink(_) => 0o120000,
        };
        t | (self.mode & 0o7777)
    }
    /// Nom d'utilisateur à écrire (root si vide).
    pub fn user_name(&self) -> &str {
        if self.user.is_empty() { if self.uid == 0 { "root" } else { "" } } else { &self.user }
    }
    pub fn group_name(&self) -> &str {
        if self.group.is_empty() { if self.gid == 0 { "root" } else { "" } } else { &self.group }
    }
}

/// Normalise un chemin d'archive : sans « ./ », sans « / » au début ni à la fin.
pub fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                // Les chemins qui sortent de la racine sont refusés.
                parts.pop()?;
            }
            x => parts.push(x),
        }
    }
    if parts.is_empty() { None } else { Some(parts.join("/")) }
}

pub fn parent(path: &str) -> Option<&str> {
    path.rfind('/').map(|i| &path[..i])
}

pub fn basename(path: &str) -> &str {
    path.rfind('/').map(|i| &path[i + 1..]).unwrap_or(path)
}

/// L'arbre virtuel d'un paquet. Les chemins sont relatifs à la racine « / ».
pub struct Tree {
    pub entries: BTreeMap<String, Entry>,
    pub store: Arc<BlobStore>,
}

impl Tree {
    pub fn new() -> Result<Self> {
        Ok(Tree { entries: BTreeMap::new(), store: Arc::new(BlobStore::new()?) })
    }

    /// Ajoute une entrée et crée au besoin les dossiers parents manquants.
    pub fn insert(&mut self, path: &str, entry: Entry) {
        let Some(p) = normalize(path) else { return };
        self.ensure_parents(&p, entry.mtime);
        match (self.entries.get(&p), &entry.kind) {
            // Un dossier déjà présent garde ses attributs s'il est re-déclaré implicitement.
            (Some(old), Kind::Dir) if old.is_dir() => {
                self.entries.insert(p, entry);
            }
            _ => {
                self.entries.insert(p, entry);
            }
        }
    }

    fn ensure_parents(&mut self, path: &str, mtime: u64) {
        let mut cur = path;
        let mut missing = Vec::new();
        while let Some(par) = parent(cur) {
            if par.is_empty() {
                break;
            }
            if self.entries.contains_key(par) {
                break;
            }
            missing.push(par.to_string());
            cur = par;
        }
        for m in missing {
            self.entries.insert(m, Entry::dir(0o755, mtime));
        }
    }

    /// Copie de l'arbre qui partage le même stockage (aucun contenu n'est dupliqué).
    pub fn clone_shallow(&self) -> Tree {
        Tree { entries: self.entries.clone(), store: Arc::clone(&self.store) }
    }

    pub fn add_file_bytes(&mut self, path: &str, data: &[u8], mode: u32, mtime: u64) -> Result<()> {
        let blob = self.store.add_bytes(data)?;
        self.insert(path, Entry::file(blob, mode, mtime));
        Ok(())
    }

    pub fn get(&self, path: &str) -> Option<&Entry> {
        self.entries.get(path)
    }

    pub fn files(&self) -> impl Iterator<Item = (&String, &Entry)> {
        self.entries.iter().filter(|(_, e)| e.is_file())
    }

    pub fn file_count(&self) -> usize {
        self.entries.values().filter(|e| !e.is_dir()).count()
    }

    pub fn dir_count(&self) -> usize {
        self.entries.values().filter(|e| e.is_dir()).count()
    }

    pub fn total_size(&self) -> u64 {
        self.entries.values().map(|e| e.size()).sum()
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        match self.entries.get(path).map(|e| &e.kind) {
            Some(Kind::File(b)) => self.store.read_all(b),
            _ => bail!("fichier introuvable dans le paquet : {path}"),
        }
    }

    /// Déplace tout ce qui est sous `from/` vers `to/` (ex. « bin » → « usr/bin »).
    /// Retourne le nombre d'entrées déplacées. En cas de doublon, l'entrée déjà
    /// présente dans `to` est conservée.
    pub fn move_prefix(&mut self, from: &str, to: &str) -> usize {
        let Some(e) = self.entries.get(from) else { return 0 };
        if !e.is_dir() {
            return 0; // ex. « lib » est un lien symbolique : on n'y touche pas
        }
        let prefix = format!("{from}/");
        let keys: Vec<String> = self.entries.keys().filter(|k| k.starts_with(&prefix)).cloned().collect();
        let mut moved = 0;
        for k in keys {
            let entry = self.entries.remove(&k).unwrap();
            let newk = format!("{to}/{}", &k[prefix.len()..]);
            if self.entries.contains_key(&newk) {
                continue;
            }
            self.ensure_parents(&newk, entry.mtime);
            self.entries.insert(newk, entry);
            moved += 1;
        }
        self.entries.remove(from);
        moved
    }

    /// Vérifie qu'aucun chemin n'est vide ou dangereux.
    pub fn validate(&self) -> Result<()> {
        for k in self.entries.keys() {
            if k.is_empty() || k.starts_with('/') || k.split('/').any(|p| p == ".." || p.is_empty()) {
                bail!("chemin invalide dans le paquet : {k:?}");
            }
        }
        Ok(())
    }
}

/// Lecteur paresseux : n'ouvre le fichier qu'au moment de la première lecture
/// et le referme à la fin (évite d'ouvrir des milliers de fichiers à la fois).
pub struct LazyReader {
    path: PathBuf,
    file: Option<File>,
    done: bool,
}

impl LazyReader {
    pub fn new(path: PathBuf) -> Self {
        LazyReader { path, file: None, done: false }
    }
}

impl Read for LazyReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done {
            return Ok(0);
        }
        if self.file.is_none() {
            self.file = Some(File::open(&self.path)?);
        }
        let n = self.file.as_mut().unwrap().read(buf)?;
        if n == 0 {
            self.file = None;
            self.done = true;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_paths() {
        assert_eq!(normalize("./usr/bin/x").as_deref(), Some("usr/bin/x"));
        assert_eq!(normalize("/usr//bin/").as_deref(), Some("usr/bin"));
        assert_eq!(normalize("./").as_deref(), None);
        assert_eq!(normalize("../etc/passwd"), None);
        assert_eq!(normalize("usr/../etc").as_deref(), Some("etc"));
    }

    #[test]
    fn parents_created_and_move() {
        let mut t = Tree::new().unwrap();
        t.insert("bin/foo", Entry::symlink("bar", 0));
        assert!(t.get("bin").unwrap().is_dir());
        t.move_prefix("bin", "usr/bin");
        assert!(t.get("usr/bin/foo").is_some());
        assert!(t.get("usr").unwrap().is_dir());
        assert!(t.get("bin").is_none());
    }
}
