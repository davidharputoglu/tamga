//! Ouverture d'un terminal qui affiche le journal en direct (mode verbeux).
//! Opening a terminal emulator that follows the verbose log.

use anyhow::{bail, Result};
use serde::Serialize;
use std::process::{Command, Stdio};

/// Comment un terminal reçoit la commande à lancer.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Style {
    /// `terminal [options] -- sh -c CMD`
    DashDash,
    /// `terminal -e sh -c CMD`
    DashE,
    /// `terminal -x sh -c CMD`
    DashX,
    /// `terminal -e "sh -c 'CMD'"` (une seule chaîne)
    ESingle,
    /// `terminal sh -c CMD`
    Direct,
}

struct Known {
    id: &'static str,
    label: &'static str,
    bin: &'static str,
    pre: &'static [&'static str],
    style: Style,
}

/// Terminaux connus, du plus courant au moins courant.
const KNOWN: &[Known] = &[
    Known { id: "ptyxis", label: "Ptyxis (GNOME)", bin: "ptyxis", pre: &["--new-window"], style: Style::DashDash },
    Known { id: "kgx", label: "Console (GNOME)", bin: "kgx", pre: &[], style: Style::DashDash },
    Known { id: "gnome-terminal", label: "GNOME Terminal", bin: "gnome-terminal", pre: &[], style: Style::DashDash },
    Known { id: "konsole", label: "Konsole (KDE)", bin: "konsole", pre: &[], style: Style::DashE },
    Known { id: "xfce4-terminal", label: "Xfce Terminal", bin: "xfce4-terminal", pre: &[], style: Style::DashX },
    Known { id: "mate-terminal", label: "MATE Terminal", bin: "mate-terminal", pre: &[], style: Style::DashX },
    Known { id: "tilix", label: "Tilix", bin: "tilix", pre: &[], style: Style::ESingle },
    Known { id: "terminator", label: "Terminator", bin: "terminator", pre: &[], style: Style::DashX },
    Known { id: "kitty", label: "kitty", bin: "kitty", pre: &[], style: Style::Direct },
    Known { id: "alacritty", label: "Alacritty", bin: "alacritty", pre: &[], style: Style::DashE },
    Known { id: "wezterm", label: "WezTerm", bin: "wezterm", pre: &["start"], style: Style::DashDash },
    Known { id: "foot", label: "foot", bin: "foot", pre: &[], style: Style::Direct },
    Known { id: "ghostty", label: "Ghostty", bin: "ghostty", pre: &[], style: Style::DashE },
    Known { id: "qterminal", label: "QTerminal (LXQt)", bin: "qterminal", pre: &[], style: Style::ESingle },
    Known { id: "lxterminal", label: "LXTerminal (LXDE)", bin: "lxterminal", pre: &[], style: Style::ESingle },
    Known { id: "deepin-terminal", label: "Deepin Terminal", bin: "deepin-terminal", pre: &[], style: Style::DashE },
    Known { id: "io.elementary.terminal", label: "Terminal (elementary)", bin: "io.elementary.terminal", pre: &[], style: Style::DashE },
    Known { id: "urxvt", label: "rxvt-unicode", bin: "urxvt", pre: &[], style: Style::DashE },
    Known { id: "st", label: "st", bin: "st", pre: &[], style: Style::DashE },
    Known { id: "xterm", label: "XTerm", bin: "xterm", pre: &["-u8", "-T", "TAMGA"], style: Style::DashE },
    Known { id: "xdg-terminal-exec", label: "xdg-terminal-exec", bin: "xdg-terminal-exec", pre: &[], style: Style::Direct },
    Known { id: "x-terminal-emulator", label: "x-terminal-emulator (Debian)", bin: "x-terminal-emulator", pre: &[], style: Style::DashE },
];

/// Un terminal disponible sur cette machine.
#[derive(Clone, Debug, Serialize)]
pub struct TerminalInfo {
    pub id: String,
    pub label: String,
}

/// Terminaux installés.
pub fn available() -> Vec<TerminalInfo> {
    KNOWN
        .iter()
        .filter(|k| crate::distro::which(k.bin).is_some())
        .map(|k| TerminalInfo { id: k.id.into(), label: k.label.into() })
        .collect()
}

/// Terminal du bureau en cours (GNOME, KDE, Xfce…), sinon le premier trouvé.
pub fn auto() -> Option<TerminalInfo> {
    let desk = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_ascii_lowercase();
    let prefs: &[&str] = if desk.contains("kde") {
        &["konsole"]
    } else if desk.contains("xfce") {
        &["xfce4-terminal"]
    } else if desk.contains("mate") {
        &["mate-terminal"]
    } else if desk.contains("lxqt") {
        &["qterminal"]
    } else if desk.contains("lxde") {
        &["lxterminal"]
    } else if desk.contains("deepin") {
        &["deepin-terminal"]
    } else if desk.contains("pantheon") {
        &["io.elementary.terminal"]
    } else if desk.contains("cinnamon") {
        &["gnome-terminal", "xfce4-terminal"]
    } else if desk.contains("gnome") || desk.contains("unity") || desk.contains("budgie") {
        &["ptyxis", "kgx", "gnome-terminal"]
    } else {
        &[]
    };
    let order = prefs.iter().copied().chain(["xdg-terminal-exec", "x-terminal-emulator"]).chain(KNOWN.iter().map(|k| k.id));
    for id in order {
        if let Some(k) = KNOWN.iter().find(|k| k.id == id) {
            if crate::distro::which(k.bin).is_some() {
                return Some(TerminalInfo { id: k.id.into(), label: k.label.into() });
            }
        }
    }
    None
}

/// Met une chaîne entre apostrophes pour le shell.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Commande lancée dans le terminal : un titre, puis le journal en direct.
pub fn follow_command(journal: &str) -> String {
    format!(
        "printf '\\033]0;TAMGA — journal\\007'; \
         echo 'TAMGA — mode verbeux / verbose mode'; \
         echo {j}; echo; \
         exec tail -n 200 -F {j}",
        j = sh_quote(journal)
    )
}

/// Programme et arguments pour lancer `cmd` dans le terminal `id`.
/// `custom` : commande choisie par l'utilisateur ; « {cmd} » y est remplacé par la commande.
fn argv(id: &str, custom: Option<&str>, cmd: &str) -> Result<Vec<String>> {
    if id == "custom" {
        let t = custom.map(str::trim).filter(|s| !s.is_empty());
        let Some(t) = t else { bail!("err.terminal_custom") };
        let inner = format!("sh -c {}", sh_quote(cmd));
        let line = if t.contains("{cmd}") { t.replace("{cmd}", &inner) } else { format!("{t} {inner}") };
        return Ok(vec!["sh".into(), "-c".into(), line]);
    }
    let k = KNOWN.iter().find(|k| k.id == id).ok_or_else(|| anyhow::anyhow!("err.terminal_unknown"))?;
    let mut v: Vec<String> = vec![k.bin.into()];
    v.extend(k.pre.iter().map(|s| s.to_string()));
    match k.style {
        Style::DashDash => v.extend(["--".into(), "sh".into(), "-c".into(), cmd.into()]),
        Style::DashE => v.extend(["-e".into(), "sh".into(), "-c".into(), cmd.into()]),
        Style::DashX => v.extend(["-x".into(), "sh".into(), "-c".into(), cmd.into()]),
        Style::ESingle => v.extend(["-e".into(), format!("sh -c {}", sh_quote(cmd))]),
        Style::Direct => v.extend(["sh".into(), "-c".into(), cmd.into()]),
    }
    Ok(v)
}

/// Variables mises par une AppImage qui gêneraient les programmes du système.
const APPIMAGE_VARS: &[&str] = &[
    "LD_LIBRARY_PATH", "LD_PRELOAD", "GIO_MODULE_DIR", "GDK_PIXBUF_MODULE_FILE", "GDK_PIXBUF_MODULEDIR", "GTK_PATH",
    "GTK_EXE_PREFIX", "GTK_DATA_PREFIX", "GTK_IM_MODULE_FILE", "GSETTINGS_SCHEMA_DIR", "PYTHONHOME", "PYTHONPATH",
    "PERLLIB", "QT_PLUGIN_PATH", "GST_PLUGIN_SYSTEM_PATH", "GST_PLUGIN_SYSTEM_PATH_1_0",
];

/// Ouvre le terminal (`id` : « auto », un terminal connu, ou « custom ») sur le journal.
/// Retourne le nom du terminal utilisé.
pub fn open(id: &str, custom: Option<&str>, journal: &str) -> Result<String> {
    let (id, label) = if id == "auto" || id.is_empty() {
        let a = auto().ok_or_else(|| anyhow::anyhow!("err.no_terminal"))?;
        (a.id, a.label)
    } else if id == "custom" {
        ("custom".to_string(), custom.unwrap_or("").to_string())
    } else {
        (id.to_string(), KNOWN.iter().find(|k| k.id == id).map(|k| k.label.to_string()).unwrap_or_default())
    };
    let v = argv(&id, custom, &follow_command(journal))?;
    crate::vlog!("terminal", "ouverture : {}", v.iter().map(|a| if a.contains(' ') { sh_quote(a) } else { a.clone() }).collect::<Vec<_>>().join(" "));
    let mut c = Command::new(&v[0]);
    c.args(&v[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if std::env::var_os("APPIMAGE").is_some() {
        for k in APPIMAGE_VARS {
            c.env_remove(k);
        }
    }
    c.spawn().map_err(|e| anyhow::anyhow!("err.terminal_failed: {e}"))?;
    Ok(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands() {
        let cmd = follow_command("/home/a b/.cache/tamga/journal.log");
        assert!(cmd.contains("tail -n 200 -F '/home/a b/.cache/tamga/journal.log'"));
        assert_eq!(argv("gnome-terminal", None, "X").unwrap(), vec!["gnome-terminal", "--", "sh", "-c", "X"]);
        assert_eq!(argv("konsole", None, "X").unwrap(), vec!["konsole", "-e", "sh", "-c", "X"]);
        assert_eq!(argv("tilix", None, "echo 'a'").unwrap(), vec!["tilix", "-e", "sh -c 'echo '\\''a'\\'''"]);
        assert_eq!(argv("custom", Some("myterm --run {cmd}"), "X").unwrap(), vec!["sh", "-c", "myterm --run sh -c 'X'"]);
        assert_eq!(argv("custom", Some("myterm -e"), "X").unwrap()[2], "myterm -e sh -c 'X'");
        assert!(argv("custom", Some("  "), "X").is_err());
        assert!(argv("nope", None, "X").is_err());
    }
}
