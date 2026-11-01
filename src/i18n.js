// TAMGA — moteur de traduction / translation engine
const LANGS = [
  { code: "tr", name: "Türkçe",    flag: "🇹🇷" },
  { code: "fr", name: "Français",  flag: "🇫🇷" },
  { code: "en", name: "English",   flag: "🇬🇧" },
  { code: "ar", name: "العربية",   flag: "🇸🇦", rtl: true },
  { code: "it", name: "Italiano",  flag: "🇮🇹" },
  { code: "pt", name: "Português", flag: "🇵🇹" },
  { code: "es", name: "Español",   flag: "🇪🇸" }
];

let currentLang = "fr";
let dict = {};
let fallback = {};
const langListeners = [];

function storageGet(k) { try { return localStorage.getItem(k); } catch (e) { return null; } }
function storageSet(k, v) { try { localStorage.setItem(k, v); } catch (e) { /* ignoré */ } }

function detectLang() {
  const saved = storageGet("tamga-lang");
  if (saved && LANGS.some(l => l.code === saved)) return saved;
  const sys = (navigator.language || "en").slice(0, 2);
  return LANGS.some(l => l.code === sys) ? sys : "en";
}

async function fetchJson(code) {
  const res = await fetch("locales/" + code + ".json");
  return res.json();
}

/** Traduit une clé ; `vars` remplace les {nom} dans le texte. */
function t(key, vars) {
  let s = dict[key] || fallback[key] || key;
  if (vars) for (const k in vars) s = s.split("{" + k + "}").join(vars[k]);
  return s;
}

/** Vrai si la clé existe dans les traductions. */
function hasT(key) { return key in dict || key in fallback; }

async function loadLang(code) {
  try {
    if (!Object.keys(fallback).length) fallback = await fetchJson("en");
    dict = await fetchJson(code);
    currentLang = code;
    storageSet("tamga-lang", code);
    applyLang();
  } catch (err) {
    console.error("TAMGA : impossible de charger la langue " + code, err);
  }
}

function applyLang() {
  document.querySelectorAll("[data-i18n]").forEach(el => {
    const key = el.getAttribute("data-i18n");
    if (dict[key] || fallback[key]) el.textContent = t(key);
  });
  document.querySelectorAll("[data-i18n-placeholder]").forEach(el => {
    el.placeholder = t(el.getAttribute("data-i18n-placeholder"));
  });
  document.querySelectorAll("[data-i18n-title]").forEach(el => {
    el.title = t(el.getAttribute("data-i18n-title"));
  });
  const lang = LANGS.find(l => l.code === currentLang);
  document.documentElement.lang = currentLang;
  document.documentElement.dir = (lang && lang.rtl) ? "rtl" : "ltr";
  document.querySelectorAll(".lang-btn").forEach(b => {
    b.classList.toggle("active", b.dataset.code === currentLang);
  });
  langListeners.forEach(fn => { try { fn(); } catch (e) { console.error(e); } });
}

/** Appelé à chaque changement de langue (pour les textes générés en JS). */
function onLangChange(fn) { langListeners.push(fn); }

function buildSelector() {
  const wrap = document.getElementById("lang-selector");
  if (!wrap) return;
  wrap.innerHTML = "";
  LANGS.forEach(l => {
    const b = document.createElement("button");
    b.className = "lang-btn";
    b.dataset.code = l.code;
    b.textContent = l.flag + " " + l.name;
    b.addEventListener("click", () => loadLang(l.code));
    wrap.appendChild(b);
  });
}

function initI18n() {
  currentLang = detectLang();
  buildSelector();
  return loadLang(currentLang);
}
