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
pub mod mapping;
pub mod model;
pub mod pacman;
pub mod rpm;
pub mod scripts;
pub mod shell;
pub mod tarutil;
pub mod tree;
pub mod version;

pub use convert::{analyze, convert, Analysis, ConvertOptions, ConvertResult};
pub use model::{Dependency, Format, Metadata, Package};

/// Version de TAMGA (synchronisée avec Cargo.toml).
pub const TAMGA_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Identité utilisée par défaut comme mainteneur des paquets convertis.
pub const DEFAULT_MAINTAINER: &str = "TAMGA <tamga@localhost>";

#[cfg(test)]
mod send_sync {
    /// Le paquet chargé doit pouvoir passer d'un fil d'exécution à l'autre (interface Tauri).
    #[test]
    fn package_is_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<crate::model::Package>();
        check::<crate::elf::ElfInfo>();
    }
}
