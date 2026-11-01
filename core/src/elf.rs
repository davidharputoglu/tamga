//! Analyse des binaires ELF : bibliothèques nécessaires (DT_NEEDED), bibliothèques
//! fournies (DT_SONAME), versions de symboles (GLIBC_2.34…).
//! ELF scanning: needed/provided shared libraries and symbol versions.

use crate::fsdirs::looks_like_shared_lib;
use crate::tree::{basename, Kind, Tree};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

/// Bibliothèques du chargeur à ne jamais compter comme dépendances.
const IGNORED: &[&str] = &[
    "ld-linux-x86-64.so.2", "ld-linux-aarch64.so.1", "ld-linux.so.2", "ld-linux-armhf.so.3",
    "ld-linux-riscv64-lp64d.so.1", "ld64.so.2", "ld64.so.1", "linux-vdso.so.1", "linux-gate.so.1",
];

/// Résumé de l'analyse ELF d'un paquet.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ElfInfo {
    /// Nombre de fichiers ELF trouvés.
    pub elf_files: usize,
    /// Le paquet contient du code 64 bits ?
    pub is64: bool,
    /// Architecture déduite des binaires (canonique), si présente.
    pub machine: Option<String>,
    /// Bibliothèques externes nécessaires (soname → versions de symboles demandées).
    pub needed: BTreeMap<String, BTreeSet<String>>,
    /// Bibliothèques fournies par le paquet lui-même.
    pub provided: BTreeSet<String>,
    /// Version maximale de la glibc demandée (ex. « 2.34 »).
    pub glibc: Option<String>,
    /// Version maximale de libstdc++ demandée (ex. « 3.4.30 »).
    pub glibcxx: Option<String>,
}

fn machine_name(m: u16) -> Option<&'static str> {
    use goblin::elf::header::*;
    Some(match m {
        EM_X86_64 => "x86_64",
        EM_AARCH64 => "aarch64",
        EM_386 => "i686",
        EM_ARM => "armv7",
        EM_RISCV => "riscv64",
        EM_PPC64 => "ppc64le",
        EM_S390 => "s390x",
        _ => return None,
    })
}

/// Compare deux versions numériques « 2.34 » / « 2.4 ».
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let pa: Vec<u64> = a.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    let pb: Vec<u64> = b.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    pa.cmp(&pb)
}

fn max_version(cur: &mut Option<String>, v: &str) {
    if cur.as_deref().map(|c| version_cmp(v, c).is_gt()).unwrap_or(true) {
        *cur = Some(v.to_string());
    }
}

/// Analyse tous les fichiers ELF de l'arbre.
pub fn scan(tree: &Tree) -> ElfInfo {
    let mut info = ElfInfo::default();
    let mut all_needed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut machines: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (path, e) in tree.entries.iter() {
        let Kind::File(blob) = &e.kind else { continue };
        if blob.size < 64 {
            continue;
        }
        // Lecture rapide de l'en-tête avant de charger tout le fichier.
        let Ok(mut f) = tree.store.open(blob) else { continue };
        let mut magic = [0u8; 4];
        if f.read_exact(&mut magic).is_err() || &magic != b"\x7fELF" {
            continue;
        }
        drop(f);
        let Ok(file) = tree.store.open(blob) else { continue };
        // SAFETY : le fichier est dans notre dossier temporaire privé et n'est pas modifié.
        let Ok(map) = (unsafe { memmap2::Mmap::map(&file) }) else { continue };
        let Ok(elf) = goblin::elf::Elf::parse(&map) else { continue };
        info.elf_files += 1;
        if elf.is_64 {
            info.is64 = true;
        }
        if let Some(m) = machine_name(elf.header.e_machine) {
            *machines.entry(m).or_default() += 1;
        }
        if let Some(so) = elf.soname {
            info.provided.insert(so.to_string());
        }
        if looks_like_shared_lib(path) {
            info.provided.insert(basename(path).to_string());
        }
        for lib in &elf.libraries {
            all_needed.entry(lib.to_string()).or_default();
        }
        if let Some(verneed) = &elf.verneed {
            for need in verneed.iter() {
                let Some(file) = elf.dynstrtab.get_at(need.vn_file) else { continue };
                for aux in need.iter() {
                    let Some(name) = elf.dynstrtab.get_at(aux.vna_name) else { continue };
                    if name == "GLIBC_PRIVATE" {
                        continue;
                    }
                    all_needed.entry(file.to_string()).or_default().insert(name.to_string());
                    if let Some(v) = name.strip_prefix("GLIBC_") {
                        if v.starts_with(|c: char| c.is_ascii_digit()) {
                            max_version(&mut info.glibc, v);
                        }
                    }
                    if let Some(v) = name.strip_prefix("GLIBCXX_") {
                        if v.starts_with(|c: char| c.is_ascii_digit()) {
                            max_version(&mut info.glibcxx, v);
                        }
                    }
                }
            }
        }
    }
    info.machine = machines.into_iter().max_by_key(|(_, n)| *n).map(|(m, _)| m.to_string());
    for (lib, vers) in all_needed {
        if IGNORED.contains(&lib.as_str()) || info.provided.contains(&lib) {
            continue;
        }
        info.needed.insert(lib, vers);
    }
    info
}

/// Nom de dépendance rpm pour une bibliothèque : « libfoo.so.1()(64bit) ».
pub fn rpm_soname(soname: &str, version: Option<&str>, is64: bool) -> String {
    let v = version.unwrap_or("");
    if is64 {
        format!("{soname}({v})(64bit)")
    } else if v.is_empty() {
        soname.to_string()
    } else {
        format!("{soname}({v})")
    }
}

/// Plus grande version de symbole par préfixe (GLIBC_, GLIBCXX_, CXXABI_, GCC_…),
/// pour n'émettre qu'une dépendance par famille de symboles.
pub fn max_symbol_versions(vers: &BTreeSet<String>) -> Vec<String> {
    let mut best: BTreeMap<String, String> = BTreeMap::new();
    for v in vers {
        let split = v.rfind('_').map(|i| (&v[..i], &v[i + 1..]));
        match split {
            Some((prefix, num)) if num.starts_with(|c: char| c.is_ascii_digit()) => {
                let cur = best.get(prefix);
                let num_cur = cur.and_then(|c| c.rsplit_once('_').map(|(_, n)| n.to_string()));
                if num_cur.as_deref().map(|nc| version_cmp(num, nc).is_gt()).unwrap_or(true) {
                    best.insert(prefix.to_string(), v.clone());
                }
            }
            _ => {
                best.insert(v.clone(), v.clone());
            }
        }
    }
    best.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpm_names() {
        assert_eq!(rpm_soname("libc.so.6", None, true), "libc.so.6()(64bit)");
        assert_eq!(rpm_soname("libc.so.6", Some("GLIBC_2.34"), true), "libc.so.6(GLIBC_2.34)(64bit)");
        assert_eq!(rpm_soname("libc.so.6", None, false), "libc.so.6");
        let s: BTreeSet<String> = ["GLIBC_2.2.5", "GLIBC_2.34", "GLIBC_2.4", "GLIBCXX_3.4.21", "GLIBCXX_3.4.9"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let m = max_symbol_versions(&s);
        assert!(m.contains(&"GLIBC_2.34".to_string()));
        assert!(m.contains(&"GLIBCXX_3.4.21".to_string()));
        assert_eq!(m.len(), 2);
    }
}
