// TAMGA — logique de l'interface / UI logic
// Les écrans : legal → home → busy → package → deps → result (ou error).

const TAURI = window.__TAURI__ || null;
const invoke = (cmd, args) => TAURI ? TAURI.core.invoke(cmd, args) : Promise.reject("Tauri indisponible");

const LEGAL_VERSION = "1";
const TARGETS = [
  { id: "deb", ext: ".deb", desc: "target.deb" },
  { id: "rpm", ext: ".rpm", desc: "target.rpm" },
  { id: "arch", ext: ".pkg.tar.zst", desc: "target.arch" },
  { id: "sh", ext: ".sh", desc: "target.sh" },
  { id: "slack", ext: ".txz", desc: "target.slack" },
  { id: "tar", ext: ".tar.*", desc: "target.tar" },
  { id: "slp", ext: ".slp", desc: "target.slp" }
];
// Nom affiché du format d'origine.
const FORMAT_LABEL = {
  deb: ".deb", rpm: ".rpm", arch: ".pkg.tar.zst", sh: ".sh",
  slack: "Slackware", tar: ".tar", slp: ".slp", solaris: "Solaris .pkg"
};
// Formats sans dépendances déclarées : on vérifie seulement les bibliothèques.
const GENERIC = ["sh", "slack", "tar", "slp"];
// Compressions proposées.
const COMPRESSIONS = {
  tar: [["gz", ".tar.gz"], ["xz", ".tar.xz"], ["zst", ".tar.zst"], ["bz2", ".tar.bz2"], ["none", ".tar"]],
  slack: [["xz", ".txz"], ["gz", ".tgz"], ["bz2", ".tbz"]]
};

const state = {
  sys: null,
  analysis: null,
  target: null,
  deps: [],
  extraDeps: [],
  result: null,
  installable: [],
  installed: false,
  busyTimer: null
};

const $ = id => document.getElementById(id);
const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};

function fmtSize(n) {
  if (n == null) return "—";
  const u = ["o", "Ko", "Mo", "Go"];
  let i = 0, v = n;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  const num = i ? v.toLocaleString(currentLang, { maximumFractionDigits: 1 }) : String(v);
  return num + " " + t("unit." + u[i]);
}

function show(id) {
  document.querySelectorAll(".screen").forEach(s => { s.hidden = s.id !== id; });
  $("lang-selector").hidden = !(id === "screen-home" || id === "screen-legal");
  window.scrollTo(0, 0);
}

function toast(msg) {
  const tEl = $("toast");
  tEl.textContent = msg;
  tEl.hidden = false;
  clearTimeout(toast._t);
  toast._t = setTimeout(() => { tEl.hidden = true; }, 1800);
}

// ---------------------------------------------------------------- attente
function busy(titleKey, file) {
  $("busy-title").textContent = t(titleKey);
  $("busy-title").dataset.key = titleKey;
  $("busy-file").textContent = file || "";
  const start = Date.now();
  clearInterval(state.busyTimer);
  $("busy-time").textContent = "";
  state.busyTimer = setInterval(() => {
    const s = Math.floor((Date.now() - start) / 1000);
    $("busy-time").textContent = s >= 3 ? "(" + s + " s)" : "";
  }, 500);
  show("screen-busy");
}
function stopBusy() { clearInterval(state.busyTimer); }

// ---------------------------------------------------------------- erreurs
function showError(err) {
  stopBusy();
  const raw = String(err && err.message ? err.message : err);
  const key = raw.split(/[\s:]/)[0];
  if (key.startsWith("err.") && hasT(key)) {
    $("error-message").textContent = t(key);
    $("error-detail").textContent = "";
  } else {
    $("error-message").textContent = t("err.generic");
    $("error-detail").textContent = raw;
  }
  show("screen-error");
}

// ---------------------------------------------------------------- système
function systemLabel() {
  const s = state.sys;
  if (!s) return "";
  const fam = s.family ? t("family." + s.family) : t("home.system_unknown");
  return (s.name || s.id || "Linux") + " · " + fam;
}

async function loadSystem() {
  try {
    state.sys = await invoke("system_info");
    const b = $("sys-badge");
    b.textContent = t("home.system") + " : " + systemLabel();
    b.title = b.textContent;
    b.hidden = false;
  } catch (e) { console.error(e); }
}

// ---------------------------------------------------------------- analyse
async function openFile(path) {
  busy("busy.analyzing", baseName(path));
  try {
    state.analysis = await invoke("analyze", { path });
    state.target = null;
    state.extraDeps = [];
    stopBusy();
    renderPackage();
    show("screen-package");
  } catch (e) { showError(e); }
}

function renderPackage() {
  const a = state.analysis;
  if (!a) return;
  const m = a.meta;
  $("pkg-name").textContent = m.name;
  $("pkg-version").textContent = (m.epoch ? m.epoch + ":" : "") + m.version + (m.release ? "-" + m.release : "");
  $("pkg-summary").textContent = m.summary || "";
  $("pkg-format").textContent = FORMAT_LABEL[a.format] || a.format;
  $("pkg-arch").textContent = m.arch;
  $("pkg-files").textContent = a.file_count;
  $("pkg-size").textContent = fmtSize(a.installed_size);
  $("pkg-libs").textContent = Object.keys(a.elf.needed || {}).length;
  $("pkg-glibc").textContent = a.elf.glibc || "—";
  $("pkg-scripts").textContent = a.has_scripts ? t("pkg.yes") : t("pkg.no");

  const w = $("pkg-warnings");
  w.innerHTML = "";
  (a.warnings || []).forEach(k => w.appendChild(el("div", "note", t(k))));
  w.hidden = !a.warnings || !a.warnings.length;

  const list = $("pkg-file-list");
  list.innerHTML = "";
  a.files_preview.forEach(f => list.appendChild(el("li", "", f)));
  if (a.file_count > a.files_preview.length) list.appendChild(el("li", "", "… (+" + (a.file_count - a.files_preview.length) + ")"));

  // Cibles
  const box = $("targets");
  box.innerHTML = "";
  TARGETS.forEach(tg => {
    const b = el("button", "target");
    b.type = "button";
    b.dataset.id = tg.id;
    b.appendChild(el("div", "target-ext", tg.ext));
    b.appendChild(el("div", "target-desc", tg.id === a.format ? t("same_format") : t(tg.desc)));
    b.disabled = tg.id === a.format;
    b.addEventListener("click", () => selectTarget(tg.id));
    box.appendChild(b);
  });
  if (!state.target) {
    // Par défaut : le format de cette machine, sinon le premier possible.
    const fam = state.sys && state.sys.family;
    const pref = { debian: "deb", fedora: "rpm", suse: "rpm", arch: "arch" }[fam];
    state.target = (pref && pref !== a.format) ? pref : TARGETS.find(x => x.id !== a.format).id;
  }
  selectTarget(state.target);

  $("opt-scripts-row").hidden = !a.has_scripts;
  $("opt-scripts").checked = false;
  $("edit-name").value = m.name;
  $("edit-version").value = m.version;
  $("edit-release").value = m.release || "";
  $("edit-license").value = m.license || "";
  $("edit-summary").value = m.summary || "";
  $("edit-maintainer").value = m.maintainer || "";
  $("edit-url").value = m.url || "";
}

function selectTarget(id) {
  state.target = id;
  document.querySelectorAll(".target").forEach(b => b.classList.toggle("selected", b.dataset.id === id));
  fillDistros();
  fillCompression();
}

function fillCompression() {
  const opts = COMPRESSIONS[state.target];
  $("compression-row").hidden = !opts;
  if (!opts) return;
  const sel = $("compression");
  const prev = sel.value;
  sel.innerHTML = "";
  opts.forEach(([v, label]) => {
    const o = el("option", "", label);
    o.value = v;
    sel.appendChild(o);
  });
  if (opts.some(o => o[0] === prev)) sel.value = prev;
}

function compression() {
  return COMPRESSIONS[state.target] ? $("compression").value : null;
}

function canInstall(fmt) {
  return state.installable.includes(fmt);
}

function fillDistros() {
  const sel = $("distro");
  const prev = sel.value;
  sel.innerHTML = "";
  const fam = state.sys && state.sys.family;
  const fmtOfFam = { debian: "deb", fedora: "rpm", suse: "rpm", arch: "arch" }[fam];
  const opts = [];
  if (GENERIC.includes(state.target)) {
    opts.push(["any", t("distro.any")]);
  } else {
    if (fmtOfFam === state.target) opts.push(["this_system", t("distro.this", { name: state.sys.name || state.sys.id })]);
    if (state.target === "deb") opts.push(["debian", t("distro.debian")]);
    if (state.target === "rpm") { opts.push(["fedora", t("distro.fedora")]); opts.push(["suse", t("distro.suse")]); }
    if (state.target === "arch") opts.push(["arch", t("distro.arch")]);
  }
  opts.forEach(([v, label]) => {
    const o = el("option", "", label);
    o.value = v;
    sel.appendChild(o);
  });
  if (opts.some(o => o[0] === prev)) sel.value = prev;
  else if (fam === "suse" && state.target === "rpm" && opts.some(o => o[0] === "this_system")) sel.value = "this_system";
}

function metaOverride() {
  const v = id => $(id).value;
  return {
    name: v("edit-name"),
    version: v("edit-version"),
    release: v("edit-release"),
    summary: v("edit-summary"),
    maintainer: v("edit-maintainer"),
    license: v("edit-license"),
    url: v("edit-url")
  };
}

// ---------------------------------------------------------------- dépendances
async function checkDeps() {
  if (!$("edit-name").value.trim()) { toast(t("err.empty_name")); return; }
  busy("busy.resolving", state.analysis.file_name);
  try {
    state.deps = await invoke("resolve_deps", {
      format: state.target,
      distro: $("distro").value,
      online: $("opt-online").checked
    });
    stopBusy();
    renderDeps();
    show("screen-deps");
  } catch (e) { showError(e); }
}

function depText(d) {
  if (!d) return "—";
  return d.op && d.version ? d.name + " " + ({ Lt: "<", Le: "<=", Eq: "=", Ge: ">=", Gt: ">" }[d.op]) + " " + d.version : d.name;
}

function renderDeps() {
  const tb = $("deps-body");
  tb.innerHTML = "";
  const target = TARGETS.find(x => x.id === state.target);
  const comp = COMPRESSIONS[state.target] && $("compression").selectedOptions[0];
  $("deps-target").textContent = "→ " + (comp ? comp.textContent : target.ext) + " · " + $("distro").selectedOptions[0].textContent;
  $("deps-note").textContent = state.target === "sh" ? t("deps.sh_note") : (GENERIC.includes(state.target) ? t("deps.generic_note") : "");
  $("btn-convert-install").hidden = !canInstall(state.target);

  const rows = state.deps.concat(state.extraDeps);
  rows.forEach((d, i) => {
    const tr = el("tr");
    const src = el("td", "mono", d.source);
    const tgt = el("td", "mono", depText(d.target));
    if (d.method && d.method !== "none") tgt.appendChild(el("span", "method", t("method." + d.method)));
    const st = el("td");
    const chip = el("span", "chip " + d.status, t("deps." + d.status));
    if (d.detail) chip.title = d.detail;
    st.appendChild(chip);
    const keep = el("td");
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = !!d.keep;
    cb.disabled = !d.target;
    cb.addEventListener("change", () => { d.keep = cb.checked; tr.classList.toggle("off", !cb.checked); updateMissing(); });
    keep.appendChild(cb);
    tr.classList.toggle("off", !d.keep);
    tr.append(src, tgt, st, keep);
    tb.appendChild(tr);
  });
  $("deps-empty").hidden = rows.length > 0;
  updateMissing();
}

function hasMissing() {
  return state.deps.concat(state.extraDeps).some(d => d.status === "missing" && (d.keep || !d.target));
}

function updateMissing() {
  const missing = hasMissing();
  $("deps-missing").hidden = !missing;
  $("deps-actions").hidden = missing;
}

function addDep() {
  const v = $("dep-add-input").value.trim();
  if (!v) return;
  const m = v.match(/^([^\s<>=]+)\s*(>=|<=|=|>|<)?\s*(\S+)?$/);
  if (!m) return;
  const ops = { ">=": "Ge", "<=": "Le", "=": "Eq", ">": "Gt", "<": "Lt" };
  const target = { name: m[1], op: m[2] ? ops[m[2]] : null, version: m[2] ? m[3] : null };
  state.extraDeps.push({ source: v, origin: "declared", target, status: "estimated", method: "manual", detail: null, keep: true });
  $("dep-add-input").value = "";
  renderDeps();
}

// ---------------------------------------------------------------- conversion
async function doConvert(installAfter) {
  const meta = metaOverride();
  let output;
  try {
    const suggested = await invoke("default_output", { format: state.target, meta, compression: compression() });
    output = await TAURI.dialog.save({
      title: t("save.title"),
      defaultPath: suggested
    });
  } catch (e) { showError(e); return; }
  if (!output) return; // annulé
  const deps = state.deps.concat(state.extraDeps).filter(d => d.keep && d.target).map(d => d.target);
  busy("busy.converting", baseName(output));
  try {
    state.result = await invoke("convert_package", {
      options: {
        format: state.target,
        distro: $("distro").value,
        meta,
        deps,
        include_scripts: $("opt-scripts").checked,
        usrmerge: $("opt-usrmerge").checked,
        online: $("opt-online").checked,
        output,
        compression: compression()
      }
    });
    state.installed = false;
    $("install-status").hidden = true;
    $("install-log-box").hidden = true;
    $("install-log").textContent = "";
    stopBusy();
    renderResult();
    show("screen-result");
    if (installAfter === true) await runInstall();
  } catch (e) { showError(e); }
}

// ---------------------------------------------------------------- installation
async function runInstall() {
  const r = state.result;
  if (!r) return;
  $("install-log").textContent = "";
  $("busy-log").textContent = "";
  $("busy-log").hidden = false;
  busy("busy.installing", baseName(r.path));
  let ok = false, msg = "";
  try {
    const res = await invoke("install_package", { path: r.path, format: r.format });
    ok = res.code === 0;
    msg = ok ? t("install.done") : t("install.failed");
  } catch (e) {
    const raw = String(e && e.message ? e.message : e);
    const key = raw.split(/[\s:]/)[0];
    msg = key.startsWith("err.") && hasT(key) ? t(key) : t("install.failed") + " " + raw;
  }
  stopBusy();
  $("busy-log").hidden = true;
  state.installed = ok;
  const st = $("install-status");
  st.textContent = msg;
  st.className = "install-status " + (ok ? "ok" : "bad");
  st.hidden = false;
  $("install-log").textContent = $("busy-log").textContent;
  $("install-log-box").hidden = !$("install-log").textContent;
  renderResult();
  show("screen-result");
}

function appendInstallLog(line) {
  const pre = $("busy-log");
  pre.textContent += line + "\n";
  pre.scrollTop = pre.scrollHeight;
}

function renderResult() {
  const r = state.result;
  $("result-path").textContent = r.path;
  $("result-size").textContent = fmtSize(r.size);
  $("result-cmd").textContent = r.install_command;
  const w = $("result-warnings");
  w.innerHTML = "";
  (r.warnings || []).filter(k => k !== "warn.missing_deps").forEach(k => w.appendChild(el("div", "note", t(k))));
  w.hidden = !w.children.length;
  const ul = $("result-deps");
  ul.innerHTML = "";
  (r.depends || []).forEach(d => ul.appendChild(el("li", "", d)));
  $("result-deps-box").hidden = !(r.depends || []).length;
  $("btn-install").hidden = !canInstall(r.format) || state.installed;
}

// ---------------------------------------------------------------- démarrage
function goHome() {
  stopBusy();
  show("screen-home");
}

function bind() {
  document.querySelectorAll("[data-action='home']").forEach(b => b.addEventListener("click", goHome));
  document.querySelectorAll("[data-action='quit']").forEach(b => b.addEventListener("click", () => invoke("quit_app").catch(() => window.close())));
  $("brand-home").addEventListener("click", () => { if ($("screen-legal").hidden && $("screen-busy").hidden) goHome(); });
  $("btn-accept").addEventListener("click", () => { storageSet("tamga-legal", LEGAL_VERSION); goHome(); });
  $("btn-deps").addEventListener("click", checkDeps);
  $("btn-deps-back").addEventListener("click", () => show("screen-package"));
  $("btn-convert").addEventListener("click", () => doConvert(false));
  $("btn-convert-install").addEventListener("click", () => doConvert(true));
  $("btn-convert-anyway").addEventListener("click", () => doConvert(false));
  $("btn-install").addEventListener("click", runInstall);
  $("dep-add-btn").addEventListener("click", addDep);
  $("dep-add-input").addEventListener("keydown", e => { if (e.key === "Enter") addDep(); });
  $("btn-copy").addEventListener("click", async () => {
    const txt = $("result-cmd").textContent;
    try { await navigator.clipboard.writeText(txt); toast(t("action.copied")); }
    catch (e) {
      const r = document.createRange(); r.selectNodeContents($("result-cmd"));
      const s = window.getSelection(); s.removeAllRanges(); s.addRange(r);
      document.execCommand("copy"); toast(t("action.copied"));
    }
  });
  $("btn-open-folder").addEventListener("click", () => {
    if (TAURI && TAURI.opener) TAURI.opener.revealItemInDir(state.result.path).catch(err => console.error(err));
  });
  $("btn-other-format").addEventListener("click", () => { renderPackage(); show("screen-package"); });
  $("btn-about").addEventListener("click", () => { $("cache-msg").textContent = ""; $("about").showModal(); });
  $("btn-about-close").addEventListener("click", () => $("about").close());
  $("btn-legal").addEventListener("click", () => { $("about").close(); show("screen-legal"); });
  $("btn-cache").addEventListener("click", async () => {
    try { await invoke("clear_cache"); $("cache-msg").textContent = t("about.cache_done"); }
    catch (e) { $("cache-msg").textContent = String(e); }
  });
  // Textes générés en JS : à refaire au changement de langue.
  onLangChange(() => {
    if (state.sys) {
      $("sys-badge").textContent = t("home.system") + " : " + systemLabel();
    }
    if (!$("screen-busy").hidden && $("busy-title").dataset.key) $("busy-title").textContent = t($("busy-title").dataset.key);
    if (state.analysis && !$("screen-package").hidden) {
      const keepTarget = state.target;
      renderPackage();
      selectTarget(keepTarget);
    }
    if (!$("screen-deps").hidden) renderDeps();
    if (!$("screen-result").hidden && state.result) renderResult();
  });
}

async function start() {
  await initI18n();
  bind();
  initDropzone(openFile);
  await loadSystem();
  try { state.installable = await invoke("installable_formats"); } catch (e) { state.installable = []; }
  if (TAURI && TAURI.event) TAURI.event.listen("install-log", ev => appendInstallLog(String(ev.payload))).catch(() => {});
  if (storageGet("tamga-legal") === LEGAL_VERSION) show("screen-home");
  else show("screen-legal");
}

start();
