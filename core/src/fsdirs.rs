//! Dossiers « système » partagés par toutes les distributions, fusion /usr,
//! et dossiers de bibliothèques connus de chaque famille.
//! System directories, /usr merge and library search paths.

use crate::model::Family;
use crate::tree::{basename, Kind, Tree};

/// Dossiers appartenant au système de base : un paquet converti ne doit pas
/// les « posséder » (sinon conflits avec le paquet filesystem en rpm).
const SYSTEM_DIRS: &[&str] = &[
    "bin", "boot", "dev", "etc", "home", "lib", "lib32", "lib64", "libx32", "media", "mnt", "opt", "proc", "root", "run",
    "sbin", "srv", "sys", "tmp", "usr", "var",
    "etc/X11", "etc/X11/xorg.conf.d", "etc/X11/Xsession.d", "etc/xdg", "etc/xdg/autostart", "etc/profile.d",
    "etc/default", "etc/init.d", "etc/cron.d", "etc/cron.daily", "etc/cron.hourly", "etc/cron.weekly",
    "etc/cron.monthly", "etc/logrotate.d", "etc/ld.so.conf.d", "etc/modprobe.d", "etc/modules-load.d",
    "etc/sysctl.d", "etc/udev", "etc/udev/rules.d", "etc/systemd", "etc/systemd/system", "etc/systemd/user",
    "etc/dbus-1", "etc/dbus-1/system.d", "etc/dbus-1/session.d", "etc/apt", "etc/apt/sources.list.d",
    "etc/apt/trusted.gpg.d", "etc/apt/keyrings", "etc/yum.repos.d", "etc/pki", "etc/pki/rpm-gpg", "etc/pam.d",
    "etc/security", "etc/sudoers.d", "etc/bash_completion.d", "etc/fonts", "etc/fonts/conf.d", "etc/opt",
    "etc/polkit-1", "etc/polkit-1/rules.d", "etc/skel", "etc/ssl", "etc/ssl/certs", "etc/alternatives",
    "usr/bin", "usr/sbin", "usr/lib", "usr/lib32", "usr/lib64", "usr/libx32", "usr/libexec", "usr/include",
    "usr/src", "usr/games", "usr/local", "usr/local/bin", "usr/local/sbin", "usr/local/lib", "usr/local/share",
    "usr/local/include", "usr/local/etc", "usr/local/games", "usr/local/src", "usr/local/man",
    "usr/share", "usr/share/applications", "usr/share/applications/kde4", "usr/share/appdata", "usr/share/metainfo",
    "usr/share/doc", "usr/share/doc-base", "usr/share/info", "usr/share/man", "usr/share/locale", "usr/share/icons",
    "usr/share/icons/hicolor", "usr/share/pixmaps", "usr/share/mime", "usr/share/mime/packages",
    "usr/share/fonts", "usr/share/fonts/truetype", "usr/share/fonts/opentype", "usr/share/themes",
    "usr/share/sounds", "usr/share/glib-2.0", "usr/share/glib-2.0/schemas", "usr/share/dbus-1",
    "usr/share/dbus-1/services", "usr/share/dbus-1/system-services", "usr/share/dbus-1/system.d",
    "usr/share/dbus-1/interfaces", "usr/share/polkit-1", "usr/share/polkit-1/actions", "usr/share/polkit-1/rules.d",
    "usr/share/bash-completion", "usr/share/bash-completion/completions", "usr/share/zsh",
    "usr/share/zsh/site-functions", "usr/share/zsh/vendor-completions", "usr/share/fish",
    "usr/share/fish/vendor_completions.d", "usr/share/lintian", "usr/share/lintian/overrides", "usr/share/menu",
    "usr/share/gnome-shell", "usr/share/gnome-shell/extensions", "usr/share/gnome-shell/search-providers",
    "usr/share/kservices5", "usr/share/kservices6", "usr/share/plasma", "usr/share/thumbnailers",
    "usr/share/xsessions", "usr/share/wayland-sessions", "usr/share/X11", "usr/share/X11/xorg.conf.d",
    "usr/share/vulkan", "usr/share/vulkan/icd.d", "usr/share/vulkan/implicit_layer.d", "usr/share/licenses",
    "usr/share/pkgconfig", "usr/share/perl5", "usr/share/java", "usr/share/help", "usr/share/gtk-doc",
    "usr/share/gtk-doc/html", "usr/share/emacs", "usr/share/emacs/site-lisp", "usr/share/vim",
    "usr/share/fonts/truetype/dejavu", "usr/share/polkit-1/rules.d",
    "usr/lib/systemd", "usr/lib/systemd/system", "usr/lib/systemd/user", "usr/lib/systemd/system-preset",
    "usr/lib/systemd/user-preset", "usr/lib/systemd/system-generators", "usr/lib/sysusers.d", "usr/lib/tmpfiles.d",
    "usr/lib/sysctl.d", "usr/lib/modules-load.d", "usr/lib/modprobe.d", "usr/lib/udev", "usr/lib/udev/rules.d",
    "usr/lib/udev/hwdb.d", "usr/lib/environment.d", "usr/lib/binfmt.d", "usr/lib/firmware", "usr/lib/modules",
    "usr/lib/mime", "usr/lib/mime/packages", "usr/lib/pkgconfig", "usr/lib64/pkgconfig", "usr/lib/python3",
    "usr/lib/python3/dist-packages", "usr/lib/debug", "usr/lib/locale", "usr/lib/sysimage",
    "usr/lib/x86_64-linux-gnu", "usr/lib/aarch64-linux-gnu", "usr/lib/i386-linux-gnu", "usr/lib/arm-linux-gnueabihf",
    "usr/lib/riscv64-linux-gnu", "usr/lib/powerpc64le-linux-gnu", "usr/lib/s390x-linux-gnu",
    "usr/lib/x86_64-linux-gnu/pkgconfig", "usr/lib/aarch64-linux-gnu/pkgconfig", "usr/lib/mozilla",
    "usr/lib/mozilla/plugins", "usr/lib/mozilla/native-messaging-hosts", "usr/lib64/mozilla",
    "usr/lib64/mozilla/plugins", "usr/lib/jvm", "usr/lib/sddm", "usr/lib/NetworkManager",
    "lib/systemd", "lib/systemd/system", "lib/udev", "lib/udev/rules.d", "lib/modules", "lib/firmware",
    "lib/x86_64-linux-gnu", "lib/aarch64-linux-gnu", "lib/i386-linux-gnu",
    "var/lib", "var/cache", "var/log", "var/spool", "var/tmp", "var/opt", "var/mail", "var/run", "var/lock",
    "var/local", "var/games", "var/www", "var/lib/dbus", "var/lib/systemd",
];

/// Vrai si `dir` est un dossier du système de base.
pub fn is_system_dir(dir: &str) -> bool {
    if SYSTEM_DIRS.contains(&dir) {
        return true;
    }
    // Motifs : icônes hicolor/<taille>/<catégorie>, locale/<langue>/LC_MESSAGES, man/manN, man/<langue>…
    let parts: Vec<&str> = dir.split('/').collect();
    match parts.as_slice() {
        ["usr", "share", "icons", _theme] => true,
        ["usr", "share", "icons", _theme, _size] => true,
        ["usr", "share", "icons", _theme, _size, _cat] => true,
        ["usr", "share", "locale", _lang] => true,
        ["usr", "share", "locale", _lang, "LC_MESSAGES"] => true,
        ["usr", "share", "man", sec] if sec.starts_with("man") => true,
        ["usr", "share", "man", _lang] => true,
        ["usr", "share", "man", _lang, sec] if sec.starts_with("man") => true,
        ["usr", "share", "help", _lang] => true,
        ["usr", "share", "fonts", _kind] => true,
        _ => false,
    }
}

/// Dossiers de bibliothèques cherchés par défaut par le chargeur, par famille.
pub fn default_lib_dirs(family: Family, arch: &str) -> Vec<String> {
    let triplet = multiarch_triplet(arch);
    match family {
        Family::Debian => {
            let mut v = vec!["usr/lib".to_string(), "lib".to_string(), "usr/local/lib".to_string()];
            if let Some(t) = triplet {
                v.push(format!("usr/lib/{t}"));
                v.push(format!("lib/{t}"));
            }
            v
        }
        Family::Fedora | Family::Suse => {
            if is_64bit(arch) {
                vec!["usr/lib64".into(), "lib64".into()]
            } else {
                vec!["usr/lib".into(), "lib".into()]
            }
        }
        Family::Arch => vec!["usr/lib".into(), "lib".into()],
        Family::Generic => vec![],
    }
}

pub fn is_64bit(arch: &str) -> bool {
    matches!(crate::version::canonical_arch(arch).as_str(), "x86_64" | "aarch64" | "riscv64" | "ppc64le" | "s390x")
}

pub fn multiarch_triplet(arch: &str) -> Option<&'static str> {
    Some(match crate::version::canonical_arch(arch).as_str() {
        "x86_64" => "x86_64-linux-gnu",
        "aarch64" => "aarch64-linux-gnu",
        "i686" => "i386-linux-gnu",
        "armv7" => "arm-linux-gnueabihf",
        "riscv64" => "riscv64-linux-gnu",
        "ppc64le" => "powerpc64le-linux-gnu",
        "s390x" => "s390x-linux-gnu",
        _ => return None,
    })
}

/// Fusion /usr : bin → usr/bin, sbin → usr/sbin, lib → usr/lib, lib64 → usr/lib64.
/// Pour Arch, en plus : usr/sbin → usr/bin et (usr/)lib64 → usr/lib.
pub fn apply_usrmerge(tree: &mut Tree, family: Family) -> usize {
    let mut n = 0;
    n += tree.move_prefix("bin", "usr/bin");
    n += tree.move_prefix("sbin", "usr/sbin");
    n += tree.move_prefix("lib", "usr/lib");
    n += tree.move_prefix("lib64", "usr/lib64");
    if family == Family::Arch {
        n += tree.move_prefix("usr/sbin", "usr/bin");
        n += tree.move_prefix("usr/lib64", "usr/lib");
    }
    n
}

/// Est-ce une bibliothèque partagée par son nom ? (libfoo.so, libfoo.so.1.2)
pub fn looks_like_shared_lib(path: &str) -> bool {
    let b = basename(path);
    b.contains(".so") && (b.ends_with(".so") || b.contains(".so."))
}

/// Dossiers « racines de bibliothèques » du paquet qui ne sont pas cherchés
/// par défaut sur la famille cible (il faudra un fichier ld.so.conf.d).
pub fn extra_lib_dirs(tree: &Tree, family: Family, arch: &str) -> Vec<String> {
    let defaults = default_lib_dirs(family, arch);
    let mut roots: Vec<String> = vec![
        "usr/lib".into(), "usr/lib64".into(), "usr/lib32".into(), "lib".into(), "lib64".into(),
    ];
    for t in [
        "x86_64-linux-gnu", "aarch64-linux-gnu", "i386-linux-gnu", "arm-linux-gnueabihf", "riscv64-linux-gnu",
        "powerpc64le-linux-gnu", "s390x-linux-gnu",
    ] {
        roots.push(format!("usr/lib/{t}"));
        roots.push(format!("lib/{t}"));
    }
    let mut out = Vec::new();
    for r in roots {
        if defaults.contains(&r) {
            continue;
        }
        let prefix = format!("{r}/");
        let has_lib = tree.entries.iter().any(|(p, e)| {
            p.starts_with(&prefix)
                && !p[prefix.len()..].contains('/')
                && matches!(e.kind, Kind::File(_) | Kind::Symlink(_))
                && looks_like_shared_lib(p)
        });
        if has_lib {
            out.push(r);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_dirs() {
        assert!(is_system_dir("usr/bin"));
        assert!(is_system_dir("usr/share/icons/hicolor/48x48/apps"));
        assert!(is_system_dir("usr/share/locale/fr/LC_MESSAGES"));
        assert!(is_system_dir("usr/share/man/man1"));
        assert!(!is_system_dir("opt/myapp"));
        assert!(!is_system_dir("usr/share/myapp"));
    }
}
