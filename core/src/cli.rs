//! Mode ligne de commande : `tamga analyze|deps|convert …`
//! Command-line mode.

use crate::convert::{self, ConvertOptions, MetaOverride};
use crate::deps::{self, Status, TargetDistro};
use crate::distro::SystemInfo;
use crate::model::Format;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;

pub const USAGE: &str = "\
TAMGA — convertisseur de paquets Linux / Linux package converter

Utilisation / Usage:
  tamga analyze <paquet> [--json]
  tamga deps    <paquet> --to <format> [--distro <auto|debian|fedora|suse|arch|any>] [--online] [--json]
  tamga convert <paquet> --to <format> [--out <dossier|fichier>] [--distro …] [--compress <gz|xz|bz2|zst|none>]
                [--scripts] [--online] [--no-usrmerge] [--name N] [--version V] [--release R] [--install]
  tamga system

Formats lus / read : .deb .rpm Arch (.pkg.tar.*) .sh (TAMGA) Slackware (.txz .tgz .tbz .tlz)
  archives tar (.tar .tar.gz .tar.xz .tar.bz2 .tar.zst .tar.lz .tar.lzma .tar.Z .tar.lz4…)
  Stampede (.slp) Solaris (.pkg, .pkg.gz)
Formats écrits / written (--to) : deb rpm arch sh slack tar slp
  tamga --version
";

/// Commandes reconnues (pour savoir si le programme doit rester en mode texte).
pub fn is_cli_command(arg: &str) -> bool {
    matches!(arg, "analyze" | "deps" | "convert" | "system" | "--version" | "-V" | "--help-cli")
}

struct Args {
    cmd: String,
    file: Option<PathBuf>,
    to: Option<Format>,
    out: Option<PathBuf>,
    distro: TargetDistro,
    scripts: bool,
    online: bool,
    usrmerge: bool,
    meta: MetaOverride,
    json: bool,
    compress: Option<String>,
    install: bool,
}

fn parse(args: &[String]) -> Result<Args> {
    let mut a = Args {
        cmd: args.first().cloned().unwrap_or_default(),
        file: None,
        to: None,
        out: None,
        distro: TargetDistro::ThisSystem,
        scripts: false,
        online: false,
        usrmerge: true,
        meta: MetaOverride::default(),
        json: false,
        compress: None,
        install: false,
    };
    let mut i = 1;
    let val = |i: &mut usize| -> Result<String> {
        *i += 1;
        args.get(*i).cloned().context("valeur manquante / missing value")
    };
    while i < args.len() {
        match args[i].as_str() {
            "--to" => a.to = Some(Format::parse(&val(&mut i)?).context("format inconnu (deb, rpm, arch, sh, slack, tar, slp)")?),
            "--compress" => a.compress = Some(val(&mut i)?),
            "--install" => a.install = true,
            "--out" | "-o" => a.out = Some(PathBuf::from(val(&mut i)?)),
            "--distro" => a.distro = TargetDistro::parse(&val(&mut i)?).context("distribution inconnue")?,
            "--scripts" => a.scripts = true,
            "--online" => a.online = true,
            "--no-usrmerge" => a.usrmerge = false,
            "--json" => a.json = true,
            "--name" => a.meta.name = Some(val(&mut i)?),
            "--version" => a.meta.version = Some(val(&mut i)?),
            "--release" => a.meta.release = Some(val(&mut i)?),
            s if s.starts_with('-') => bail!("option inconnue : {s}\n\n{USAGE}"),
            s => a.file = Some(PathBuf::from(s)),
        }
        i += 1;
    }
    Ok(a)
}

fn status_label(s: Status) -> &'static str {
    match s {
        Status::Installed => "installé / installed",
        Status::Available => "disponible / available",
        Status::Estimated => "estimé / estimated",
        Status::Missing => "INTROUVABLE / NOT FOUND",
        Status::Skipped => "ignoré / skipped",
    }
}

/// Lance le mode texte. Retourne le code de sortie.
pub fn run(args: &[String]) -> i32 {
    match run_inner(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("TAMGA : {e:#}");
            1
        }
    }
}

fn run_inner(args: &[String]) -> Result<()> {
    let a = parse(args)?;
    let sys = SystemInfo::detect();
    match a.cmd.as_str() {
        "--version" | "-V" => {
            println!("TAMGA {}", crate::TAMGA_VERSION);
            Ok(())
        }
        "--help-cli" => {
            print!("{USAGE}");
            Ok(())
        }
        "system" => {
            println!("{}", serde_json::to_string_pretty(&sys)?);
            Ok(())
        }
        "analyze" => {
            let f = a.file.context(USAGE)?;
            let pkg = convert::load(&f)?;
            let (an, _) = convert::analyze(&pkg, &f);
            if a.json {
                println!("{}", serde_json::to_string_pretty(&an)?);
                return Ok(());
            }
            let m = &an.meta;
            println!("Paquet / Package : {} {} ({}) — {}", m.name, m.version, m.arch, an.format.label());
            println!("Résumé / Summary : {}", m.summary);
            println!("Fichiers / Files : {} ({} octets)", an.file_count, an.installed_size);
            println!("Binaires ELF     : {}", an.elf.elf_files);
            if let Some(g) = &an.elf.glibc {
                println!("glibc minimale   : {g}");
            }
            println!("Dépendances déclarées / declared:");
            for d in &m.depends {
                println!("  - {}", d.display());
            }
            println!("Bibliothèques utilisées / libraries used:");
            for l in an.elf.needed.keys() {
                println!("  - {l}");
            }
            if an.has_scripts {
                println!("Scripts d'installation : oui / yes");
            }
            for w in &an.warnings {
                println!("Avertissement / warning : {w}");
            }
            Ok(())
        }
        "deps" => {
            let f = a.file.context(USAGE)?;
            let to = a.to.context("--to <deb|rpm|arch|sh|slack|tar|slp>")?;
            let pkg = convert::load(&f)?;
            let (_, elf) = convert::analyze(&pkg, &f);
            let items = convert::resolve_deps(&pkg, &elf, to, a.distro, a.online, &sys);
            if a.json {
                println!("{}", serde_json::to_string_pretty(&items)?);
                return Ok(());
            }
            for i in &items {
                println!(
                    "{:<45} → {:<40} [{}] {}",
                    i.source,
                    i.target.as_ref().map(|t| t.display_std()).unwrap_or_else(|| "—".into()),
                    status_label(i.status),
                    i.detail.clone().unwrap_or_default()
                );
            }
            if deps::has_missing(&items) {
                println!("\nCertaines dépendances sont introuvables / some dependencies were not found.");
            }
            Ok(())
        }
        "convert" => {
            let f = a.file.context(USAGE)?;
            let to = a.to.context("--to <deb|rpm|arch|sh|slack|tar|slp>")?;
            if !to.writable() {
                bail!("TAMGA sait lire les paquets Solaris mais pas en fabriquer");
            }
            let pkg = convert::load(&f)?;
            if pkg.format == to {
                bail!("le paquet est déjà au format {}", to.label());
            }
            let (_, elf) = convert::analyze(&pkg, &f);
            let out = a.out.unwrap_or_else(|| f.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from(".")));
            let out = if out.as_os_str().is_empty() { PathBuf::from(".") } else { out };
            let opts = ConvertOptions {
                format: to,
                distro: a.distro,
                meta: a.meta,
                deps: None,
                include_scripts: a.scripts,
                usrmerge: a.usrmerge,
                online: a.online,
                output: out,
                compression: a.compress,
            };
            let r = convert::convert(&pkg, &elf, &opts, &sys)?;
            println!("{}", r.path);
            for w in &r.warnings {
                eprintln!("Avertissement / warning : {w}");
            }
            if a.install {
                let plan = crate::install::plan(to, std::path::Path::new(&r.path), &sys, crate::install::Elevation::Sudo)?;
                eprintln!("→ {}", plan.display);
                let code = crate::install::run(&plan, |l| eprintln!("{l}"))?;
                if code != 0 {
                    bail!("l'installation a échoué (code {code})");
                }
                eprintln!("Installation terminée / installed.");
            } else {
                eprintln!("Installation : {}", r.install_command);
            }
            Ok(())
        }
        _ => {
            print!("{USAGE}");
            Ok(())
        }
    }
}
