//! Adaptation des scripts d'installation d'un format à l'autre, et petits
//! scripts d'intégration automatiques (menus, icônes, bibliothèques).
//! Install-script translation between formats + automatic integration snippets.
//!
//! Chaque format appelle ses scripts avec des arguments différents :
//!   Debian : preinst install|upgrade, postinst configure [ancienne], prerm remove, postrm remove
//!   rpm    : %pre/%post avec $1 = 1 (installation) ou 2 (mise à jour), %preun/%postun avec $1 = 0
//!   Arch   : fonctions pre_install, post_install, pre_upgrade, post_upgrade, pre_remove, post_remove
//! On conserve le script d'origine tel quel et on l'appelle avec les arguments qu'il attend.
//! Un échec du script d'origine est signalé mais n'interrompt pas l'installation.

use crate::model::{ScriptStyle, Scripts};
use crate::tree::Tree;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    PreInstall,
    PostInstall,
    PreUpgrade,
    PostUpgrade,
    PreRemove,
    PostRemove,
}

/// Ce que contient le paquet et qui demande une mise à jour des caches du système.
#[derive(Clone, Debug, Default)]
pub struct Integration {
    pub ldconfig: bool,
    pub desktop: bool,
    pub icons: bool,
    pub mime: bool,
    pub schemas: bool,
    pub systemd: bool,
}

impl Integration {
    pub fn detect(tree: &Tree, ldconfig: bool) -> Integration {
        let any = |pre: &str, suf: &str| tree.entries.keys().any(|k| k.starts_with(pre) && k.ends_with(suf));
        Integration {
            ldconfig,
            desktop: any("usr/share/applications/", ".desktop"),
            icons: any("usr/share/icons/hicolor/", ""),
            mime: any("usr/share/mime/packages/", ".xml"),
            schemas: any("usr/share/glib-2.0/schemas/", ".xml"),
            systemd: any("usr/lib/systemd/system/", "") || any("lib/systemd/system/", ""),
        }
    }

    pub fn is_empty(&self) -> bool {
        !(self.ldconfig || self.desktop || self.icons || self.mime || self.schemas || self.systemd)
    }

    /// Commandes shell sûres (ignorées si l'outil n'existe pas).
    pub fn snippet(&self) -> String {
        let mut s = String::new();
        if self.ldconfig {
            s.push_str("if command -v ldconfig >/dev/null 2>&1; then ldconfig || :; fi\n");
        }
        if self.desktop {
            s.push_str("if command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database -q /usr/share/applications || :; fi\n");
        }
        if self.icons {
            s.push_str("if [ -d /usr/share/icons/hicolor ] && command -v gtk-update-icon-cache >/dev/null 2>&1; then gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || :; fi\n");
        }
        if self.mime {
            s.push_str("if command -v update-mime-database >/dev/null 2>&1; then update-mime-database /usr/share/mime >/dev/null 2>&1 || :; fi\n");
        }
        if self.schemas {
            s.push_str("if command -v glib-compile-schemas >/dev/null 2>&1; then glib-compile-schemas /usr/share/glib-2.0/schemas >/dev/null 2>&1 || :; fi\n");
        }
        if self.systemd {
            s.push_str("if [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; then systemctl daemon-reload >/dev/null 2>&1 || :; fi\n");
        }
        s
    }
}

fn delimiter(content: &str) -> String {
    let mut n: u64 = 0x7a3c_91f2;
    loop {
        let d = format!("TAMGA_SCRIPT_EOF_{n:08x}");
        if !content.contains(&d) {
            return d;
        }
        n = n.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    }
}

/// Interpréteur à utiliser, d'après la ligne « #! » du script.
fn interpreter(content: &str) -> String {
    match content.lines().next().and_then(|l| l.strip_prefix("#!")) {
        Some(i) if !i.trim().is_empty() => i.trim().to_string(),
        _ => "/bin/sh".to_string(),
    }
}

/// Écrit le script d'origine dans un fichier temporaire et l'exécute.
/// `call` reçoit le chemin du fichier (variable shell) et retourne la commande.
fn run_embedded(content: &str, call: impl Fn(&str) -> String, label: &str) -> String {
    let d = delimiter(content);
    let mut body = content.to_string();
    if !body.ends_with('\n') {
        body.push('\n');
    }
    format!(
        "tamga_f=$(mktemp 2>/dev/null || echo /tmp/tamga-script-$$)\n\
         cat > \"$tamga_f\" <<'{d}'\n{body}{d}\n\
         {} || echo \"TAMGA: script d'origine « {label} » en échec (ignoré) / original script failed (ignored)\" >&2\n\
         rm -f \"$tamga_f\"\n",
        call("$tamga_f")
    )
}

/// Retourne le fragment shell qui exécute l'équivalent de `ev` avec les scripts d'origine.
/// `new_v` et `old_v` sont des expressions shell (ex. « 1.2-1 » ou « $2 »).
pub fn invoke(s: &Scripts, ev: Event, new_v: &str, old_v: &str) -> Option<String> {
    match s.style {
        ScriptStyle::None => None,
        ScriptStyle::Deb => {
            let (body, args) = match ev {
                Event::PreInstall => (&s.pre_install, "install".to_string()),
                Event::PreUpgrade => (&s.pre_install, format!("upgrade \"{old_v}\"")),
                Event::PostInstall => (&s.post_install, "configure".to_string()),
                Event::PostUpgrade => (&s.post_install, format!("configure \"{old_v}\"")),
                Event::PreRemove => (&s.pre_remove, "remove".to_string()),
                Event::PostRemove => (&s.post_remove, "remove".to_string()),
            };
            let body = body.as_ref()?;
            let interp = interpreter(body);
            Some(run_embedded(body, |f| format!("{interp} \"{f}\" {args}"), &format!("{ev:?}")))
        }
        ScriptStyle::Rpm => {
            let (body, arg) = match ev {
                Event::PreInstall => (&s.pre_install, "1"),
                Event::PreUpgrade => (&s.pre_install, "2"),
                Event::PostInstall => (&s.post_install, "1"),
                Event::PostUpgrade => (&s.post_install, "2"),
                Event::PreRemove => (&s.pre_remove, "0"),
                Event::PostRemove => (&s.post_remove, "0"),
            };
            let body = body.as_ref()?;
            let interp = interpreter(body);
            Some(run_embedded(body, |f| format!("{interp} \"{f}\" {arg}"), &format!("{ev:?}")))
        }
        ScriptStyle::Arch => {
            let body = s.arch_install.as_ref()?;
            let (func, args) = match ev {
                Event::PreInstall => ("pre_install", format!("\"{new_v}\"")),
                Event::PostInstall => ("post_install", format!("\"{new_v}\"")),
                Event::PreUpgrade => ("pre_upgrade", format!("\"{new_v}\" \"{old_v}\"")),
                Event::PostUpgrade => ("post_upgrade", format!("\"{new_v}\" \"{old_v}\"")),
                Event::PreRemove => ("pre_remove", format!("\"{new_v}\"")),
                Event::PostRemove => ("post_remove", format!("\"{new_v}\"")),
            };
            Some(run_embedded(
                body,
                |f| {
                    format!(
                        "bash -c '. \"$1\"; shift; f=$1; shift; if type \"$f\" >/dev/null 2>&1; then \"$f\" \"$@\"; fi' tamga \"{f}\" {func} {args}"
                    )
                },
                func,
            ))
        }
    }
}

fn or_colon(s: Option<String>) -> String {
    match s {
        Some(x) if !x.trim().is_empty() => x,
        _ => ":\n".into(),
    }
}

/// Scripts finaux pour un .deb.
pub fn for_deb(s: &Scripts, include: bool, new_v: &str, integ: &Integration) -> Scripts {
    let inc = |ev, old: &str| if include { invoke(s, ev, new_v, old) } else { None };
    let mut out = Scripts { style: ScriptStyle::Deb, ..Default::default() };
    let pre = branch("[ \"$1\" = install ]", inc(Event::PreInstall, ""), inc(Event::PreUpgrade, "$2"));
    if !pre.is_empty() {
        out.pre_install = Some(format!("#!/bin/sh\n# Généré par TAMGA\nif [ \"$1\" = install ] || [ \"$1\" = upgrade ]; then\n{pre}fi\nexit 0\n"));
    }
    let post = branch("[ -z \"$2\" ]", inc(Event::PostInstall, ""), inc(Event::PostUpgrade, "$2"));
    // Debian gère déjà menus et icônes via ses déclencheurs ; seul systemd est ajouté.
    let extra = Integration { systemd: integ.systemd, ..Default::default() }.snippet();
    if !post.is_empty() || !extra.is_empty() {
        out.post_install = Some(format!("#!/bin/sh\n# Généré par TAMGA\nif [ \"$1\" = configure ]; then\n{post}{extra}fi\nexit 0\n"));
    }
    if let Some(pr) = inc(Event::PreRemove, "") {
        out.pre_remove = Some(format!("#!/bin/sh\n# Généré par TAMGA\ncase \"$1\" in\n  remove|deconfigure)\n{pr}  ;;\nesac\nexit 0\n"));
    }
    if let Some(pr) = inc(Event::PostRemove, "") {
        out.post_remove = Some(format!("#!/bin/sh\n# Généré par TAMGA\ncase \"$1\" in\n  remove)\n{pr}  ;;\nesac\nexit 0\n"));
    }
    out
}

/// Bloc « si installation … sinon … » qui n'apparaît que si l'un des deux existe.
fn branch(cond: &str, a: Option<String>, b: Option<String>) -> String {
    match (a, b) {
        (None, None) => String::new(),
        (a, b) => format!("if {cond}; then\n{}else\n{}fi\n", or_colon(a), or_colon(b)),
    }
}

/// Scripts finaux pour un .rpm (corps des sections %pre, %post, %preun, %postun).
pub fn for_rpm(s: &Scripts, include: bool, new_v: &str, integ: &Integration) -> Scripts {
    let inc = |ev| if include { invoke(s, ev, new_v, "") } else { None };
    let mut out = Scripts { style: ScriptStyle::Rpm, ..Default::default() };
    let pre = branch("[ \"$1\" -eq 1 ]", inc(Event::PreInstall), inc(Event::PreUpgrade));
    if !pre.is_empty() {
        out.pre_install = Some(format!("{pre}exit 0\n"));
    }
    let extra = integ.snippet();
    let post = branch("[ \"$1\" -eq 1 ]", inc(Event::PostInstall), inc(Event::PostUpgrade));
    if !post.is_empty() || !extra.is_empty() {
        out.post_install = Some(format!("{post}{extra}exit 0\n"));
    }
    if let Some(pr) = inc(Event::PreRemove) {
        out.pre_remove = Some(format!("if [ \"$1\" -eq 0 ]; then\n{pr}fi\nexit 0\n"));
    }
    let postun = inc(Event::PostRemove).map(|pr| format!("if [ \"$1\" -eq 0 ]; then\n{pr}fi\n")).unwrap_or_default();
    if !postun.is_empty() || !extra.is_empty() {
        out.post_remove = Some(format!("{postun}{extra}exit 0\n"));
    }
    out
}

/// Fichier .INSTALL pour Arch (pacman lance lui-même ldconfig et les crochets menus/icônes).
pub fn for_pacman(s: &Scripts, include: bool) -> Option<String> {
    if !include || s.style == ScriptStyle::None || s.is_empty() {
        return None;
    }
    let f = |ev, new_v: &str, old_v: &str| or_colon(invoke(s, ev, new_v, old_v));
    Some(format!(
        "# Généré par TAMGA\npre_install() {{\n{}}}\n\npost_install() {{\n{}}}\n\npre_upgrade() {{\n{}}}\n\npost_upgrade() {{\n{}}}\n\npre_remove() {{\n{}}}\n\npost_remove() {{\n{}}}\n",
        f(Event::PreInstall, "$1", ""),
        f(Event::PostInstall, "$1", ""),
        f(Event::PreUpgrade, "$1", "$2"),
        f(Event::PostUpgrade, "$1", "$2"),
        f(Event::PreRemove, "$1", ""),
        f(Event::PostRemove, "$1", ""),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deb_to_rpm_wraps() {
        let s = Scripts { style: ScriptStyle::Deb, post_install: Some("#!/bin/sh\necho hello \"$1\"\n".into()), ..Default::default() };
        let r = for_rpm(&s, true, "1.0-1", &Integration::default());
        let post = r.post_install.unwrap();
        assert!(post.contains("configure"));
        assert!(post.contains("echo hello"));
        assert!(r.pre_install.is_none());
        let none = for_rpm(&s, false, "1.0-1", &Integration::default());
        assert!(none.post_install.is_none());
    }

    #[test]
    fn script_runs_in_sh() {
        let s = Scripts { style: ScriptStyle::Rpm, post_install: Some("echo \"arg=$1\"\n".into()), ..Default::default() };
        let d = for_deb(&s, true, "1.0", &Integration::default());
        let script = d.post_install.unwrap();
        let out = std::process::Command::new("sh").arg("-c").arg(&script).arg("x").arg("configure").output();
        if let Ok(o) = out {
            let txt = String::from_utf8_lossy(&o.stdout);
            assert!(txt.contains("arg=1"), "{txt}");
        }
    }
}
