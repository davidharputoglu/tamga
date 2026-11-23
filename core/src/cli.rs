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
                [--scripts] [--online] [--no-bundle] [--no-usrmerge] [--name N] [--version V] [--release R] [--install]

  --online     cherche les dépendances en ligne et télécharge depuis les dépôts officiels les
               bibliothèques absentes de la cible, pour les fournir avec le paquet
               / look up dependencies online and bundle libraries the target lacks
  --no-bundle  ne pas fournir les bibliothèques manquantes / do not bundle missing libraries
  --verbose    affiche tout ce que fait TAMGA / show everything TAMGA does (aussi -v)
  --arch A     architecture du paquet créé / target architecture (x86_64, aarch64, i686, armv7, noarch…)
  --bump       révision + 1 / increase the release number by one
  --fixperms   droits des fichiers « propres » / sanitize file permissions and owners
  --folder     écrit un dossier de construction modifiable au lieu du paquet / write an editable build folder
  --test       vérifie le paquet créé (TAMGA, puis lintian, rpmlint ou namcap) / check the created package
Plusieurs paquets / several packages : tamga convert a.deb b.deb c.deb --to rpm --out dossier/
Un dossier de construction se reconvertit / a build folder converts back : tamga convert mon-dossier/ --to deb
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
    bundle: bool,
    verbose: bool,
    bump: bool,
    fixperms: bool,
    folder: bool,
    test: bool,
    files: Vec<PathBuf>,
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
        bundle: true,
        verbose: false,
        bump: false,
        fixperms: false,
        folder: false,
        test: false,
        files: Vec::new(),
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
            "--no-bundle" => a.bundle = false,
            "--verbose" | "-v" => a.verbose = true,
            "--arch" | "--target" => a.meta.arch = Some(val(&mut i)?),
            "--bump" => a.bump = true,
            "--fixperms" => a.fixperms = true,
            "--folder" | "--generate" => a.folder = true,
            "--test" => a.test = true,
            "--no-usrmerge" => a.usrmerge = false,
            "--json" => a.json = true,
            "--name" => a.meta.name = Some(val(&mut i)?),
            "--version" => a.meta.version = Some(val(&mut i)?),
            "--release" => a.meta.release = Some(val(&mut i)?),
            s if s.starts_with('-') => bail!("option inconnue : {s}\n\n{USAGE}"),
            s => {
                a.files.push(PathBuf::from(s));
                if a.file.is_none() {
                    a.file = Some(PathBuf::from(s));
                }
            }
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
        Status::Bundled => "FOURNIE / BUNDLED",
    }
}

/// Progression des recherches en ligne, sur la sortie d'erreur.
fn cli_progress() -> crate::repos::Progress {
    std::sync::Arc::new(|key: &str, detail: &str| {
        if crate::journal::on() {
            crate::journal::write("progression", &format!("{key} {detail}"));
            return;
        }
        let what = match key {
            "bundle.search" => "Recherche / looking for",
            "bundle.index" => "Liste des paquets / package list",
            "bundle.download" => "Téléchargement / download",
            _ => key,
        };
        eprintln!("… {what} : {detail}");
    })
}

/// Convertit un paquet (ou un dossier de construction) selon les options de la ligne de commande.
fn convert_one(a: &Args, f: &std::path::Path, to: Format, sys: &SystemInfo) -> Result<()> {
    let pkg = convert::load(f)?;
    if pkg.format == to && !f.is_dir() {
        bail!("le paquet est déjà au format {}", to.label());
    }
    let (_, elf) = convert::analyze(&pkg, f);
    let mut meta = a.meta.clone();
    if a.bump {
        let cur = meta.release.clone().unwrap_or_else(|| pkg.meta.release.clone());
        meta.release = Some(crate::version::bump_release(&cur));
    }
    let out = a.out.clone().unwrap_or_else(|| f.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from(".")));
    let out = if out.as_os_str().is_empty() { PathBuf::from(".") } else { out };
    // Plusieurs paquets : --out est toujours un dossier.
    let out = if a.files.len() > 1 && !out.is_dir() {
        std::fs::create_dir_all(&out)?;
        out
    } else {
        out
    };
    let opts = ConvertOptions {
        format: to,
        distro: a.distro,
        meta,
        deps: None,
        include_scripts: a.scripts,
        usrmerge: a.usrmerge,
        online: a.online,
        output: out,
        compression: a.compress.clone(),
        bundle: a.bundle,
        bundled: None,
        fixperms: a.fixperms,
        as_folder: a.folder,
    };
    let r = convert::convert_with_progress(&pkg, &elf, &opts, sys, Some(cli_progress()))?;
    println!("{}", r.path);
    for b in &r.bundled {
        eprintln!("Fournie avec le paquet / bundled : {b}");
    }
    for w in &r.warnings {
        eprintln!("Avertissement / warning : {w}");
    }
    if a.folder {
        eprintln!("Dossier de construction / build folder. Ensuite / then : {}", r.install_command);
        return Ok(());
    }
    if a.test {
        let rep = crate::check::check(std::path::Path::new(&r.path), to, sys);
        eprintln!("Vérification / check : {} erreur(s) / error(s), {} avertissement(s) / warning(s)", rep.errors, rep.warnings);
        for l in &rep.lines {
            eprintln!("  [{}] {}", l.level, l.text);
        }
    }
    if a.install {
        let plan = crate::install::plan(to, std::path::Path::new(&r.path), sys, crate::install::Elevation::Sudo)?;
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
    if a.verbose {
        // Sur la sortie d'erreur, et aussi dans ~/.cache/tamga/journal.log
        let _ = crate::journal::enable(true, true);
    }
    let sys = SystemInfo::detect();
    crate::vlog!("système", "{} {} — famille {:?}, {} , glibc {}, outils : {}", sys.name, sys.version_id, sys.family, sys.arch, sys.glibc.clone().unwrap_or_default(), sys.tools.join(", "));
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
            let items = convert::resolve_deps(&pkg, &elf, to, a.distro, a.online, a.bundle, &sys, Some(cli_progress()));
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
                if let Some(n) = &i.needed_by {
                    println!("{:<45}   (← {n})", "");
                }
            }
            if deps::has_missing(&items) {
                println!("\nCertaines dépendances sont introuvables / some dependencies were not found.");
            }
            Ok(())
        }
        "convert" => {
            if a.files.is_empty() {
                bail!("{USAGE}");
            }
            let to = a.to.context("--to <deb|rpm|arch|sh|slack|tar|slp>")?;
            if !to.writable() {
                bail!("TAMGA sait lire les paquets Solaris mais pas en fabriquer");
            }
            if a.files.len() > 1 && (a.meta.name.is_some() || a.meta.version.is_some()) {
                bail!("--name et --version ne s'appliquent qu'à un seul paquet / only for a single package");
            }
            // Plusieurs paquets : chacun est converti, et un échec n'arrête pas les autres.
            let total = a.files.len();
            let mut failed = 0;
            for (n, f) in a.files.iter().enumerate() {
                if total > 1 {
                    eprintln!("\n[{}/{}] {}", n + 1, total, f.display());
                }
                match convert_one(&a, f, to, &sys) {
                    Ok(()) => {}
                    Err(e) => {
                        failed += 1;
                        eprintln!("TAMGA : {}: {e:#}", f.display());
                        if total == 1 {
                            return Err(e);
                        }
                    }
                }
            }
            if total > 1 {
                eprintln!("\n{} réussi(s) / succeeded, {} échec(s) / failed", total - failed, failed);
                if failed > 0 {
                    bail!("{failed} paquet(s) non converti(s)");
                }
            }
            Ok(())
        }
        _ => {
            print!("{USAGE}");
            Ok(())
        }
    }
}
