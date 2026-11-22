// TAMGA — écriture en runes turciques (alphabet de l'Orkhon)
// Old Turkic (Orkhon) transliteration of words typed in Latin letters.
//
// Règles (simplifiées, d'après les inscriptions de l'Orkhon) :
// - les consonnes b, d, g, k, l, n, r, s, ş, t, y ont deux formes : « dure » (avec a, ı, o, u)
//   et « douce » (avec e, i, ö, ü) ; on prend la voyelle de la syllabe ;
// - a et e ne s'écrivent qu'au début et à la fin du mot ;
// - ı/i → 𐰃, o/u → 𐰆, ö/ü → 𐰇 ;
// - k près de ı, o/u, ö/ü prend la forme 𐰶, 𐰸, 𐰜 qui contient déjà la voyelle ;
// - ligatures : ng → 𐰭, nç → 𐰨, nt/nd → 𐰦 (en fin de syllabe) ;
// - sons absents du vieux turc : f → p, v/w → b, h/q → k, c → ç, j → y, x → ks.

const RUNE_MAX = 40;        // nombre maximal de runes (hors séparateurs)
const RUNE_MAX_WORDS = 8;
const RUNE_DEFAULT = "tamga, türük";

const RUNE_BACK = new Set(["a", "ı", "o", "u"]);
const RUNE_VOWELS = new Set(["a", "ı", "o", "u", "e", "i", "ö", "ü"]);

// [forme dure, forme douce] ou forme unique.
const RUNE_LETTERS = {
  b: ["\u{10C09}", "\u{10C0B}"],
  d: ["\u{10C11}", "\u{10C13}"],
  g: ["\u{10C0D}", "\u{10C0F}"],
  y: ["\u{10C16}", "\u{10C18}"],
  l: ["\u{10C1E}", "\u{10C20}"],
  n: ["\u{10C23}", "\u{10C24}"],
  r: ["\u{10C3A}", "\u{10C3C}"],
  s: ["\u{10C3D}", "\u{10C3E}"],
  ş: ["\u{10C3F}", "\u{10C41}"],
  t: ["\u{10C43}", "\u{10C45}"],
  m: "\u{10C22}",
  z: "\u{10C14}",
  p: "\u{10C2F}",
  ç: "\u{10C32}",
  ng: "\u{10C2D}",
  nç: "\u{10C28}",
  nt: "\u{10C26}"
};
const RUNE_A = "\u{10C00}", RUNE_I = "\u{10C03}", RUNE_O = "\u{10C06}", RUNE_OE = "\u{10C07}";
const RUNE_K = { a: "\u{10C34}", ı: "\u{10C36}", o: "\u{10C38}", u: "\u{10C38}", e: "\u{10C1A}", i: "\u{10C1A}", ö: "\u{10C1C}", ü: "\u{10C1C}" };

function isRune(ch) {
  const c = ch.codePointAt(0);
  return c >= 0x10C00 && c <= 0x10C48;
}

/** Minuscules et lettres ramenées à l'alphabet turc. */
function runeNormalize(word) {
  let s = word.replace(/İ/g, "i").toLowerCase();
  const map = {
    "â": "a", "à": "a", "á": "a", "ã": "a", "å": "a", "ä": "e", "æ": "e",
    "é": "e", "è": "e", "ê": "e", "ë": "e", "ə": "e",
    "î": "i", "ï": "i", "í": "i", "ì": "i",
    "ô": "o", "ó": "o", "ò": "o", "õ": "o", "œ": "ö", "ø": "ö",
    "û": "u", "ù": "u", "ú": "u",
    "ÿ": "y", "ý": "y", "ñ": "ng", "ß": "ss", "ğ": "g",
    "c": "ç", "f": "p", "v": "b", "w": "b", "h": "k", "q": "k", "j": "y", "x": "ks"
  };
  // Graphies françaises / anglaises courantes avant le remplacement lettre par lettre.
  s = s.replace(/ou/g, "u").replace(/sh|sch|ch/g, "ş").replace(/ph/g, "p");
  let out = "";
  for (const ch of s) {
    if (isRune(ch)) { out += ch; continue; }
    const m = map[ch];
    if (m !== undefined) { out += m; continue; }
    if (/[a-zçöşüı]/.test(ch)) out += ch;
    // le reste (chiffres, tirets, apostrophes…) est ignoré
  }
  return out;
}

/** Découpe un mot normalisé en sons : voyelles, consonnes, ligatures. */
function runeTokens(s) {
  const chars = Array.from(s);
  const toks = [];
  const isV = c => RUNE_VOWELS.has(c);
  for (let i = 0; i < chars.length; i++) {
    const c = chars[i], next = chars[i + 1], after = chars[i + 2];
    if (isRune(c)) { toks.push({ rune: c }); continue; }
    if (isV(c)) { toks.push({ v: c }); continue; }
    // Consonne doublée : une seule rune.
    const prev = toks[toks.length - 1];
    if (prev && prev.c === c) continue;
    // Ligatures en fin de syllabe (pas suivies d'une voyelle).
    if (c === "n" && next === "ç") { toks.push({ c: "nç" }); i++; continue; }
    if (c === "n" && next === "g" && !(after && isV(after))) { toks.push({ c: "ng" }); i++; continue; }
    if (c === "n" && (next === "t" || next === "d") && !(after && isV(after))) { toks.push({ c: "nt" }); i++; continue; }
    toks.push({ c });
  }
  return toks;
}

/** Écrit un mot en runes. */
function wordToRunes(word) {
  const toks = runeTokens(runeNormalize(word));
  const n = toks.length;
  const vowelAt = i => (i >= 0 && i < n && toks[i].v) ? toks[i].v : null;
  // Voyelle de la syllabe d'une consonne : la suivante si elle la suit directement,
  // sinon la précédente, sinon la prochaine du mot.
  function syllableVowel(i) {
    if (vowelAt(i + 1)) return vowelAt(i + 1);
    for (let j = i - 1; j >= 0; j--) if (toks[j].v) return toks[j].v;
    for (let j = i + 1; j < n; j++) if (toks[j].v) return toks[j].v;
    return "a";
  }
  // Formes des k d'abord (elles peuvent contenir une voyelle voisine).
  const absorbed = new Set();
  toks.forEach((tk, i) => {
    if (tk.c !== "k") return;
    const v = syllableVowel(i);
    tk.k = RUNE_K[v] || RUNE_K.a;
    if (!"ıouöü".includes(v)) return;
    // La voyelle voisine est sous-entendue par 𐰶 / 𐰸 / 𐰜 (sauf en début ou fin de mot).
    for (const j of [i + 1, i - 1]) {
      if (j > 0 && j < n - 1 && toks[j].v === v) { absorbed.add(j); break; }
    }
  });
  let out = "";
  toks.forEach((tk, i) => {
    if (tk.rune) { out += tk.rune; return; }
    if (tk.v) {
      if (absorbed.has(i)) return;
      const v = tk.v;
      if (v === "a" || v === "e") { if (i === 0 || i === n - 1) out += RUNE_A; return; }
      if (v === "ı" || v === "i") out += RUNE_I;
      else if (v === "o" || v === "u") out += RUNE_O;
      else out += RUNE_OE;
      return;
    }
    if (tk.k) { out += tk.k; return; }
    const r = RUNE_LETTERS[tk.c];
    if (!r) return;
    if (Array.isArray(r)) out += r[RUNE_BACK.has(syllableVowel(i)) ? 0 : 1];
    else out += r;
  });
  return out;
}

/** Liste des mots saisis (séparés par des virgules, points-virgules ou retours à la ligne). */
function parseRuneWords(text) {
  return String(text || "")
    .split(/[,;\n]+/)
    .map(w => w.trim().replace(/\s+/g, " "))
    .filter(Boolean)
    .map(latin => ({
      latin,
      // Plusieurs mots séparés par des espaces : « : » entre eux, comme dans les inscriptions.
      runes: latin.split(" ").map(wordToRunes).filter(Boolean).join(":")
    }))
    .filter(w => w.runes);
}

/** Nombre de runes (sans les séparateurs). */
function runeCount(words) {
  return words.reduce((n, w) => n + Array.from(w.runes).filter(isRune).length, 0);
}

function loadRuneText() {
  try {
    const s = localStorage.getItem("tamga-runes");
    if (s && parseRuneWords(s).length) return s;
  } catch (e) { /* stockage indisponible */ }
  return RUNE_DEFAULT;
}

function saveRuneText(text) {
  try {
    if (text === RUNE_DEFAULT) localStorage.removeItem("tamga-runes");
    else localStorage.setItem("tamga-runes", text);
  } catch (e) { /* stockage indisponible */ }
}

if (typeof module !== "undefined") module.exports = { wordToRunes, parseRuneWords, runeCount, runeNormalize };
