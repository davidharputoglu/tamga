// TAMGA — zone de dépôt (API native Tauri pour tout)
// Drop zone: native Tauri drag & drop and file dialog (real file paths).

const FORMATS = [
  { family: "arch",     ext: [".pkg.tar.zst", ".pkg.tar.xz", ".pkg.tar.gz", ".pkg.tar", ".pacman"] },
  { family: "deb",      ext: [".deb"] },
  { family: "rpm",      ext: [".rpm"] },
  { family: "sh",       ext: [".sh", ".run"] }
];

function detectFamily(name) {
  const n = name.toLowerCase();
  const all = FORMATS.flatMap(f => f.ext.map(e => ({ e, family: f.family })))
                     .sort((a, b) => b.e.length - a.e.length);
  for (const { e, family } of all) {
    if (n.endsWith(e)) return family;
  }
  return null;
}

/** Nom du fichier, quel que soit le séparateur (/ sous Linux, \ sous Windows). */
function baseName(path) {
  return String(path).split(/[\\/]/).pop();
}

/** Dossier contenant le fichier. */
function dirName(path) {
  const p = String(path);
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i > 0 ? p.slice(0, i) : p;
}

function initDropzone(onFile) {
  const zone = document.getElementById("dropzone");
  if (!zone) return;

  // Filet de sécurité DOM : le navigateur ne doit jamais ouvrir le fichier lui-même.
  window.addEventListener("dragover", e => e.preventDefault());
  window.addEventListener("drop", e => e.preventDefault());

  const pick = async () => {
    if (window.__TAURI__ && window.__TAURI__.dialog) {
      try {
        const path = await window.__TAURI__.dialog.open({
          multiple: false,
          directory: false,
          filters: [
            { name: t("dialog.packages"), extensions: ["deb", "rpm", "zst", "xz", "gz", "pacman", "sh", "run"] },
            { name: t("dialog.all_files"), extensions: ["*"] }
          ]
        });
        if (path) onFile(typeof path === "string" ? path : path.path);
      } catch (err) {
        console.error("TAMGA : erreur dialog.open", err);
      }
    }
  };

  // CLIC ou Entrée = fenêtre native de choix de fichier
  zone.addEventListener("click", pick);
  zone.addEventListener("keydown", e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); pick(); } });

  // DÉPÔT NATIF = événement Tauri (chemin réel)
  if (window.__TAURI__ && window.__TAURI__.webview) {
    window.__TAURI__.webview.getCurrentWebview().onDragDropEvent(event => {
      const p = event.payload;
      const onHome = !document.getElementById("screen-home").hidden;
      if (!onHome) return;
      if (p.type === "enter" || p.type === "over") zone.classList.add("dragover");
      if (p.type === "leave") zone.classList.remove("dragover");
      if (p.type === "drop") {
        zone.classList.remove("dragover");
        const paths = p.paths || [];
        if (paths.length > 0) onFile(paths[0]);
      }
    }).then(() => console.log("TAMGA : écouteur natif ACTIF"))
      .catch(err => console.error("TAMGA : erreur onDragDropEvent", err));
  }
}
