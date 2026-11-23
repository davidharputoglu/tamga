//! Journal détaillé (mode verbeux) : tout ce que fait TAMGA, ligne par ligne.
//! Verbose log: written to a file (shown live in a terminal by the app) and/or stderr (CLI).

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static ON: AtomicBool = AtomicBool::new(false);
static STDERR: AtomicBool = AtomicBool::new(false);
static SINK: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();

/// Taille au-delà de laquelle le journal est recommencé.
const MAX_SIZE: u64 = 8 * 1024 * 1024;

/// Fichier du journal : ~/.cache/tamga/journal.log
pub fn path() -> PathBuf {
    crate::deps::cache_dir().unwrap_or_else(|| std::env::temp_dir().join("tamga-cache")).join("journal.log")
}

/// Le mode verbeux est-il actif ? (à tester avant de préparer un message coûteux)
#[inline]
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Active le journal : dans le fichier (`to_file`) et/ou sur la sortie d'erreur (`to_stderr`).
pub fn enable(to_file: bool, to_stderr: bool) -> std::io::Result<PathBuf> {
    let p = path();
    START.get_or_init(Instant::now);
    if to_file {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        let too_big = std::fs::metadata(&p).map(|m| m.len() > MAX_SIZE).unwrap_or(false);
        let f = std::fs::OpenOptions::new().create(true).append(!too_big).write(true).truncate(too_big).open(&p)?;
        *SINK.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(f);
    }
    STDERR.store(to_stderr, Ordering::Relaxed);
    let was_on = ON.swap(true, Ordering::Relaxed);
    if !was_on {
        write(
            "tamga",
            &format!(
                "===== TAMGA {} — mode verbeux activé ({} UTC) =====",
                crate::TAMGA_VERSION,
                utc_time(crate::tarutil::now())
            ),
        );
    }
    Ok(p)
}

/// Désactive le journal.
pub fn disable() {
    if on() {
        write("tamga", "===== mode verbeux désactivé =====");
    }
    ON.store(false, Ordering::Relaxed);
    if let Some(m) = SINK.get() {
        *m.lock().unwrap() = None;
    }
}

/// Écrit une ligne : « [  12.345] [catégorie] message ».
pub fn write(cat: &str, msg: &str) {
    if !on() {
        return;
    }
    let t = START.get().map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0);
    let mut out = String::new();
    for (i, line) in msg.lines().enumerate() {
        if i == 0 {
            out.push_str(&format!("[{t:9.3}] [{cat}] {line}\n"));
        } else {
            out.push_str(&format!("{:>13} {line}\n", ""));
        }
    }
    if msg.is_empty() {
        out = format!("[{t:9.3}] [{cat}]\n");
    }
    if STDERR.load(Ordering::Relaxed) {
        eprint!("{out}");
    }
    if let Some(m) = SINK.get() {
        if let Ok(mut g) = m.lock() {
            if let Some(f) = g.as_mut() {
                let _ = f.write_all(out.as_bytes());
                let _ = f.flush();
            }
        }
    }
}

/// « 22:47:05 » (UTC) pour l'en-tête.
fn utc_time(secs: u64) -> String {
    let d = secs % 86400;
    format!("{:02}:{:02}:{:02}", d / 3600, d % 3600 / 60, d % 60)
}

/// Taille lisible : « 14,4 Mo ».
pub fn size(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} Mo", n as f64 / 1048576.0)
    } else if n >= 1024 {
        format!("{:.1} Ko", n as f64 / 1024.0)
    } else {
        format!("{n} o")
    }
}

/// Écrit dans le journal si le mode verbeux est actif (le message n'est préparé qu'alors).
#[macro_export]
macro_rules! vlog {
    ($cat:expr, $($arg:tt)*) => {
        if $crate::journal::on() {
            $crate::journal::write($cat, &format!($($arg)*));
        }
    };
}
