//! TAMGA — moteur de conversion de paquets Linux.
//! TAMGA — Linux package conversion engine.
//!
//! Principe / Principle :
//!   paquet source  ──lecture──▶  `Package` (métadonnées + arbre de fichiers virtuel)
//!   `Package`      ──écriture──▶ paquet cible (.deb, .rpm, Arch, .sh)
//!
//! Tout est écrit en Rust pur : aucun outil externe (dpkg, rpmbuild, makepkg)
//! n'est nécessaire pour convertir. Les outils du système ne servent qu'à
//! vérifier les dépendances disponibles sur la machine.

pub mod ar;
pub mod builddir;
pub mod bundle;
pub mod check;
pub mod cli;
pub mod compress;
pub mod convert;
pub mod cpio;
pub mod deb;
pub mod deps;
pub mod detect;
pub mod distro;
pub mod elf;
pub mod fsdirs;
pub mod install;
pub mod journal;
pub mod lzw;
pub mod mapping;
pub mod model;
pub mod pacman;
pub mod polish;
pub mod repos;
pub mod rpm;
pub mod scripts;
pub mod shell;
pub mod slack;
pub mod slp;
pub mod svr4;
pub mod tarutil;
pub mod terminal;
pub mod tree;
pub mod version;

pub use convert::{analyze, convert, Analysis, ConvertOptions, ConvertResult};
pub use model::{Dependency, Format, Metadata, Package};

/// Version affichée de TAMGA, choisie par l'auteur (« 0.1 » jusqu'à nouvel ordre).
/// Les fichiers techniques (Cargo.toml, package.json, tauri.conf.json) exigent trois
/// chiffres et portent donc « 0.1.0 ».
pub const TAMGA_VERSION: &str = "0.1";

/// Identité utilisée par défaut comme mainteneur des paquets convertis.
pub const DEFAULT_MAINTAINER: &str = "TAMGA <tamga@localhost>";

/// Responsable du paquet quand l'original n'en indique pas : comme les outils Debian
/// (DEBFULLNAME / DEBEMAIL, puis NAME / EMAIL, puis l'identité git de l'utilisateur),
/// sinon « TAMGA <tamga@localhost> ».
pub fn default_maintainer() -> String {
    static CACHE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            let env = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
            let git = |k: &str| distro::run("git", &["config", "--get", k], 5).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
            let mail = env("DEBEMAIL").or_else(|| env("EMAIL")).or_else(|| git("user.email"));
            let name = env("DEBFULLNAME").or_else(|| env("NAME")).or_else(|| git("user.name"));
            match (name, mail) {
                (Some(n), Some(m)) if m.contains('@') => format!("{n} <{m}>"),
                (None, Some(m)) if m.contains('@') => format!("TAMGA <{m}>"),
                _ => DEFAULT_MAINTAINER.to_string(),
            }
        })
        .clone()
}

#[cfg(test)]
mod send_sync {
    /// Le paquet chargé doit pouvoir passer d'un fil d'exécution à l'autre (interface Tauri).
    #[test]
    fn package_is_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<crate::model::Package>();
        check::<crate::elf::ElfInfo>();
    }

    /// La version affichée doit correspondre à celle de Cargo.toml (0.1 ↔ 0.1.0).
    #[test]
    fn display_version_matches_cargo() {
        let cargo = env!("CARGO_PKG_VERSION");
        assert!(cargo == crate::TAMGA_VERSION || cargo == format!("{}.0", crate::TAMGA_VERSION), "{cargo}");
    }
}
