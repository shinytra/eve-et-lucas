"""Regénère les icônes de l'appli à partir de `original.png`.

    python3 icons/source/generer.py          # depuis apps/desktop/src-tauri
    npx tauri icon /tmp/icone.png -o /tmp/i  # puis copier 32x32, 128x128,
                                             # 128x128@2x, icon.png, icon.ico

- PC et ancien Android : carré serré sur les deux visages (lisible en 16 px).
- Android 8+ : icône adaptative, l'image en fond sur 108 dp, dont le
  lanceur ne montre que les 72 dp du centre (rond ou carré arrondi).
  Le projet Android est régénéré par la CI : elle copie `icons/android`
  dans `gen/android/app/src/main/res` après `tauri android init`.
"""

import os

from PIL import Image, ImageDraw

ICI = os.path.dirname(os.path.abspath(__file__))
ICONES = os.path.dirname(ICI)
L = Image.LANCZOS

src = Image.open(os.path.join(ICI, "original.png")).convert("RGB")
serre = src.crop((90, 95, 620, 625)).resize((1024, 1024), L)
serre.save("/tmp/icone.png")
fond = src.crop((0, 0, 736, 736))

for d, k in {"mdpi": 1, "hdpi": 1.5, "xhdpi": 2, "xxhdpi": 3, "xxxhdpi": 4}.items():
    o = os.path.join(ICONES, "android", f"mipmap-{d}")
    os.makedirs(o, exist_ok=True)
    a, b = int(48 * k), int(108 * k)
    serre.resize((a, a), L).save(os.path.join(o, "ic_launcher.png"), optimize=True)
    grand = serre.resize((a * 4, a * 4), L)
    m = Image.new("L", grand.size, 0)
    ImageDraw.Draw(m).ellipse((0, 0, a * 4 - 1, a * 4 - 1), fill=255)
    rond = Image.new("RGBA", grand.size, (0, 0, 0, 0))
    rond.paste(grand, (0, 0), m)
    rond.resize((a, a), L).save(os.path.join(o, "ic_launcher_round.png"), optimize=True)
    fond.resize((b, b), L).save(os.path.join(o, "ic_launcher_background.png"), optimize=True)
    Image.new("RGBA", (b, b), (0, 0, 0, 0)).save(
        os.path.join(o, "ic_launcher_foreground.png"), optimize=True
    )
