"""Génère src/locales/*.json. Les traductions existantes de l'utilisateur sont conservées."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from locales_fr_en import FR, EN
from locales_tr_ar import TR, AR
from locales_it_pt_es import IT, PT, ES
from locales_extra import EXTRA
ALL = {"fr": FR, "en": EN, "tr": TR, "ar": AR, "it": IT, "pt": PT, "es": ES}
# Textes ajoutés ensuite (Slackware, tar, .slp, Solaris, installation) : ils complètent
# ou remplacent les textes de base.
for code, extra in EXTRA.items():
    ALL[code] = {**ALL[code], **extra}
FR = ALL["fr"]
EN = ALL["en"]
keys = list(FR.keys())
out_dir = sys.argv[1]
existing_dir = sys.argv[2] if len(sys.argv) > 2 else None
ok = True
for code, d in ALL.items():
    missing = [k for k in keys if k not in d]
    extra = [k for k in d if k not in FR]
    if missing or extra:
        ok = False
        print(code, "missing:", missing, "extra:", extra)
    merged = {k: d.get(k, EN.get(k)) for k in keys}
    if existing_dir:
        p = os.path.join(existing_dir, code + ".json")
        if os.path.exists(p):
            old = json.load(open(p, encoding="utf-8"))
            for k, v in old.items():
                merged[k] = v  # garder la traduction de l'utilisateur
    with open(os.path.join(out_dir, code + ".json"), "w", encoding="utf-8") as f:
        json.dump(merged, f, ensure_ascii=False, indent=2)
        f.write("\n")
print("OK" if ok else "INCOMPLET", len(keys), "clés")
