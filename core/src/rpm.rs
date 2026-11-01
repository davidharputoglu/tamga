//! Paquets RPM (Fedora, RHEL, openSUSE…) : lecture et écriture du format v4.
//! RPM v4 package reader and writer.
//!
//! Structure d'un .rpm :
//!   lead (96 octets) · en-tête de signature · bourrage 8 · en-tête principal · charge utile (cpio compressé)

use crate::compress::{self, Codec};
use crate::cpio;
use crate::model::{Dependency, Family, Format, Metadata, Op, Package, ScriptStyle, Scripts};
use crate::tree::{normalize, Kind, Tree};
use crate::version;
use anyhow::{bail, Context, Result};
use md5::{Digest as _, Md5};
use sha1::Sha1;
use sha2::Sha256;
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

const HEADER_MAGIC: [u8; 8] = [0x8e, 0xad, 0xe8, 0x01, 0, 0, 0, 0];
const LEAD_MAGIC: [u8; 4] = [0xed, 0xab, 0xee, 0xdb];

// Types de données
const T_INT16: u32 = 3;
const T_INT32: u32 = 4;
const T_INT64: u32 = 5;
const T_STRING: u32 = 6;
const T_BIN: u32 = 7;
const T_STRING_ARRAY: u32 = 8;
const T_I18NSTRING: u32 = 9;

// Étiquettes (tags) utilisées
pub mod tag {
    pub const HEADERSIGNATURES: u32 = 62;
    pub const HEADERIMMUTABLE: u32 = 63;
    pub const HEADERI18NTABLE: u32 = 100;
    pub const SIG_SHA1: u32 = 269;
    pub const SIG_SHA256: u32 = 273;
    pub const SIG_SIZE: u32 = 1000;
    pub const SIG_MD5: u32 = 1004;
    pub const SIG_PAYLOADSIZE: u32 = 1007;
    pub const NAME: u32 = 1000;
    pub const VERSION: u32 = 1001;
    pub const RELEASE: u32 = 1002;
    pub const EPOCH: u32 = 1003;
    pub const SUMMARY: u32 = 1004;
    pub const DESCRIPTION: u32 = 1005;
    pub const BUILDTIME: u32 = 1006;
    pub const BUILDHOST: u32 = 1007;
    pub const SIZE: u32 = 1009;
    pub const VENDOR: u32 = 1011;
    pub const LICENSE: u32 = 1014;
    pub const PACKAGER: u32 = 1015;
    pub const GROUP: u32 = 1016;
    pub const URL: u32 = 1020;
    pub const OS: u32 = 1021;
    pub const ARCH: u32 = 1022;
    pub const PREIN: u32 = 1023;
    pub const POSTIN: u32 = 1024;
    pub const PREUN: u32 = 1025;
    pub const POSTUN: u32 = 1026;
    pub const FILESIZES: u32 = 1028;
    pub const FILEMODES: u32 = 1030;
    pub const FILERDEVS: u32 = 1033;
    pub const FILEMTIMES: u32 = 1034;
    pub const FILEDIGESTS: u32 = 1035;
    pub const FILELINKTOS: u32 = 1036;
    pub const FILEFLAGS: u32 = 1037;
    pub const FILEUSERNAME: u32 = 1039;
    pub const FILEGROUPNAME: u32 = 1040;
    pub const SOURCERPM: u32 = 1044;
    pub const FILEVERIFYFLAGS: u32 = 1045;
    pub const PROVIDENAME: u32 = 1047;
    pub const REQUIREFLAGS: u32 = 1048;
    pub const REQUIRENAME: u32 = 1049;
    pub const REQUIREVERSION: u32 = 1050;
    pub const CONFLICTFLAGS: u32 = 1053;
    pub const CONFLICTNAME: u32 = 1054;
    pub const CONFLICTVERSION: u32 = 1055;
    pub const RPMVERSION: u32 = 1064;
    pub const PREINPROG: u32 = 1085;
    pub const POSTINPROG: u32 = 1086;
    pub const PREUNPROG: u32 = 1087;
    pub const POSTUNPROG: u32 = 1088;
    pub const OBSOLETENAME: u32 = 1090;
    pub const FILEDEVICES: u32 = 1095;
    pub const FILEINODES: u32 = 1096;
    pub const FILELANGS: u32 = 1097;
    pub const PROVIDEFLAGS: u32 = 1112;
    pub const PROVIDEVERSION: u32 = 1113;
    pub const OBSOLETEFLAGS: u32 = 1114;
    pub const OBSOLETEVERSION: u32 = 1115;
    pub const DIRINDEXES: u32 = 1116;
    pub const BASENAMES: u32 = 1117;
    pub const DIRNAMES: u32 = 1118;
    pub const PAYLOADFORMAT: u32 = 1124;
    pub const PAYLOADCOMPRESSOR: u32 = 1125;
    pub const PAYLOADFLAGS: u32 = 1126;
    pub const FILEDIGESTALGO: u32 = 5011;
    pub const ENCODING: u32 = 5062;
    pub const PAYLOADDIGEST: u32 = 5092;
    pub const PAYLOADDIGESTALGO: u32 = 5093;
    pub const PAYLOADDIGESTALT: u32 = 5097;
}

const RPMFILE_CONFIG: u32 = 1;
const RPMFILE_NOREPLACE: u32 = 1 << 4;
const RPMFILE_GHOST: u32 = 1 << 6;
const RPMSENSE_INTERP: u32 = 1 << 8;
const RPMSENSE_SCRIPT_PRE: u32 = 1 << 9;
const RPMSENSE_SCRIPT_POST: u32 = 1 << 10;
const RPMSENSE_SCRIPT_PREUN: u32 = 1 << 11;
const RPMSENSE_SCRIPT_POSTUN: u32 = 1 << 12;
const RPMSENSE_RPMLIB: u32 = 1 << 24;
const PGPHASHALGO_SHA256: u32 = 8;

// ---------------------------------------------------------------------------
// Lecture
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Value {
    Int(Vec<i64>),
    Str(String),
    StrArray(Vec<String>),
    Bin(Vec<u8>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            Value::StrArray(a) => a.first().map(|s| s.as_str()),
            _ => None,
        }
    }
    pub fn as_strs(&self) -> Vec<String> {
        match self {
            Value::Str(s) => vec![s.clone()],
            Value::StrArray(a) => a.clone(),
            _ => vec![],
        }
    }
    pub fn as_ints(&self) -> Vec<i64> {
        match self {
            Value::Int(v) => v.clone(),
            _ => vec![],
        }
    }
}

pub type Header = BTreeMap<u32, Value>;

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Lit un en-tête rpm à la position courante. Retourne (en-tête, octets bruts).
pub fn read_header(r: &mut dyn Read) -> Result<(Header, Vec<u8>)> {
    let mut pre = [0u8; 16];
    r.read_exact(&mut pre).context("en-tête rpm tronqué")?;
    if pre[0..3] != HEADER_MAGIC[0..3] {
        bail!("en-tête rpm invalide");
    }
    let il = be32(&pre[8..12]) as usize;
    let dl = be32(&pre[12..16]) as usize;
    if il > 100_000 || dl > 512 * 1024 * 1024 {
        bail!("en-tête rpm trop grand");
    }
    let mut body = vec![0u8; il * 16 + dl];
    r.read_exact(&mut body).context("en-tête rpm tronqué")?;
    let (index, data) = body.split_at(il * 16);
    let mut h = Header::new();
    for i in 0..il {
        let e = &index[i * 16..i * 16 + 16];
        let t = be32(&e[0..4]);
        let ty = be32(&e[4..8]);
        let off = be32(&e[8..12]) as usize;
        let count = be32(&e[12..16]) as usize;
        if off > data.len() {
            continue;
        }
        let d = &data[off..];
        let v = match ty {
            2 => Value::Int(d.iter().take(count).map(|x| *x as i64).collect()),
            T_INT16 => Value::Int((0..count).filter(|k| k * 2 + 2 <= d.len()).map(|k| u16::from_be_bytes([d[k * 2], d[k * 2 + 1]]) as i64).collect()),
            T_INT32 => Value::Int((0..count).filter(|k| k * 4 + 4 <= d.len()).map(|k| be32(&d[k * 4..]) as i64).collect()),
            T_INT64 => Value::Int(
                (0..count)
                    .filter(|k| k * 8 + 8 <= d.len())
                    .map(|k| u64::from_be_bytes(d[k * 8..k * 8 + 8].try_into().unwrap()) as i64)
                    .collect(),
            ),
            T_STRING => Value::Str(cstr(d).0),
            T_STRING_ARRAY | T_I18NSTRING => {
                let mut v = Vec::with_capacity(count);
                let mut pos = 0;
                for _ in 0..count {
                    if pos >= d.len() {
                        break;
                    }
                    let (s, n) = cstr(&d[pos..]);
                    v.push(s);
                    pos += n + 1;
                }
                if ty == T_I18NSTRING { Value::Str(v.into_iter().next().unwrap_or_default()) } else { Value::StrArray(v) }
            }
            T_BIN => Value::Bin(d.iter().take(count).copied().collect()),
            _ => continue,
        };
        h.insert(t, v);
    }
    let mut raw = pre.to_vec();
    raw.extend_from_slice(&body);
    Ok((h, raw))
}

fn cstr(d: &[u8]) -> (String, usize) {
    let n = d.iter().position(|&c| c == 0).unwrap_or(d.len());
    (String::from_utf8_lossy(&d[..n]).to_string(), n)
}

fn hstr(h: &Header, t: u32) -> String {
    h.get(&t).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn hstrs(h: &Header, t: u32) -> Vec<String> {
    h.get(&t).map(|v| v.as_strs()).unwrap_or_default()
}

fn hints(h: &Header, t: u32) -> Vec<i64> {
    h.get(&t).map(|v| v.as_ints()).unwrap_or_default()
}

fn deps_from(h: &Header, names: u32, flags: u32, versions: u32) -> Vec<Dependency> {
    let n = hstrs(h, names);
    let f = hints(h, flags);
    let v = hstrs(h, versions);
    let mut out = Vec::new();
    for (i, name) in n.iter().enumerate() {
        let fl = f.get(i).copied().unwrap_or(0) as u32;
        if fl & RPMSENSE_RPMLIB != 0 || name.starts_with("rpmlib(") {
            continue;
        }
        let ver = v.get(i).cloned().unwrap_or_default();
        let op = Op::from_rpm_flags(fl);
        if let (Some(op), false) = (op, ver.is_empty()) {
            out.push(Dependency::with(name, op, &ver));
        } else {
            out.push(Dependency::simple(name));
        }
    }
    out
}

/// Lit un fichier .rpm.
pub fn read(path: &Path) -> Result<Package> {
    let f = File::open(path).with_context(|| format!("ouverture de {}", path.display()))?;
    let mut r = BufReader::with_capacity(256 * 1024, f);
    let mut lead = [0u8; 96];
    r.read_exact(&mut lead).context("fichier rpm trop court")?;
    if lead[0..4] != LEAD_MAGIC {
        bail!("ce fichier n'est pas un paquet .rpm");
    }
    if lead[6..8] == [0, 1] {
        bail!("ceci est un paquet source (.src.rpm) : il contient du code à compiler, pas un programme installable");
    }
    // Signature, alignée sur 8 octets
    let (_sig, sig_raw) = read_header(&mut r)?;
    let pad = (8 - (sig_raw.len() % 8)) % 8;
    let mut padbuf = vec![0u8; pad];
    r.read_exact(&mut padbuf)?;
    let (h, _) = read_header(&mut r)?;
    if !h.contains_key(&tag::SOURCERPM) && h.contains_key(&tag::NAME) {
        // Pas de SOURCERPM : c'est un paquet source.
        bail!("ceci est un paquet source (.src.rpm) : il contient du code à compiler, pas un programme installable");
    }

    let mut tree = Tree::new()?;
    let mut warnings = Vec::new();
    let fmt = hstr(&h, tag::PAYLOADFORMAT);
    if !fmt.is_empty() && fmt != "cpio" {
        bail!("format de contenu rpm non pris en charge : {fmt}");
    }
    let mut dec = compress::auto_decoder(&mut r)?;
    cpio::read_into(&mut dec, &mut tree)?;

    // Propriétaires, fichiers de configuration, fantômes : depuis l'en-tête
    let basenames = hstrs(&h, tag::BASENAMES);
    let dirnames = hstrs(&h, tag::DIRNAMES);
    let dirindexes = hints(&h, tag::DIRINDEXES);
    let users = hstrs(&h, tag::FILEUSERNAME);
    let groups = hstrs(&h, tag::FILEGROUPNAME);
    let fflags = hints(&h, tag::FILEFLAGS);
    for (i, b) in basenames.iter().enumerate() {
        let d = dirindexes.get(i).and_then(|&k| dirnames.get(k as usize)).cloned().unwrap_or_default();
        let Some(p) = normalize(&format!("{d}{b}")) else { continue };
        if let Some(e) = tree.entries.get_mut(&p) {
            if let Some(u) = users.get(i) {
                e.user = u.clone();
                e.uid = if u == "root" { 0 } else { e.uid };
            }
            if let Some(g) = groups.get(i) {
                e.group = g.clone();
                e.gid = if g == "root" { 0 } else { e.gid };
            }
            let fl = fflags.get(i).copied().unwrap_or(0) as u32;
            if fl & RPMFILE_CONFIG != 0 {
                e.config = true;
            }
        } else {
            let fl = fflags.get(i).copied().unwrap_or(0) as u32;
            if fl & RPMFILE_GHOST == 0 {
                // fichier annoncé mais absent du contenu : ignoré
            }
        }
    }
    if users.iter().chain(groups.iter()).any(|u| u != "root") {
        warnings.push("warn.non_root_owner".into());
    }

    let script = |t: u32| -> Option<String> {
        let s = hstr(&h, t);
        if s.trim().is_empty() { None } else { Some(s) }
    };
    let prog = |t: u32| hstr(&h, t);
    // Les scripts non-shell (lua…) ne sont pas convertibles.
    let keep = |body: Option<String>, p: String| -> Option<String> {
        let body = body?;
        if p.is_empty() || p.ends_with("/sh") || p.ends_with("/bash") {
            Some(body)
        } else if p.contains("lua") {
            None
        } else {
            Some(format!("#!{p}\n{body}"))
        }
    };
    let scripts = Scripts {
        style: ScriptStyle::Rpm,
        pre_install: keep(script(tag::PREIN), prog(tag::PREINPROG)),
        post_install: keep(script(tag::POSTIN), prog(tag::POSTINPROG)),
        pre_remove: keep(script(tag::PREUN), prog(tag::PREUNPROG)),
        post_remove: keep(script(tag::POSTUN), prog(tag::POSTUNPROG)),
        arch_install: None,
    };
    let name = hstr(&h, tag::NAME);
    let mut provides = deps_from(&h, tag::PROVIDENAME, tag::PROVIDEFLAGS, tag::PROVIDEVERSION);
    provides.retain(|d| d.name != name && !d.name.starts_with(&format!("{name}(")));
    let epoch = hints(&h, tag::EPOCH).first().map(|&e| e as u32);
    let packager = hstr(&h, tag::PACKAGER);
    let vendor = hstr(&h, tag::VENDOR);
    let meta = Metadata {
        name: name.clone(),
        version: hstr(&h, tag::VERSION),
        release: hstr(&h, tag::RELEASE),
        epoch,
        arch: version::canonical_arch(&hstr(&h, tag::ARCH)),
        summary: hstr(&h, tag::SUMMARY),
        description: hstr(&h, tag::DESCRIPTION),
        license: hstr(&h, tag::LICENSE),
        url: hstr(&h, tag::URL),
        maintainer: if !packager.is_empty() { packager } else { vendor },
        section: hstr(&h, tag::GROUP),
        depends: deps_from(&h, tag::REQUIRENAME, tag::REQUIREFLAGS, tag::REQUIREVERSION),
        provides,
        conflicts: deps_from(&h, tag::CONFLICTNAME, tag::CONFLICTFLAGS, tag::CONFLICTVERSION),
        replaces: deps_from(&h, tag::OBSOLETENAME, tag::OBSOLETEFLAGS, tag::OBSOLETEVERSION),
        dep_family: Some(if is_suse(&h) { Family::Suse } else { Family::Fedora }),
        scripts,
        origin_format: Some(Format::Rpm),
    };
    Ok(Package { meta, tree, format: Format::Rpm, warnings })
}

fn is_suse(h: &Header) -> bool {
    let v = hstr(h, tag::VENDOR).to_lowercase();
    let host = hstr(h, tag::BUILDHOST).to_lowercase();
    let dist = hstr(h, 1010).to_lowercase(); // DISTRIBUTION
    v.contains("suse") || host.contains("suse") || dist.contains("suse")
}

// ---------------------------------------------------------------------------
// Écriture
// ---------------------------------------------------------------------------

enum Data {
    I16(Vec<u16>),
    I32(Vec<u32>),
    Str(String),
    StrArr(Vec<String>),
    I18n(String),
    Bin(Vec<u8>),
}

#[derive(Default)]
struct HeaderBuilder {
    entries: BTreeMap<u32, Data>,
}

impl HeaderBuilder {
    fn set(&mut self, t: u32, d: Data) {
        self.entries.insert(t, d);
    }

    /// Sérialise l'en-tête avec sa région immuable (`region_tag` = 62 ou 63).
    fn build(&self, region_tag: u32) -> Vec<u8> {
        let n = self.entries.len() + 1; // + l'entrée de région
        let mut index: Vec<(u32, u32, u32, u32)> = Vec::with_capacity(n);
        let mut data: Vec<u8> = Vec::new();
        for (&t, d) in &self.entries {
            let (ty, align, count, bytes): (u32, usize, u32, Vec<u8>) = match d {
                Data::I16(v) => (T_INT16, 2, v.len() as u32, v.iter().flat_map(|x| x.to_be_bytes()).collect()),
                Data::I32(v) => (T_INT32, 4, v.len() as u32, v.iter().flat_map(|x| x.to_be_bytes()).collect()),
                Data::Str(s) => (T_STRING, 1, 1, cbytes(s)),
                Data::I18n(s) => (T_I18NSTRING, 1, 1, cbytes(s)),
                Data::StrArr(v) => (T_STRING_ARRAY, 1, v.len() as u32, v.iter().flat_map(|s| cbytes(s)).collect()),
                Data::Bin(b) => (T_BIN, 1, b.len() as u32, b.clone()),
            };
            while data.len() % align != 0 {
                data.push(0);
            }
            index.push((t, ty, data.len() as u32, count));
            data.extend_from_slice(&bytes);
        }
        // Remorque de région : 16 octets à la fin des données.
        let trailer_off = data.len() as u32;
        data.extend_from_slice(&region_tag.to_be_bytes());
        data.extend_from_slice(&T_BIN.to_be_bytes());
        data.extend_from_slice(&(-((n as i32) * 16)).to_be_bytes());
        data.extend_from_slice(&16u32.to_be_bytes());
        index.insert(0, (region_tag, T_BIN, trailer_off, 16));

        let mut out = Vec::with_capacity(16 + n * 16 + data.len());
        out.extend_from_slice(&HEADER_MAGIC);
        out.extend_from_slice(&(n as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        for (t, ty, off, count) in index {
            out.extend_from_slice(&t.to_be_bytes());
            out.extend_from_slice(&ty.to_be_bytes());
            out.extend_from_slice(&off.to_be_bytes());
            out.extend_from_slice(&count.to_be_bytes());
        }
        out.extend_from_slice(&data);
        out
    }
}

fn cbytes(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

/// Dépendance finale au format rpm.
#[derive(Clone, Debug)]
pub struct RpmDep {
    pub name: String,
    pub flags: u32,
    pub version: String,
}

impl RpmDep {
    pub fn from_dep(d: &Dependency) -> RpmDep {
        match (&d.op, &d.version) {
            (Some(op), Some(v)) => RpmDep { name: d.name.clone(), flags: op.rpm_flags(), version: v.clone() },
            _ => RpmDep { name: d.name.clone(), flags: 0, version: String::new() },
        }
    }
}

pub struct RpmWriteOpts {
    pub requires: Vec<Dependency>,
    pub provides: Vec<Dependency>,
    pub conflicts: Vec<Dependency>,
    /// Scripts finaux (style rpm).
    pub scripts: Scripts,
    /// Compression de la charge utile.
    pub codec: Codec,
}

fn set_deps(h: &mut HeaderBuilder, deps: &[RpmDep], tn: u32, tf: u32, tv: u32) {
    if deps.is_empty() {
        return;
    }
    h.set(tn, Data::StrArr(deps.iter().map(|d| d.name.clone()).collect()));
    h.set(tf, Data::I32(deps.iter().map(|d| d.flags).collect()));
    h.set(tv, Data::StrArr(deps.iter().map(|d| d.version.clone()).collect()));
}

/// Écrit un .rpm. Les dossiers « système » ne sont pas inclus dans la liste des fichiers.
pub fn write(meta: &Metadata, tree: &Tree, o: &RpmWriteOpts, out: &Path) -> Result<()> {
    let now = crate::tarutil::now() as u32;
    let name = version::rpm_name(&meta.name);
    let (ver, rel) = version::rpm_version(meta);
    let arch = version::rpm_arch(&meta.arch);
    let evr = match meta.epoch.filter(|e| *e > 0) {
        Some(e) => format!("{e}:{ver}-{rel}"),
        None => format!("{ver}-{rel}"),
    };

    // Liste des fichiers : chemins absolus triés, sans les dossiers système.
    let files: Vec<(String, &crate::tree::Entry)> = tree
        .entries
        .iter()
        .filter(|(p, e)| !(e.is_dir() && crate::fsdirs::is_system_dir(p)))
        .map(|(p, e)| (format!("/{p}"), e))
        .collect();
    let mut sorted = files;
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    // --- charge utile cpio compressée → fichier temporaire
    let mut payload = tempfile::tempfile()?;
    let archive_size = {
        let enc = compress::encoder(o.codec, BufWriter::new(&mut payload), None)?;
        let mut cw = cpio::Writer::new(enc);
        for (p, e) in &sorted {
            cw.add(tree, &p[1..], e)?;
        }
        let (enc, written) = cw.finish()?;
        enc.finish_box()?;
        written
    };
    let payload_size = payload.seek(SeekFrom::End(0))?;
    payload.seek(SeekFrom::Start(0))?;
    let payload_sha256 = {
        let mut sha = Sha256::new();
        std::io::copy(&mut payload, &mut DigestWriter(&mut sha))?;
        payload.seek(SeekFrom::Start(0))?;
        hex::encode(sha.finalize())
    };

    // --- en-tête principal
    let mut h = HeaderBuilder::default();
    h.set(tag::HEADERI18NTABLE, Data::StrArr(vec!["C".into()]));
    h.set(tag::NAME, Data::Str(name.clone()));
    h.set(tag::VERSION, Data::Str(ver.clone()));
    h.set(tag::RELEASE, Data::Str(rel.clone()));
    if let Some(e) = meta.epoch.filter(|e| *e > 0) {
        h.set(tag::EPOCH, Data::I32(vec![e]));
    }
    let summary = meta.summary.lines().next().unwrap_or("").trim().to_string();
    h.set(tag::SUMMARY, Data::I18n(if summary.is_empty() { name.clone() } else { summary.clone() }));
    let desc = if meta.description.trim().is_empty() { summary.clone() } else { meta.description.clone() };
    h.set(tag::DESCRIPTION, Data::I18n(if desc.is_empty() { name.clone() } else { desc }));
    h.set(tag::BUILDTIME, Data::I32(vec![now]));
    h.set(tag::BUILDHOST, Data::Str("tamga".into()));
    let total: u64 = tree.total_size();
    h.set(tag::SIZE, Data::I32(vec![total.min(u32::MAX as u64) as u32]));
    h.set(tag::LICENSE, Data::Str(if meta.license.trim().is_empty() { "Unknown".into() } else { meta.license.clone() }));
    if !meta.maintainer.trim().is_empty() {
        h.set(tag::PACKAGER, Data::Str(meta.maintainer.lines().next().unwrap_or("").to_string()));
    }
    h.set(tag::GROUP, Data::I18n("Unspecified".into()));
    if !meta.url.trim().is_empty() {
        h.set(tag::URL, Data::Str(meta.url.trim().to_string()));
    }
    h.set(tag::OS, Data::Str("linux".into()));
    h.set(tag::ARCH, Data::Str(arch.clone()));
    h.set(tag::SOURCERPM, Data::Str(format!("{name}-{ver}-{rel}.src.rpm")));
    h.set(tag::RPMVERSION, Data::Str("4.18.0".into()));
    h.set(tag::ENCODING, Data::Str("utf-8".into()));
    h.set(tag::PAYLOADFORMAT, Data::Str("cpio".into()));
    h.set(tag::PAYLOADCOMPRESSOR, Data::Str(o.codec.rpm_name().into()));
    h.set(tag::PAYLOADFLAGS, Data::Str(match o.codec {
        Codec::Zstd => "12".into(),
        _ => "6".into(),
    }));
    h.set(tag::PAYLOADDIGEST, Data::StrArr(vec![payload_sha256]));
    h.set(tag::PAYLOADDIGESTALGO, Data::I32(vec![PGPHASHALGO_SHA256]));

    // Scripts
    let mut script_reqs: Vec<RpmDep> = Vec::new();
    for (body, tb, tp, sense) in [
        (&o.scripts.pre_install, tag::PREIN, tag::PREINPROG, RPMSENSE_SCRIPT_PRE),
        (&o.scripts.post_install, tag::POSTIN, tag::POSTINPROG, RPMSENSE_SCRIPT_POST),
        (&o.scripts.pre_remove, tag::PREUN, tag::PREUNPROG, RPMSENSE_SCRIPT_PREUN),
        (&o.scripts.post_remove, tag::POSTUN, tag::POSTUNPROG, RPMSENSE_SCRIPT_POSTUN),
    ] {
        if let Some(b) = body {
            h.set(tb, Data::Str(b.clone()));
            h.set(tp, Data::Str("/bin/sh".into()));
            script_reqs.push(RpmDep { name: "/bin/sh".into(), flags: RPMSENSE_INTERP | sense, version: String::new() });
        }
    }

    // Fichiers
    if !sorted.is_empty() {
        let mut dirnames: Vec<String> = Vec::new();
        let mut dir_idx: HashMap<String, u32> = HashMap::new();
        let mut basenames = Vec::new();
        let mut dirindexes = Vec::new();
        for (p, _) in &sorted {
            let (d, b) = match p.rfind('/') {
                Some(i) => (&p[..=i], &p[i + 1..]),
                None => ("/", p.as_str()),
            };
            let idx = *dir_idx.entry(d.to_string()).or_insert_with(|| {
                dirnames.push(d.to_string());
                (dirnames.len() - 1) as u32
            });
            basenames.push(b.to_string());
            dirindexes.push(idx);
        }
        h.set(tag::FILESIZES, Data::I32(sorted.iter().map(|(_, e)| e.size().min(u32::MAX as u64) as u32).collect()));
        h.set(tag::FILEMODES, Data::I16(sorted.iter().map(|(_, e)| e.st_mode() as u16).collect()));
        h.set(tag::FILERDEVS, Data::I16(vec![0; sorted.len()]));
        h.set(tag::FILEMTIMES, Data::I32(sorted.iter().map(|(_, e)| e.mtime as u32).collect()));
        h.set(
            tag::FILEDIGESTS,
            Data::StrArr(
                sorted
                    .iter()
                    .map(|(_, e)| match &e.kind {
                        Kind::File(b) => b.sha256_hex(),
                        _ => String::new(),
                    })
                    .collect(),
            ),
        );
        h.set(
            tag::FILELINKTOS,
            Data::StrArr(
                sorted
                    .iter()
                    .map(|(_, e)| match &e.kind {
                        Kind::Symlink(t) => t.clone(),
                        _ => String::new(),
                    })
                    .collect(),
            ),
        );
        h.set(
            tag::FILEFLAGS,
            Data::I32(sorted.iter().map(|(_, e)| if e.config && e.is_file() { RPMFILE_CONFIG | RPMFILE_NOREPLACE } else { 0 }).collect()),
        );
        h.set(tag::FILEUSERNAME, Data::StrArr(sorted.iter().map(|(_, e)| owner_or_root(e.user_name())).collect()));
        h.set(tag::FILEGROUPNAME, Data::StrArr(sorted.iter().map(|(_, e)| owner_or_root(e.group_name())).collect()));
        h.set(tag::FILEVERIFYFLAGS, Data::I32(vec![u32::MAX; sorted.len()]));
        h.set(tag::FILEDEVICES, Data::I32(vec![1; sorted.len()]));
        h.set(tag::FILEINODES, Data::I32((1..=sorted.len() as u32).collect()));
        h.set(tag::FILELANGS, Data::StrArr(vec![String::new(); sorted.len()]));
        h.set(tag::DIRINDEXES, Data::I32(dirindexes));
        h.set(tag::BASENAMES, Data::StrArr(basenames));
        h.set(tag::DIRNAMES, Data::StrArr(dirnames));
        h.set(tag::FILEDIGESTALGO, Data::I32(vec![PGPHASHALGO_SHA256]));
    }

    // Dépendances
    let mut requires: Vec<RpmDep> = o.requires.iter().map(RpmDep::from_dep).collect();
    requires.extend(script_reqs);
    let rpmlib = |n: &str, v: &str| RpmDep { name: format!("rpmlib({n})"), flags: RPMSENSE_RPMLIB | 2 | 8, version: v.into() };
    requires.push(rpmlib("CompressedFileNames", "3.0.4-1"));
    requires.push(rpmlib("FileDigests", "4.6.0-1"));
    requires.push(rpmlib("PayloadFilesHavePrefix", "4.0-1"));
    match o.codec {
        Codec::Xz => requires.push(rpmlib("PayloadIsXz", "5.2-1")),
        Codec::Zstd => requires.push(rpmlib("PayloadIsZstd", "5.4.18-1")),
        _ => {}
    }
    requires.sort_by(|a, b| a.name.cmp(&b.name).then(a.flags.cmp(&b.flags)));
    requires.dedup_by(|a, b| a.name == b.name && a.flags == b.flags && a.version == b.version);
    set_deps(&mut h, &requires, tag::REQUIRENAME, tag::REQUIREFLAGS, tag::REQUIREVERSION);

    let mut provides: Vec<RpmDep> = vec![RpmDep { name: name.clone(), flags: 8, version: evr.clone() }];
    if let Some(isa) = isa_suffix(&arch) {
        provides.push(RpmDep { name: format!("{name}({isa})"), flags: 8, version: evr.clone() });
    }
    provides.extend(o.provides.iter().map(RpmDep::from_dep));
    set_deps(&mut h, &provides, tag::PROVIDENAME, tag::PROVIDEFLAGS, tag::PROVIDEVERSION);
    let conflicts: Vec<RpmDep> = o.conflicts.iter().map(RpmDep::from_dep).collect();
    set_deps(&mut h, &conflicts, tag::CONFLICTNAME, tag::CONFLICTFLAGS, tag::CONFLICTVERSION);

    let header = h.build(tag::HEADERIMMUTABLE);

    // --- signature (empreintes)
    let mut md5 = Md5::new();
    md5.update(&header);
    {
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            let n = payload.read(&mut buf)?;
            if n == 0 {
                break;
            }
            md5.update(&buf[..n]);
        }
        payload.seek(SeekFrom::Start(0))?;
    }
    let mut s = HeaderBuilder::default();
    s.set(tag::SIG_SHA1, Data::Str(hex::encode(Sha1::digest(&header))));
    s.set(tag::SIG_SHA256, Data::Str(hex::encode(Sha256::digest(&header))));
    let total_size = header.len() as u64 + payload_size;
    if total_size <= u32::MAX as u64 {
        s.set(tag::SIG_SIZE, Data::I32(vec![total_size as u32]));
    }
    s.set(tag::SIG_MD5, Data::Bin(md5.finalize().to_vec()));
    if archive_size <= u32::MAX as u64 {
        s.set(tag::SIG_PAYLOADSIZE, Data::I32(vec![archive_size as u32]));
    }
    let sig = s.build(tag::HEADERSIGNATURES);

    // --- lead
    let mut lead = [0u8; 96];
    lead[0..4].copy_from_slice(&LEAD_MAGIC);
    lead[4] = 3; // version majeure
    lead[5] = 0;
    lead[6..8].copy_from_slice(&0u16.to_be_bytes()); // binaire
    lead[8..10].copy_from_slice(&lead_archnum(&arch).to_be_bytes());
    let nvr = format!("{name}-{ver}-{rel}");
    let nb = nvr.as_bytes();
    let n = nb.len().min(65);
    lead[10..10 + n].copy_from_slice(&nb[..n]);
    lead[76..78].copy_from_slice(&1u16.to_be_bytes()); // Linux
    lead[78..80].copy_from_slice(&5u16.to_be_bytes()); // signature de type « header »

    // --- assemblage
    let file = File::create(out).with_context(|| format!("création de {}", out.display()))?;
    let mut w = BufWriter::new(file);
    w.write_all(&lead)?;
    w.write_all(&sig)?;
    let pad = (8 - (sig.len() % 8)) % 8;
    w.write_all(&vec![0u8; pad])?;
    w.write_all(&header)?;
    std::io::copy(&mut payload, &mut w)?;
    w.flush()?;
    Ok(())
}

fn owner_or_root(s: &str) -> String {
    if s.is_empty() { "root".into() } else { s.to_string() }
}

fn isa_suffix(arch: &str) -> Option<&'static str> {
    Some(match arch {
        "x86_64" => "x86-64",
        "aarch64" => "aarch-64",
        "ppc64le" => "ppc-64",
        "s390x" => "s390-64",
        "riscv64" => "riscv-64",
        "i686" => "x86-32",
        "armv7hl" => "arm-32",
        _ => return None,
    })
}

fn lead_archnum(arch: &str) -> u16 {
    match arch {
        "x86_64" | "i686" => 1,
        "aarch64" => 19,
        "ppc64le" => 16,
        "s390x" => 15,
        "armv7hl" => 12,
        _ => 0,
    }
}

/// Adaptateur : alimente une empreinte avec ce qu'on écrit.
struct DigestWriter<'a, D: sha2::Digest>(&'a mut D);

impl<D: sha2::Digest> Write for DigestWriter<'_, D> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.update(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
