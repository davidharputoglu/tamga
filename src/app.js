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

// ---------------------------------------------------------------- runes
// Mots écrits en runes turciques (alphabet de l'Orkhon) autour du sceau pendant le travail.
// Par défaut « tamga : türük » ; l'utilisateur peut choisir ses propres mots (runes.js).
const runeState = { words: [], index: 0, timer: null };

function runeTitle() {
  const latin = runeState.words.map(w => w.latin).join(", ");
  return loadRuneText() === RUNE_DEFAULT ? t("busy.runes") : t("busy.runes_words", { words: latin });
}

function buildRunes() {
  const ring = $("rune-ring");
  if (!ring) return;
  runeState.words = parseRuneWords(loadRuneText());
  if (!runeState.words.length) runeState.words = parseRuneWords(RUNE_DEFAULT);
  runeState.index = 0;
  ring.innerHTML = "";
  // Anneau : les mots séparés par « : », répétés pour faire le tour, comme l'anneau du logo.
  const unit = [];
  runeState.words.forEach(w => { unit.push(...Array.from(w.runes), ":"); });
  const reps = Math.max(1, Math.round(30 / unit.length));
  const glyphs = [];
  for (let r = 0; r < reps; r++) glyphs.push(...unit);
  const n = glyphs.length;
  ring.style.setProperty("--step", (360 / n) + "deg");
  ring.style.setProperty("--delay", (3 / n) + "s");  // une vague de lumière fait le tour en 3 s
  ring.style.setProperty("--size", Math.max(12, Math.min(21, Math.floor(21 * 32 / n))) + "px");
  glyphs.forEach((g, i) => {
    const sp = el("span", "", g);
    sp.style.setProperty("--i", i);
    ring.appendChild(sp);
  });
  showRuneWord();
  const stage = document.querySelector(".rune-stage");
  if (stage) stage.title = runeTitle();
}

/** Affiche le mot courant sous le sceau, lettre par lettre (de droite à gauche). */
function showRuneWord() {
  const word = $("rune-word");
  const w = runeState.words[runeState.index % runeState.words.length];
  word.innerHTML = "";
  word.classList.remove("fade");
  void word.offsetWidth; // relance l'animation d'apparition
  word.classList.add("fade");
  Array.from(w.runes).forEach((g, i) => {
    const sp = el("span", "", g);
    sp.style.setProperty("--i", i);
    word.appendChild(sp);
  });
}

/** Plusieurs mots : ils défilent un par un pendant le travail. */
function startRuneWords() {
  clearInterval(runeState.timer);
  runeState.index = 0;
  showRuneWord();
  if (runeState.words.length < 2) return;
  runeState.timer = setInterval(() => { runeState.index++; showRuneWord(); }, 4000);
}

// Fenêtre « Vos mots en runes »
function renderRunePreview() {
  const words = parseRuneWords($("runes-input").value);
  const count = runeCount(words);
  const box = $("runes-preview");
  box.innerHTML = "";
  words.forEach(w => {
    const fig = el("figure");
    fig.appendChild(el("span", "r", w.runes));
    fig.appendChild(el("figcaption", "", w.latin));
    box.appendChild(fig);
  });
  const c = $("runes-count");
  let bad = false;
  if (!words.length) { c.textContent = t("runes.empty"); bad = true; }
  else if (count > RUNE_MAX || words.length > RUNE_MAX_WORDS) {
    c.textContent = t("runes.too_long", { max: RUNE_MAX, n: count }); bad = true;
  } else c.textContent = count + " / " + RUNE_MAX;
  c.classList.toggle("bad", bad);
  $("btn-runes-save").disabled = bad;
}

function openRunes() {
  $("runes-input").value = loadRuneText();
  renderRunePreview();
  $("runes-dialog").showModal();
  $("runes-input").focus();
}

function saveRunes() {
  const text = parseRuneWords($("runes-input").value).map(w => w.latin).join(", ");
  if (!text) return;
  saveRuneText(text);
  buildRunes();
  $("runes-dialog").close();
  toast(t("runes.saved"));
}

// ---------------------------------------------------------------- paramètres
// Mode verbeux : TAMGA écrit tout ce qu'il fait dans un journal, affiché en direct dans un terminal.
const settings = {
  get verbose() { return storageGet("tamga-verbose") === "1"; },
  set verbose(v) { storageSet("tamga-verbose", v ? "1" : "0"); },
  get terminal() { return storageGet("tamga-terminal") || "auto"; },
  set terminal(v) { storageSet("tamga-terminal", v); },
  get custom() { return storageGet("tamga-terminal-custom") || ""; },
  set custom(v) { storageSet("tamga-terminal-custom", v); },
  terminals: null,
  journal: ""
};

function settingsMsg(text, bad) {
  $("set-msg").textContent = text || "";
  $("set-msg").classList.toggle("bad", !!bad);
}

function errText(e) {
  const raw = String(e && e.message ? e.message : e);
  const key = raw.split(/[\s:]/)[0];
  return key.startsWith("err.") && hasT(key) ? t(key) : raw;
}

function fillTerminals() {
  const sel = $("set-terminal");
  sel.innerHTML = "";
  const tl = settings.terminals || { auto: null, available: [] };
  const add = (value, label) => { const o = document.createElement("option"); o.value = value; o.textContent = label; sel.appendChild(o); };
  add("auto", tl.auto ? t("settings.terminal_auto", { name: tl.auto.label }) : t("settings.terminal_auto_none"));
  tl.available.forEach(x => add(x.id, x.label));
  add("custom", t("settings.terminal_custom"));
  sel.value = Array.from(sel.options).some(o => o.value === settings.terminal) ? settings.terminal : "auto";
  $("set-custom-row").hidden = sel.value !== "custom";
}

async function openTerminal(quiet) {
  try {
    const name = await invoke("open_terminal", { terminal: settings.terminal, custom: settings.custom || null });
    if (!quiet) settingsMsg(t("settings.opened", { name }));
  } catch (e) {
    settingsMsg(errText(e), true);
    if (quiet) toast(errText(e));
  }
}

async function applyVerbose(openTerm) {
  try {
    const r = await invoke("set_verbose", { enabled: settings.verbose });
    settings.journal = r.journal;
    $("set-journal").textContent = t("settings.journal", { path: r.journal });
    if (settings.verbose && openTerm) await openTerminal(true);
  } catch (e) { console.error(e); }
}

async function openSettings() {
  settingsMsg("");
  if (!settings.terminals) {
    try { settings.terminals = await invoke("list_terminals"); } catch (e) { settings.terminals = { auto: null, available: [] }; }
  }
  fillTerminals();
  $("set-verbose").checked = settings.verbose;
  $("set-custom").value = settings.custom;
  if (!settings.journal) await applyVerbose(false);
  $("settings").showModal();
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
  startRuneWords();
  show("screen-busy");
}
function stopBusy() { clearInterval(state.busyTimer); clearInterval(runeState.timer); }

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
    state.targetChosen = false;
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
    b.addEventListener("click", () => selectTarget(tg.id, true));
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
  fillArch(m.arch);
  $("opt-bump").checked = false;
  $("opt-fixperms").checked = false;
  state.releaseBeforeBump = null;
  // Dossier de construction : tous les formats sont possibles, le format conseillé est choisi.
  if (a.is_folder) {
    $("pkg-format").textContent = t("folder.label");
    document.querySelectorAll("#targets .target").forEach(b => {
      b.disabled = false;
      const tg = TARGETS.find(x => x.id === b.dataset.id);
      b.querySelector(".target-desc").textContent = t(tg.desc);
    });
    if (a.suggested && !state.targetChosen) selectTarget(a.suggested);
  }
}

// Architecture (type de processeur) du paquet créé.
const ARCHES = ["x86_64", "aarch64", "i686", "armv7", "riscv64", "ppc64le", "s390x", "noarch"];
function fillArch(current) {
  const sel = $("edit-arch");
  sel.innerHTML = "";
  const auto = el("option", "", t("edit.arch_auto", { arch: current || "?" }));
  auto.value = "auto";
  sel.appendChild(auto);
  ARCHES.forEach(a => { const o = el("option", "", t("arch." + a)); o.value = a; sel.appendChild(o); });
  sel.value = "auto";
}

/** Révision + 1 : « 3 » → « 4 », « 1ubuntu2 » → « 2ubuntu2 », « » → « 1 » (comme le moteur). */
function bumpRelease(r) {
  r = String(r || "").trim();
  if (!r) return "1";
  const m = r.match(/^(\d+)(.*)$/);
  return m ? String(Number(m[1]) + 1) + m[2] : r + ".1";
}

function selectTarget(id, byUser) {
  state.target = id;
  if (byUser) state.targetChosen = true;
  document.querySelectorAll(".target").forEach(b => b.classList.toggle("selected", b.dataset.id === id));
  fillDistros();
  fillCompression();
}

function fillCompression(row, sel, target) {
  row = row || $("compression-row");
  sel = sel || $("compression");
  target = target || state.target;
  const opts = COMPRESSIONS[target];
  row.hidden = !opts;
  if (!opts) return;
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

function fillDistros(sel, target) {
  sel = sel || $("distro");
  target = target || state.target;
  const prev = sel.value;
  sel.innerHTML = "";
  const fam = state.sys && state.sys.family;
  const fmtOfFam = { debian: "deb", fedora: "rpm", suse: "rpm", arch: "arch" }[fam];
  const opts = [];
  if (GENERIC.includes(target)) {
    opts.push(["any", t("distro.any")]);
  } else {
    if (fmtOfFam === target) opts.push(["this_system", t("distro.this", { name: state.sys.name || state.sys.id })]);
    if (target === "deb") opts.push(["debian", t("distro.debian")]);
    if (target === "rpm") { opts.push(["fedora", t("distro.fedora")]); opts.push(["suse", t("distro.suse")]); }
    if (target === "arch") opts.push(["arch", t("distro.arch")]);
  }
  opts.forEach(([v, label]) => {
    const o = el("option", "", label);
    o.value = v;
    sel.appendChild(o);
  });
  if (opts.some(o => o[0] === prev)) sel.value = prev;
  else if (fam === "suse" && target === "rpm" && opts.some(o => o[0] === "this_system")) sel.value = "this_system";
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
    url: v("edit-url"),
    arch: v("edit-arch") === "auto" ? null : v("edit-arch")
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
      online: $("opt-online").checked,
      bundle: $("opt-bundle").checked
    });
    state.deps.forEach(d => { if (d.bundle) d.useBundle = true; });
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
    // Ligne « fournie » : le paquet d'origine de la bibliothèque, pas un nom pour la cible.
    const tgt = el("td", "mono", d.status === "bundled" ? (d.bundle ? d.bundle.package : "—") : depText(d.target));
    if (d.method && d.method !== "none") tgt.appendChild(el("span", "method", t("method." + d.method)));
    const st = el("td");
    const chip = el("span", "chip " + d.status, t("deps." + d.status));
    if (d.detail) chip.title = d.detail;
    st.appendChild(chip);
    // Bibliothèque fournie avec le paquet : d'où elle vient, et qui en a besoin.
    if (d.status === "bundled" && d.detail) st.appendChild(el("span", "dep-detail", d.detail));
    if (d.needed_by) src.appendChild(el("span", "dep-detail", t("deps.needed_by", { lib: d.needed_by })));
    const keep = el("td");
    const cb = document.createElement("input");
    cb.type = "checkbox";
    if (d.bundle) {
      // Case cochée = la bibliothèque est fournie avec le paquet.
      cb.checked = d.useBundle !== false;
      cb.title = t("deps.bundle_keep");
      cb.addEventListener("change", () => { d.useBundle = cb.checked; tr.classList.toggle("off", !cb.checked); updateMissing(); });
      tr.classList.toggle("off", d.useBundle === false);
    } else {
      cb.checked = !!d.keep;
      cb.disabled = !d.target || d.status === "bundled";
      cb.addEventListener("change", () => { d.keep = cb.checked; tr.classList.toggle("off", !cb.checked); updateMissing(); });
      tr.classList.toggle("off", !d.keep && d.status !== "bundled");
    }
    keep.appendChild(cb);
    tr.append(src, tgt, st, keep);
    tb.appendChild(tr);
  });
  $("deps-empty").hidden = rows.length > 0;
  updateMissing();
}

function hasMissing() {
  return state.deps.concat(state.extraDeps).some(d =>
    (d.status === "missing" && (d.keep || !d.target)) || (d.bundle && d.useBundle === false));
}

/** Bibliothèques à fournir avec le paquet (lignes « fournie » gardées). */
function bundledLibs() {
  return state.deps.filter(d => d.bundle && d.useBundle !== false).map(d => d.bundle);
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
        compression: compression(),
        bundle: $("opt-bundle").checked,
        bundled: bundledLibs(),
        fixperms: $("opt-fixperms").checked,
        as_folder: false
      }
    });
    state.installed = false;
    $("install-status").hidden = true;
    $("install-log-box").hidden = true;
    $("install-log").textContent = "";
    stopBusy();
    state.check = null;
    renderResult();
    show("screen-result");
    if (installAfter === true) await runInstall();
    if ($("opt-check").checked) runCheck();
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
  const folder = (r.warnings || []).includes("warn.folder_written");
  document.querySelector(".result-card h2").textContent = t(folder ? "result.folder_title" : "result.title");
  document.querySelector(".result-install-label").textContent = t(folder ? "result.folder_next" : "result.install");
  document.querySelector(".result-card [data-i18n='result.file']").textContent = t(folder ? "folder.label" : "result.file");
  $("btn-check").hidden = folder;
  renderCheck();
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
  const ub = $("result-bundled");
  ub.innerHTML = "";
  (r.bundled || []).forEach(b => ub.appendChild(el("li", "", b)));
  $("result-bundled-box").hidden = !(r.bundled || []).length;
  $("btn-install").hidden = folder || !canInstall(r.format) || state.installed;
}

// ---------------------------------------------------------------- vérification du paquet
async function runCheck() {
  const r = state.result;
  if (!r) return;
  state.check = "running";
  renderCheck();
  try {
    state.check = await invoke("check_package", { path: r.path, format: r.format });
  } catch (e) {
    state.check = { lines: [{ level: "error", text: String(e) }], errors: 1, warnings: 0 };
  }
  renderCheck();
}

function checkLineText(l) {
  return l.key ? t(l.key, l.params || {}) : l.text;
}

/** Résumé en clair d'un rapport de vérification. */
function checkSummary(c) {
  if (c.errors > 0) return { cls: "bad", text: t("check.summary_errors", { n: c.errors }) };
  if (c.warnings > 0) return { cls: "warn", text: t("check.summary_warnings", { n: c.warnings }) };
  return { cls: "ok", text: t("check.summary_ok") };
}

function renderCheck() {
  const box = $("check-box");
  const c = state.check;
  box.hidden = !c;
  if (!c) return;
  const sum = $("check-summary");
  const ul = $("check-lines");
  ul.innerHTML = "";
  if (c === "running") {
    sum.textContent = t("check.running");
    sum.className = "check-summary";
    $("check-note").textContent = "";
    return;
  }
  const s = checkSummary(c);
  sum.textContent = s.text;
  sum.className = "check-summary " + s.cls;
  $("check-note").textContent = (c.tool && (c.errors || c.warnings)) ? t("check.note_converted") : "";
  (c.lines || []).forEach(l => ul.appendChild(el("li", l.level, checkLineText(l))));
  $("check-details").open = c.errors > 0;
}

// ---------------------------------------------------------------- dossier de construction
async function makeFolder() {
  let dir;
  try {
    dir = await TAURI.dialog.open({ directory: true, multiple: false, title: t("folder.choose"), defaultPath: dirName(state.analysis.path) });
  } catch (e) { showError(e); return; }
  if (!dir) return;
  const deps = state.deps.concat(state.extraDeps).filter(d => d.keep && d.target).map(d => d.target);
  busy("busy.folder", state.analysis.file_name);
  try {
    state.result = await invoke("convert_package", {
      options: {
        format: state.target,
        distro: $("distro").value,
        meta: metaOverride(),
        deps,
        include_scripts: $("opt-scripts").checked,
        usrmerge: $("opt-usrmerge").checked,
        online: $("opt-online").checked,
        output: typeof dir === "string" ? dir : dir.path,
        compression: compression(),
        bundle: $("opt-bundle").checked,
        bundled: bundledLibs(),
        fixperms: $("opt-fixperms").checked,
        as_folder: true
      }
    });
    state.installed = false;
    state.check = null;
    $("install-status").hidden = true;
    $("install-log-box").hidden = true;
    stopBusy();
    renderResult();
    show("screen-result");
  } catch (e) { showError(e); }
}

async function openBuildFolder() {
  try {
    const dir = await TAURI.dialog.open({ directory: true, multiple: false, title: t("home.open_folder") });
    if (dir) openFile(typeof dir === "string" ? dir : dir.path);
  } catch (e) { showError(e); }
}

// ---------------------------------------------------------------- plusieurs paquets
const batch = { files: [], running: false, current: null, outDir: "" };

/** Fichiers choisis ou déposés : un seul → écran du paquet, plusieurs → conversion par lot. */
function openPaths(paths) {
  if (!paths || !paths.length) return;
  if (paths.length === 1 && !batch.files.length) { openFile(paths[0]); return; }
  addBatchFiles(paths);
  if (!batch.outDir) batch.outDir = dirName(paths[0]);
  fillBatchFormats();
  renderBatch();
  show("screen-batch");
}

function addBatchFiles(paths) {
  paths.forEach(p => {
    if (!batch.files.some(f => f.path === p)) batch.files.push({ path: p, status: "waiting", detail: "", result: null });
  });
}

function renderBatch() {
  const ul = $("batch-list");
  ul.innerHTML = "";
  batch.files.forEach((f, i) => {
    const li = el("li");
    const name = el("div", "b-name", baseName(f.path));
    name.appendChild(el("span", "b-detail", f.detail || dirName(f.path)));
    const chipCls = { waiting: "skipped", running: "estimated", done: "installed", missing: "estimated", failed: "missing" }[f.status];
    const chip = el("span", "chip " + chipCls, t("batch." + f.status));
    const rm = el("button", "b-remove", "×");
    rm.type = "button";
    rm.title = t("batch.remove");
    rm.disabled = batch.running;
    rm.addEventListener("click", () => { batch.files.splice(i, 1); renderBatch(); if (!batch.files.length) goHome(); });
    li.append(name, chip, rm);
    ul.appendChild(li);
  });
  $("batch-count").textContent = "(" + batch.files.length + ")";
  $("batch-out").value = batch.outDir;
  $("batch-go").disabled = batch.running || !batch.files.length;
  $("batch-add").disabled = batch.running;
  const done = batch.files.filter(f => f.status === "done" || f.status === "missing").length;
  const fail = batch.files.filter(f => f.status === "failed").length;
  $("batch-summary").textContent = (done || fail) ? t("batch.summary", { ok: done, fail }) : "";
  $("batch-open-out").hidden = !(done && !batch.running);
}

function fillBatchFormats() {
  const sel = $("batch-format");
  if (sel.options.length) return;
  TARGETS.forEach(tg => { const o = el("option", "", tg.ext + " — " + t(tg.desc)); o.value = tg.id; sel.appendChild(o); });
  const fam = state.sys && state.sys.family;
  sel.value = { debian: "deb", fedora: "rpm", suse: "rpm", arch: "arch" }[fam] || "deb";
  fillBatchTarget();
}

function fillBatchTarget() {
  const tgt = $("batch-format").value;
  fillDistros($("batch-distro"), tgt);
  fillCompression($("batch-compression-row"), $("batch-compression"), tgt);
}

async function runBatch() {
  if (batch.running || !batch.files.length) return;
  batch.running = true;
  const format = $("batch-format").value;
  const options = {
    format,
    distro: $("batch-distro").value,
    meta: {},
    deps: null,
    include_scripts: false,
    usrmerge: $("batch-opt-usrmerge").checked,
    online: $("batch-opt-online").checked,
    output: batch.outDir,
    compression: COMPRESSIONS[format] ? $("batch-compression").value : null,
    bundle: $("batch-opt-bundle").checked,
    fixperms: $("batch-opt-fixperms").checked,
    as_folder: false
  };
  batch.files.forEach(f => { if (f.status !== "done") { f.status = "waiting"; f.detail = ""; } });
  renderBatch();
  for (const f of batch.files) {
    if (f.status === "done") continue;
    f.status = "running";
    batch.current = f;
    renderBatch();
    try {
      const r = await invoke("convert_file", { path: f.path, options, bump: $("batch-opt-bump").checked });
      if (r.error) {
        f.status = "failed";
        f.detail = errText(r.error);
      } else {
        f.result = r.result;
        f.status = r.missing ? "missing" : "done";
        f.detail = baseName(r.result.path) + " · " + fmtSize(r.result.size) + (r.result.bundled && r.result.bundled.length ? " · " + t("batch.bundled", { n: r.result.bundled.length }) : "");
        if ($("batch-opt-check").checked) {
          renderBatch();
          try {
            const c = await invoke("check_package", { path: r.result.path, format });
            f.detail += " · " + checkSummary(c).text;
          } catch (e) { /* vérification impossible : on continue */ }
        }
      }
    } catch (e) {
      f.status = "failed";
      f.detail = errText(e);
    }
    renderBatch();
  }
  batch.running = false;
  batch.current = null;
  renderBatch();
}

// ---------------------------------------------------------------- démarrage
function goHome() {
  stopBusy();
  if (!batch.running) { batch.files = []; batch.outDir = ""; }
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
  $("btn-check").addEventListener("click", runCheck);
  $("btn-make-folder").addEventListener("click", makeFolder);
  $("btn-open-build-folder").addEventListener("click", openBuildFolder);
  $("opt-bump").addEventListener("change", () => {
    // La révision augmentée se voit (et se corrige) dans « Modifier les informations ».
    if ($("opt-bump").checked) {
      state.releaseBeforeBump = $("edit-release").value;
      $("edit-release").value = bumpRelease(state.releaseBeforeBump);
    } else if (state.releaseBeforeBump !== null) {
      $("edit-release").value = state.releaseBeforeBump;
    }
  });
  $("batch-format").addEventListener("change", fillBatchTarget);
  $("batch-go").addEventListener("click", runBatch);
  $("batch-open-out").addEventListener("click", () => {
    const f = batch.files.find(x => x.result);
    if (f && TAURI && TAURI.opener) TAURI.opener.revealItemInDir(f.result.path).catch(err => console.error(err));
  });
  $("batch-add").addEventListener("click", async () => {
    try {
      const picked = await TAURI.dialog.open({ multiple: true, directory: false });
      const list = (Array.isArray(picked) ? picked : (picked ? [picked] : [])).map(p => typeof p === "string" ? p : p.path);
      if (list.length) { addBatchFiles(list); renderBatch(); }
    } catch (e) { console.error(e); }
  });
  $("batch-out-btn").addEventListener("click", async () => {
    try {
      const dir = await TAURI.dialog.open({ directory: true, multiple: false, defaultPath: batch.outDir || undefined });
      if (dir) { batch.outDir = typeof dir === "string" ? dir : dir.path; renderBatch(); }
    } catch (e) { console.error(e); }
  });
  const syncBatchBundle = () => { $("batch-opt-bundle").disabled = !$("batch-opt-online").checked; };
  $("batch-opt-online").addEventListener("change", syncBatchBundle);
  syncBatchBundle();
  // Fournir les bibliothèques manquantes demande la recherche en ligne.
  const syncBundle = () => { $("opt-bundle").disabled = !$("opt-online").checked; };
  $("opt-online").addEventListener("change", syncBundle);
  syncBundle();
  $("btn-settings").addEventListener("click", openSettings);
  $("btn-settings-close").addEventListener("click", () => $("settings").close());
  $("set-verbose").addEventListener("change", async () => {
    settings.verbose = $("set-verbose").checked;
    settingsMsg("");
    await applyVerbose(settings.verbose);
  });
  $("set-terminal").addEventListener("change", () => {
    settings.terminal = $("set-terminal").value;
    $("set-custom-row").hidden = settings.terminal !== "custom";
    settingsMsg("");
  });
  $("set-custom").addEventListener("input", () => { settings.custom = $("set-custom").value.trim(); });
  $("btn-open-terminal").addEventListener("click", () => openTerminal(false));
  $("btn-runes").addEventListener("click", openRunes);
  $("runes-input").addEventListener("input", renderRunePreview);
  $("runes-input").addEventListener("keydown", e => { if (e.key === "Enter" && !$("btn-runes-save").disabled) { e.preventDefault(); saveRunes(); } });
  $("btn-runes-save").addEventListener("click", saveRunes);
  $("btn-runes-cancel").addEventListener("click", () => $("runes-dialog").close());
  $("btn-runes-reset").addEventListener("click", () => { $("runes-input").value = RUNE_DEFAULT; renderRunePreview(); });
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
    const stage = document.querySelector(".rune-stage");
    if (stage) stage.title = runeTitle();
    if ($("runes-dialog").open) renderRunePreview();
    if ($("settings").open) fillTerminals();
    if (settings.journal) $("set-journal").textContent = t("settings.journal", { path: settings.journal });
    if (state.analysis && !$("screen-package").hidden) {
      const keepTarget = state.target;
      renderPackage();
      selectTarget(keepTarget);
    }
    if (!$("screen-deps").hidden) renderDeps();
    if ($("batch-format").options.length) {
      const keep = [$("batch-format").value, $("batch-distro").value];
      $("batch-format").innerHTML = "";
      fillBatchFormats();
      $("batch-format").value = keep[0];
      fillBatchTarget();
      $("batch-distro").value = keep[1];
      renderBatch();
    }
    if (!$("screen-result").hidden && state.result) renderResult();
  });
}

async function start() {
  await initI18n();
  buildRunes();
  // Mode verbeux gardé de la dernière fois : on rouvre le terminal du journal.
  if (TAURI && settings.verbose) applyVerbose(true);
  bind();
  initDropzone(openPaths);
  await loadSystem();
  try { state.installable = await invoke("installable_formats"); } catch (e) { state.installable = []; }
  if (TAURI && TAURI.event) TAURI.event.listen("install-log", ev => appendInstallLog(String(ev.payload))).catch(() => {});
  // Recherche des bibliothèques manquantes : ce qui se passe, sous le titre d'attente.
  if (TAURI && TAURI.event) TAURI.event.listen("bundle-progress", ev => {
    const p = ev.payload || {};
    if (!$("screen-busy").hidden) $("busy-file").textContent = t(p.key) + " " + (p.detail || "");
    if (batch.running && batch.current) { batch.current.detail = t(p.key) + " " + (p.detail || ""); renderBatch(); }
  }).catch(() => {});
  if (storageGet("tamga-legal") === LEGAL_VERSION) show("screen-home");
  else show("screen-legal");
}

start();
