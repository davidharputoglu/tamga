//! Installateur autonome « .sh » (format propre à TAMGA) : écriture et relecture.
//! Self-extracting `.sh` installer (TAMGA format): writer and reader.
//!
//! Structure du fichier :
//!   script shell (POSIX) · ligne « __TAMGA_PAYLOAD__ » · archive tar.gz des fichiers
//! Les métadonnées complètes sont dans une ligne « # tamga-meta: <base64 JSON> »,
//! ce qui permet à TAMGA de reconvertir ensuite le .sh vers un autre format.

use crate::compress::{self, Codec};
use crate::model::{Format, Metadata, Package};
use crate::scripts::{self, Event, Integration};
use crate::tarutil::{self, Sink, TarOpts};
use crate::tree::Tree;
use anyhow::{bail, Context, Result};
use base64::Engine as _;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const MARKER: &str = "__TAMGA_PAYLOAD__";
const META_PREFIX: &str = "# tamga-meta: ";

/// Met une chaîne entre apostrophes pour le shell.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Messages de l'installateur dans les 7 langues de TAMGA.
/// Ordre : (clé, [tr, fr, en, ar, it, pt, es])
const MESSAGES: &[(&str, [&str; 7])] = &[
    ("M_INSTALLING", ["Kuruluyor:", "Installation de", "Installing", "جارٍ تثبيت", "Installazione di", "Instalando", "Instalando"]),
    ("M_ARCH", [
        "Uyumsuz mimari: bu paket %s için hazırlanmış, bu sistem %s.",
        "Architecture incompatible : ce paquet est prévu pour %s, ce système est %s.",
        "Incompatible architecture: this package is built for %s, this system is %s.",
        "معمارية غير متوافقة: هذه الحزمة مُعدّة لـ %s، وهذا النظام %s.",
        "Architettura non compatibile: questo pacchetto è per %s, questo sistema è %s.",
        "Arquitetura incompatível: este pacote é para %s, este sistema é %s.",
        "Arquitectura incompatible: este paquete es para %s, este sistema es %s.",
    ]),
    ("M_CHECK", [
        "Gerekli kütüphaneler denetleniyor…",
        "Vérification des bibliothèques nécessaires…",
        "Checking required libraries…",
        "جارٍ التحقق من المكتبات المطلوبة…",
        "Verifica delle librerie necessarie…",
        "Verificando as bibliotecas necessárias…",
        "Comprobando las bibliotecas necesarias…",
    ]),
    ("M_MISSING", [
        "Bu sistemde bulunamayan kütüphaneler:",
        "Bibliothèques introuvables sur ce système :",
        "Libraries not found on this system:",
        "مكتبات غير موجودة على هذا النظام:",
        "Librerie non trovate su questo sistema:",
        "Bibliotecas não encontradas neste sistema:",
        "Bibliotecas no encontradas en este sistema:",
    ]),
    ("M_HINT", [
        "Bunları kurmak için önerilen komut:",
        "Commande suggérée pour les installer :",
        "Suggested command to install them:",
        "الأمر المقترح لتثبيتها:",
        "Comando suggerito per installarle:",
        "Comando sugerido para instalá-las:",
        "Comando sugerido para instalarlas:",
    ]),
    ("M_GLIBC", [
        "Bu program glibc %s veya daha yenisini gerektiriyor; bu sistemde %s var.",
        "Ce programme demande la glibc %s ou plus récente ; ce système a la %s.",
        "This program needs glibc %s or newer; this system has %s.",
        "يتطلب هذا البرنامج glibc %s أو أحدث؛ وهذا النظام يحتوي على %s.",
        "Questo programma richiede glibc %s o successiva; questo sistema ha la %s.",
        "Este programa precisa da glibc %s ou mais recente; este sistema tem a %s.",
        "Este programa necesita glibc %s o más reciente; este sistema tiene %s.",
    ]),
    ("M_ASK", [
        "Yine de devam edilsin mi? [e/H] ",
        "Continuer quand même ? [o/N] ",
        "Continue anyway? [y/N] ",
        "هل تريد المتابعة رغم ذلك؟ [y/N] ",
        "Continuare comunque? [s/N] ",
        "Continuar mesmo assim? [s/N] ",
        "¿Continuar de todos modos? [s/N] ",
    ]),
    ("M_ABORT", [
        "Kurulum iptal edildi.",
        "Installation annulée.",
        "Installation cancelled.",
        "تم إلغاء التثبيت.",
        "Installazione annullata.",
        "Instalação cancelada.",
        "Instalación cancelada.",
    ]),
    ("M_ROOT", [
        "Yönetici yetkisi gerekiyor…",
        "Droits administrateur nécessaires…",
        "Administrator rights required…",
        "صلاحيات المدير مطلوبة…",
        "Servono i diritti di amministratore…",
        "São necessários direitos de administrador…",
        "Se necesitan permisos de administrador…",
    ]),
    ("M_NOROOT", [
        "Yönetici yetkisi alınamadı: sudo ile yeniden çalıştırın.",
        "Impossible d'obtenir les droits administrateur : relancez avec sudo.",
        "Cannot get administrator rights: run again with sudo.",
        "تعذّر الحصول على صلاحيات المدير: أعد التشغيل باستخدام sudo.",
        "Impossibile ottenere i diritti di amministratore: riavvia con sudo.",
        "Não foi possível obter direitos de administrador: execute novamente com sudo.",
        "No se pudieron obtener permisos de administrador: vuelva a ejecutar con sudo.",
    ]),
    ("M_PREV", [
        "Kurulu sürüm (%s) değiştirilecek.",
        "Version déjà installée (%s) : elle sera remplacée.",
        "Version already installed (%s): it will be replaced.",
        "الإصدار المثبّت (%s) سيتم استبداله.",
        "Versione già installata (%s): verrà sostituita.",
        "Versão já instalada (%s): será substituída.",
        "Versión ya instalada (%s): será reemplazada.",
    ]),
    ("M_OVER", [
        "Var olan %s dosya değiştirilecek (yedeği saklanır):",
        "%s fichier(s) déjà présent(s) seront remplacés (une sauvegarde est conservée) :",
        "%s existing file(s) will be replaced (a backup is kept):",
        "سيتم استبدال %s ملف(ات) موجودة (مع الاحتفاظ بنسخة احتياطية):",
        "%s file esistenti verranno sostituiti (viene conservata una copia):",
        "%s arquivo(s) existente(s) serão substituídos (uma cópia de segurança é mantida):",
        "Se reemplazarán %s archivo(s) existente(s) (se guarda una copia de seguridad):",
    ]),
    ("M_COPY", ["Dosyalar kopyalanıyor…", "Copie des fichiers…", "Copying files…", "جارٍ نسخ الملفات…", "Copia dei file…", "Copiando arquivos…", "Copiando archivos…"]),
    ("M_DONE", ["Kurulum tamamlandı.", "Installation terminée.", "Installation complete.", "اكتمل التثبيت.", "Installazione completata.", "Instalação concluída.", "Instalación completada."]),
    ("M_UNHINT", ["Kaldırmak için:", "Pour désinstaller :", "To uninstall:", "لإلغاء التثبيت:", "Per disinstallare:", "Para desinstalar:", "Para desinstalar:"]),
    ("M_UNINST", ["Kaldırılıyor:", "Désinstallation de", "Uninstalling", "جارٍ إلغاء تثبيت", "Disinstallazione di", "Desinstalando", "Desinstalando"]),
    ("M_UNDONE", ["Kaldırma tamamlandı.", "Désinstallation terminée.", "Uninstallation complete.", "اكتملت إزالة التثبيت.", "Disinstallazione completata.", "Desinstalação concluída.", "Desinstalación completada."]),
    ("M_NOTINST", ["Bu paket kurulu değil.", "Ce paquet n'est pas installé.", "This package is not installed.", "هذه الحزمة غير مثبّتة.", "Questo pacchetto non è installato.", "Este pacote não está instalado.", "Este paquete no está instalado."]),
    ("M_USAGE", ["Kullanım", "Utilisation", "Usage", "الاستخدام", "Uso", "Uso", "Uso"]),
    ("M_O_INSTALL", ["kur (varsayılan)", "installer (par défaut)", "install (default)", "تثبيت (افتراضي)", "installa (predefinito)", "instalar (padrão)", "instalar (predeterminado)"]),
    ("M_O_UNINSTALL", ["kaldır", "désinstaller", "uninstall", "إلغاء التثبيت", "disinstalla", "desinstalar", "desinstalar"]),
    ("M_O_LIST", ["dosyaları listele", "lister les fichiers", "list files", "عرض قائمة الملفات", "elenca i file", "listar os arquivos", "listar los archivos"]),
    ("M_O_EXTRACT", [
        "kurmadan bir klasöre çıkar",
        "extraire dans un dossier sans installer",
        "extract into a folder without installing",
        "الاستخراج إلى مجلد دون تثبيت",
        "estrai in una cartella senza installare",
        "extrair para uma pasta sem instalar",
        "extraer en una carpeta sin instalar",
    ]),
    ("M_O_INFO", ["bilgileri göster", "afficher les informations", "show information", "عرض المعلومات", "mostra le informazioni", "mostrar as informações", "mostrar la información"]),
    ("M_O_YES", ["tüm sorulara evet de", "répondre oui à toutes les questions", "answer yes to every question", "الإجابة بنعم على كل الأسئلة", "rispondi sì a tutte le domande", "responder sim a todas as perguntas", "responder sí a todas las preguntas"]),
];

const LANGS: [&str; 7] = ["tr", "fr", "en", "ar", "it", "pt", "es"];

/// Bloc shell qui choisit la langue d'après $LANG et définit les messages.
fn lang_block() -> String {
    let mut s = String::from(
        "tamga_l=\"${LC_ALL:-${LC_MESSAGES:-${LANG:-en}}}\"\n\
         case \"$tamga_l\" in tr*) TL=tr;; fr*) TL=fr;; ar*) TL=ar;; it*) TL=it;; pt*) TL=pt;; es*) TL=es;; *) TL=en;; esac\n\
         case \"$TL\" in\n",
    );
    for (i, lang) in LANGS.iter().enumerate() {
        s.push_str(&format!("{lang})\n"));
        for (key, texts) in MESSAGES {
            s.push_str(&format!("  {key}={}\n", sh_quote(texts[i])));
        }
        s.push_str("  ;;\n");
    }
    s.push_str("esac\n");
    s
}

/// Options de l'installateur .sh.
pub struct ShWriteOpts {
    /// Bibliothèques (sonames) à vérifier sur la machine cible.
    pub needed_libs: Vec<String>,
    /// Paquets suggérés par bibliothèque : (soname, "apt:x dnf:y zypper:z pacman:w").
    pub hints: Vec<(String, String)>,
    /// glibc minimale.
    pub glibc: Option<String>,
    /// Inclure les scripts d'origine.
    pub include_scripts: bool,
    pub integration: Integration,
}

fn ldconfig_arch_tag(arch: &str) -> &'static str {
    match arch {
        "x86_64" => "x86-64",
        "aarch64" => "AArch64",
        _ => "",
    }
}

/// Génère le texte du script (sans la charge utile).
pub fn script_text(meta: &Metadata, o: &ShWriteOpts) -> Result<String> {
    let name = crate::version::deb_name(&meta.name);
    let ver = crate::version::deb_version(meta);
    let arch = crate::version::canonical_arch(&meta.arch);
    let meta_json = serde_json::to_vec(meta)?;
    let meta_b64 = base64::engine::general_purpose::STANDARD.encode(meta_json);
    let s_inc = |ev, new_v: &str, old_v: &str| -> String {
        if o.include_scripts { scripts::invoke(&meta.scripts, ev, new_v, old_v).unwrap_or_default() } else { String::new() }
    };
    let integ = o.integration.snippet();

    let mut hints = String::new();
    for (lib, h) in &o.hints {
        hints.push_str(&format!("    {}) echo {} ;;\n", sh_case_pattern(lib), sh_quote(h)));
    }

    // Script de désinstallation (écrit sur la machine au moment de l'installation).
    let uninstall = format!(
        r#"#!/bin/sh
# Désinstallation de {name} — créé par TAMGA / uninstaller generated by TAMGA
set -u
{lang}
if [ "$(id -u)" != 0 ]; then
  if command -v sudo >/dev/null 2>&1; then exec sudo sh "$0" "$@"; fi
  if command -v pkexec >/dev/null 2>&1; then exec pkexec sh "$(readlink -f "$0")" "$@"; fi
  echo "$M_NOROOT" >&2; exit 1
fi
DB=/var/lib/tamga
N={qname}
LIST="$DB/$N.files"
if [ ! -f "$LIST" ]; then echo "$M_NOTINST" >&2; exit 1; fi
echo "$M_UNINST $N"
{pre_remove}
while IFS= read -r f; do
  if [ -L "/$f" ] || [ -f "/$f" ]; then rm -f "/$f"; fi
done < "$LIST"
sort -r "$LIST" | while IFS= read -r f; do
  if [ -d "/$f" ] && [ ! -L "/$f" ]; then rmdir "/$f" 2>/dev/null || :; fi
done
if [ -f "$DB/$N.backup.tar.gz" ]; then
  tar -xzpf "$DB/$N.backup.tar.gz" -C / && rm -f "$DB/$N.backup.tar.gz"
fi
{integ}{post_remove}
rm -f "$LIST" "$DB/$N.info"
rm -f "$0"
echo "$M_UNDONE"
exit 0
"#,
        name = name,
        qname = sh_quote(&name),
        lang = lang_block(),
        pre_remove = s_inc(Event::PreRemove, &ver, ""),
        post_remove = s_inc(Event::PostRemove, &ver, ""),
        integ = integ,
    );
    let un_delim = "TAMGA_UNINSTALL_EOF_5f3a9c";

    let libs = o.needed_libs.join(" ");
    let script = format!(
        r#"#!/bin/sh
# ==========================================================================
#  {name} {ver} ({arch})
#  Installateur autonome créé par TAMGA {tamga} — self-extracting installer
#  generated by TAMGA. Options : --help
# ==========================================================================
# tamga-sh-format: 1
{meta_prefix}{meta_b64}
set -u
umask 022
TAMGA_NAME={qname}
TAMGA_VERSION={qver}
TAMGA_ARCH={qarch}
TAMGA_GLIBC={qglibc}
TAMGA_LIBS={qlibs}
TAMGA_LDTAG={qldtag}
TAMGA_DB=/var/lib/tamga
{lang}
SELF=$(readlink -f "$0" 2>/dev/null || echo "$0")
ACTION=install
ASSUME_YES=0
SKIP_DEPS=0
EXTRACT_DIR=
while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help) ACTION=help ;;
    --info) ACTION=info ;;
    --list) ACTION=list ;;
    --extract) ACTION=extract; EXTRACT_DIR="${{2:-}}"; [ $# -gt 1 ] && shift ;;
    --uninstall) ACTION=uninstall ;;
    -y|--yes) ASSUME_YES=1 ;;
    --skip-deps) SKIP_DEPS=1 ;;
    *) ACTION=help ;;
  esac
  shift
done

payload() {{
  tamga_line=$(awk '/^{marker}$/ {{ print NR + 1; exit 0 }}' "$SELF")
  tail -n +"$tamga_line" "$SELF" | gzip -dc
}}

ask() {{
  [ "$ASSUME_YES" = 1 ] && return 0
  [ -t 0 ] || return 1
  printf '%s' "$M_ASK"
  read -r tamga_a || return 1
  case "$tamga_a" in y|Y|yes|o|O|oui|e|E|evet|s|S|si|sí|sim|نعم) return 0 ;; esac
  return 1
}}

be_root() {{
  [ "$(id -u)" = 0 ] && return 0
  echo "$M_ROOT"
  tamga_r=""
  [ "$ASSUME_YES" = 1 ] && tamga_r="$tamga_r --yes"
  [ "$ACTION" = uninstall ] && tamga_r="$tamga_r --uninstall"
  if command -v sudo >/dev/null 2>&1; then exec sudo sh "$SELF" $tamga_r --skip-deps; fi
  if command -v pkexec >/dev/null 2>&1; then exec pkexec sh "$SELF" $tamga_r --skip-deps; fi
  echo "$M_NOROOT" >&2
  exit 1
}}

ver_ge() {{
  awk -v a="$1" -v b="$2" 'BEGIN {{ n = split(a, x, "."); m = split(b, y, "."); k = (n > m ? n : m);
    for (i = 1; i <= k; i++) {{ if ((x[i] + 0) > (y[i] + 0)) exit 0; if ((x[i] + 0) < (y[i] + 0)) exit 1 }} exit 0 }}'
}}

hint_for() {{
  case "$1" in
{hints}    *) echo "" ;;
  esac
}}

check_deps() {{
  tamga_bad=0
  if [ "$TAMGA_ARCH" != noarch ]; then
    tamga_m=$(uname -m)
    case "$tamga_m" in armv7*|armv8l) tamga_m=armv7 ;; i?86) tamga_m=i686 ;; esac
    if [ "$tamga_m" != "$TAMGA_ARCH" ]; then
      printf "$M_ARCH\n" "$TAMGA_ARCH" "$tamga_m" >&2
      exit 1
    fi
  fi
  [ -n "$TAMGA_LIBS$TAMGA_GLIBC" ] || return 0
  echo "$M_CHECK"
  if [ -n "$TAMGA_GLIBC" ]; then
    tamga_g=$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{{ print $2 }}')
    if [ -n "$tamga_g" ] && ! ver_ge "$tamga_g" "$TAMGA_GLIBC"; then
      printf "$M_GLIBC\n" "$TAMGA_GLIBC" "$tamga_g"
      tamga_bad=1
    fi
  fi
  tamga_ldc=$(command -v ldconfig 2>/dev/null || true)
  [ -n "$tamga_ldc" ] || for c in /sbin/ldconfig /usr/sbin/ldconfig /usr/bin/ldconfig; do [ -x "$c" ] && tamga_ldc=$c && break; done
  tamga_cache=$([ -n "$tamga_ldc" ] && "$tamga_ldc" -p 2>/dev/null || true)
  tamga_missing=""
  for lib in $TAMGA_LIBS; do
    if printf '%s\n' "$tamga_cache" | awk -v n="$lib" -v t="$TAMGA_LDTAG" '$1 == n && (t == "" || index($0, t)) {{ f = 1 }} END {{ exit !f }}'; then continue; fi
    tamga_found=0
    for d in /usr/lib64 /lib64 /usr/lib /lib /usr/lib/*-linux-gnu /lib/*-linux-gnu /usr/local/lib; do
      if [ -e "$d/$lib" ]; then tamga_found=1; break; fi
    done
    [ "$tamga_found" = 1 ] && continue
    tamga_missing="$tamga_missing $lib"
  done
  if [ -n "$tamga_missing" ]; then
    echo "$M_MISSING"
    tamga_pm=""
    for p in apt-get dnf zypper pacman yum; do if command -v $p >/dev/null 2>&1; then tamga_pm=$p; break; fi; done
    tamga_key=$tamga_pm; [ "$tamga_pm" = apt-get ] && tamga_key=apt; [ "$tamga_pm" = yum ] && tamga_key=dnf
    tamga_pkgs=""
    for lib in $tamga_missing; do
      echo "  - $lib"
      for h in $(hint_for "$lib"); do
        case "$h" in "$tamga_key":*) tamga_pkgs="$tamga_pkgs ${{h#*:}}" ;; esac
      done
    done
    if [ -n "$tamga_pkgs" ]; then
      echo "$M_HINT"
      case "$tamga_pm" in
        apt-get) echo "  sudo apt install$tamga_pkgs" ;;
        dnf|yum) echo "  sudo $tamga_pm install$tamga_pkgs" ;;
        zypper) echo "  sudo zypper install$tamga_pkgs" ;;
        pacman) echo "  sudo pacman -S$tamga_pkgs" ;;
      esac
    fi
    tamga_bad=1
  fi
  if [ "$tamga_bad" = 1 ]; then
    ask || {{ echo "$M_ABORT"; exit 1; }}
  fi
}}

do_install() {{
  echo "$M_INSTALLING $TAMGA_NAME $TAMGA_VERSION"
  mkdir -p "$TAMGA_DB"
  tamga_list="$TAMGA_DB/$TAMGA_NAME.files"
  tamga_new=$(mktemp)
  payload | tar -tf - | sed -e 's#^\./##' -e 's#/$##' | grep -v '^$' > "$tamga_new"
  tamga_upgrade=0
  if [ -f "$tamga_list" ]; then
    tamga_upgrade=1
    tamga_old=$(sed -n 's/^version=//p' "$TAMGA_DB/$TAMGA_NAME.info" 2>/dev/null)
    printf "$M_PREV\n" "${{tamga_old:-?}}"
  fi
  tamga_conf=$(mktemp)
  while IFS= read -r f; do
    if [ -e "/$f" ] || [ -L "/$f" ]; then
      if [ -d "/$f" ] && [ ! -L "/$f" ]; then continue; fi
      if [ "$tamga_upgrade" = 1 ] && grep -Fxq "$f" "$tamga_list"; then continue; fi
      printf '%s\n' "$f" >> "$tamga_conf"
    fi
  done < "$tamga_new"
  tamga_n=$(wc -l < "$tamga_conf" | tr -d ' ')
  if [ "$tamga_n" -gt 0 ]; then
    printf "$M_OVER\n" "$tamga_n"
    head -n 10 "$tamga_conf" | sed 's#^#  /#'
    ask || {{ echo "$M_ABORT"; rm -f "$tamga_new" "$tamga_conf"; exit 1; }}
    (cd / && tar -czf "$TAMGA_DB/$TAMGA_NAME.backup.tar.gz" -T "$tamga_conf") 2>/dev/null || :
  fi
  rm -f "$tamga_conf"
  if [ "$tamga_upgrade" = 1 ]; then
    tamga_oldv="${{tamga_old:-}}"
    : "$tamga_oldv"
{pre_upgrade}  else
    :
{pre_install}  fi
  echo "$M_COPY"
  if ! payload | tar -xpf - -C /; then
    echo "$M_ABORT" >&2; rm -f "$tamga_new"; exit 1
  fi
  if [ "$tamga_upgrade" = 1 ]; then
    grep -Fxv -f "$tamga_new" "$tamga_list" | while IFS= read -r f; do
      if [ -L "/$f" ] || [ -f "/$f" ]; then rm -f "/$f"; fi
    done
  fi
  mv -f "$tamga_new" "$tamga_list"
  printf 'name=%s\nversion=%s\narch=%s\ninstaller=TAMGA {tamga}\n' "$TAMGA_NAME" "$TAMGA_VERSION" "$TAMGA_ARCH" > "$TAMGA_DB/$TAMGA_NAME.info"
  cat > "$TAMGA_DB/$TAMGA_NAME.uninstall" <<'{un_delim}'
{uninstall}{un_delim}
  chmod 755 "$TAMGA_DB/$TAMGA_NAME.uninstall"
{integ}  if [ "$tamga_upgrade" = 1 ]; then
    :
{post_upgrade}  else
    :
{post_install}  fi
  echo "$M_DONE"
  echo "$M_UNHINT"
  echo "  sudo sh $TAMGA_DB/$TAMGA_NAME.uninstall"
}}

case "$ACTION" in
  help)
    echo "$TAMGA_NAME $TAMGA_VERSION ($TAMGA_ARCH) — TAMGA"
    echo "$M_USAGE: sh $(basename "$0") [option]"
    echo "  (—)              $M_O_INSTALL"
    echo "  --uninstall      $M_O_UNINSTALL"
    echo "  --list           $M_O_LIST"
    echo "  --extract DIR    $M_O_EXTRACT"
    echo "  --info           $M_O_INFO"
    echo "  -y, --yes        $M_O_YES"
    ;;
  info)
    echo "name=$TAMGA_NAME"; echo "version=$TAMGA_VERSION"; echo "arch=$TAMGA_ARCH"
    [ -n "$TAMGA_GLIBC" ] && echo "glibc>=$TAMGA_GLIBC"
    for l in $TAMGA_LIBS; do echo "needs=$l"; done
    ;;
  list) payload | tar -tf - ;;
  extract)
    [ -n "$EXTRACT_DIR" ] || {{ echo "--extract DIR" >&2; exit 1; }}
    mkdir -p "$EXTRACT_DIR" && payload | tar -xf - -C "$EXTRACT_DIR"
    ;;
  uninstall)
    be_root
    if [ -f "$TAMGA_DB/$TAMGA_NAME.uninstall" ]; then exec sh "$TAMGA_DB/$TAMGA_NAME.uninstall"; fi
    echo "$M_NOTINST" >&2; exit 1
    ;;
  install)
    [ "$SKIP_DEPS" = 1 ] || check_deps
    be_root
    do_install
    ;;
esac
exit 0
{marker}
"#,
        name = name,
        ver = ver,
        arch = arch,
        tamga = crate::TAMGA_VERSION,
        meta_prefix = META_PREFIX,
        meta_b64 = meta_b64,
        qname = sh_quote(&name),
        qver = sh_quote(&ver),
        qarch = sh_quote(&arch),
        qglibc = sh_quote(o.glibc.as_deref().unwrap_or("")),
        qlibs = sh_quote(&libs),
        qldtag = sh_quote(ldconfig_arch_tag(&arch)),
        lang = lang_block(),
        marker = MARKER,
        hints = hints,
        pre_install = indent(&s_inc(Event::PreInstall, &ver, "")),
        pre_upgrade = indent(&s_inc(Event::PreUpgrade, &ver, "$tamga_oldv")),
        post_install = indent(&s_inc(Event::PostInstall, &ver, "")),
        post_upgrade = indent(&s_inc(Event::PostUpgrade, &ver, "$tamga_oldv")),
        integ = indent(&integ),
        uninstall = uninstall,
        un_delim = un_delim,
    );
    Ok(script)
}

fn indent(s: &str) -> String {
    // Les heredocs internes doivent rester en début de ligne : on n'indente pas.
    s.to_string()
}

/// Motif « case » qui correspond exactement au nom (caractères spéciaux échappés).
fn sh_case_pattern(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || "._-".contains(c) {
            out.push(c);
        } else {
            out.push('\\');
            out.push(c);
        }
    }
    out
}

/// Écrit l'installateur .sh.
pub fn write(meta: &Metadata, tree: &Tree, o: &ShWriteOpts, out: &Path) -> Result<()> {
    let script = script_text(meta, o)?;
    let file = File::create(out).with_context(|| format!("création de {}", out.display()))?;
    let mut w = BufWriter::new(file);
    w.write_all(script.as_bytes())?;
    {
        let enc = compress::encoder(Codec::Gzip, &mut w, Some(9))?;
        let mut b = tar::Builder::new(enc);
        let opts = TarOpts { dot_prefix: true, root_entry: false, numeric_owner_fallback: true };
        for (p, e) in tree.entries.iter() {
            // Les dossiers du système ne sont pas inscrits (ils existent déjà).
            if e.is_dir() && crate::fsdirs::is_system_dir(p) {
                continue;
            }
            tarutil::append_entry(&mut b, tree, p, e, &opts)?;
        }
        let enc = b.into_inner()?;
        enc.finish_box()?;
    }
    w.flush()?;
    drop(w);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(out, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// Vrai si le début du fichier ressemble à un installateur TAMGA.
pub fn is_tamga_sh(head: &[u8]) -> bool {
    head.starts_with(b"#!") && head.windows(META_PREFIX.len()).any(|w| w == META_PREFIX.as_bytes())
}

/// Relit un installateur .sh créé par TAMGA.
pub fn read(path: &Path) -> Result<Package> {
    let mut f = File::open(path).with_context(|| format!("ouverture de {}", path.display()))?;
    // Recherche de la ligne de métadonnées et du marqueur dans le texte du script.
    let mut head = vec![0u8; 4 * 1024 * 1024];
    let n = read_up_to(&mut f, &mut head)?;
    head.truncate(n);
    if !is_tamga_sh(&head) {
        bail!("ce script .sh n'a pas été créé par TAMGA : impossible de savoir où vont ses fichiers");
    }
    let text_end = find(&head, format!("\n{MARKER}\n").as_bytes()).context("installateur .sh incomplet (marqueur absent)")?;
    let payload_off = (text_end + MARKER.len() + 2) as u64;
    let text = String::from_utf8_lossy(&head[..text_end]);
    let b64 = text
        .lines()
        .find_map(|l| l.strip_prefix(META_PREFIX))
        .context("métadonnées TAMGA absentes")?;
    let json = base64::engine::general_purpose::STANDARD.decode(b64.trim()).context("métadonnées TAMGA illisibles")?;
    let mut meta: Metadata = serde_json::from_slice(&json).context("métadonnées TAMGA illisibles")?;
    if meta.origin_format.is_none() {
        meta.origin_format = Some(Format::Sh);
    }
    f.seek(SeekFrom::Start(payload_off))?;
    let mut dec = compress::auto_decoder(BufReader::new(f))?;
    let mut tree = Tree::new()?;
    tarutil::read_tar_into(&mut dec, &mut tree, |_| Sink::Tree)?;
    Ok(Package { meta, tree, format: Format::Sh, warnings: vec![] })
}

fn read_up_to(r: &mut dyn Read, buf: &mut [u8]) -> Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        let k = r.read(&mut buf[n..])?;
        if k == 0 {
            break;
        }
        n += k;
    }
    Ok(n)
}

fn find(h: &[u8], needle: &[u8]) -> Option<usize> {
    h.windows(needle.len()).position(|w| w == needle)
}
