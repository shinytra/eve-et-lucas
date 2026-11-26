// Apparence : applique le thème (variables CSS) et gère l'écran des réglages.
// Les préréglages, les bornes et la validation viennent du cœur Rust
// (couple-core/src/appearance.rs) via les commandes Tauri.
"use strict";

(() => {
  const root = document.documentElement;
  const CACHE_KEY = "apparence";

  // Familles de polices : uniquement des polices du système, l'appli marche
  // hors ligne. Le navigateur prend la première disponible.
  const FONTS = {
    system: { label: "Système", stack: 'system-ui, -apple-system, "Segoe UI", Roboto, Ubuntu, Cantarell, "Noto Sans", sans-serif' },
    rounded: { label: "Arrondie", stack: 'ui-rounded, "SF Pro Rounded", Nunito, Quicksand, "Varela Round", Comfortaa, "M PLUS Rounded 1c", system-ui, sans-serif' },
    serif: { label: "Classique", stack: '"Iowan Old Style", "Palatino Linotype", Palatino, "Book Antiqua", Georgia, "Noto Serif", "DejaVu Serif", serif' },
    elegant: { label: "Élégante", stack: 'Didot, "Bodoni MT", "Playfair Display", "Cormorant Garamond", Garamond, "Noto Serif Display", "Times New Roman", serif' },
    mono: { label: "Machine", stack: 'ui-monospace, "Cascadia Code", "JetBrains Mono", "SF Mono", Consolas, "Roboto Mono", "DejaVu Sans Mono", monospace' },
    handwritten: { label: "Manuscrite", stack: '"Segoe Print", "Bradley Hand", Caveat, "Patrick Hand", "Comic Neue", "Comic Sans MS", cursive' },
  };
  const GAPS = { compact: 10, normal: 14, spacious: 20 };

  // ---------- Couleurs ----------

  function rgb(hex) {
    const n = parseInt(hex.slice(1), 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }

  function rgba(hex, alpha) {
    const [r, g, b] = rgb(hex);
    return `rgba(${r}, ${g}, ${b}, ${Math.max(0, Math.min(1, alpha)).toFixed(3)})`;
  }

  // Luminance relative (WCAG), de 0 (noir) à 1 (blanc).
  function luminance(hex) {
    const [r, g, b] = rgb(hex).map((c) => {
      const s = c / 255;
      return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  }

  const isHex = (s) => typeof s === "string" && /^#[0-9a-fA-F]{6}$/.test(s);

  // ---------- Application du thème ----------

  // Variables CSS d'un thème, pour la page entière ou pour un aperçu.
  function themeVars(a) {
    const op = a.glass_opacity / 100;
    const dark = luminance(a.text) > 0.5;
    // Les boutons sont un dégradé accent → accent 2 : texte foncé si l'une
    // des deux couleurs est claire.
    const accentText = Math.max(luminance(a.accent), luminance(a.accent2)) > 0.45 ? "#14161f" : "#ffffff";
    const shadowAlpha = (a.shadow / 100) * (dark ? 0.55 : 0.3);
    return {
      "--accent": a.accent,
      "--accent-2": a.accent2,
      "--accent-text": accentText,
      "--text": a.text,
      "--glass-bg": rgba(a.glass_tint, op),
      "--glass-strong": rgba(a.glass_tint, Math.min(1, op + 0.3)),
      "--glass-solid": rgba(a.glass_tint, Math.max(0.94, op)),
      "--field-bg": rgba(a.glass_tint, Math.min(1, op * 0.6 + 0.12)),
      "--blur": `${a.blur}px`,
      "--sat": `${a.saturation}%`,
      "--border-width": `${a.border_width}px`,
      "--border-color": rgba("#ffffff", (a.border_opacity / 100) * (dark ? 0.6 : 1)),
      "--radius": `${a.radius}px`,
      "--shadow": a.shadow ? `0 ${4 + a.shadow / 10}px ${16 + a.shadow / 3}px rgba(0, 0, 0, ${shadowAlpha.toFixed(3)})` : "none",
      "--shine": a.shine
        ? `linear-gradient(135deg, rgba(255, 255, 255, ${dark ? 0.12 : 0.45}), rgba(255, 255, 255, 0) 45%)`
        : "none",
      "--gap": `${GAPS[a.density] ?? GAPS.normal}px`,
      "--font": (FONTS[a.font] ?? FONTS.system).stack,
      "--wall-0": a.wallpaper_colors[0],
      "--wall-1": a.wallpaper_colors[1],
      "--wall-2": a.wallpaper_colors[2],
      "--wall-blur": `${a.wallpaper_blur}px`,
      "--wall-dim": String(a.wallpaper_dim / 100),
      "--grain": String((a.grain / 100) * 0.6),
    };
  }

  let wallpaperImage = null; // data URL de l'image de fond, si elle existe
  // Tant que l'image n'est pas chargée, un fond « image » reste sur la couleur
  // unie au lieu de montrer brièvement les taches de couleur.
  let wallpaperLoaded = false;

  function apply(a) {
    for (const [name, value] of Object.entries(themeVars(a))) root.style.setProperty(name, value);
    root.style.fontSize = `${a.font_scale}%`;
    root.toggleAttribute("data-sombre", luminance(a.text) > 0.5);
    root.toggleAttribute("data-anime", Boolean(a.animate));
    root.dataset.fond = a.wallpaper;
    if (wallpaperImage) {
      root.style.setProperty("--wall-image", `url("${wallpaperImage}")`);
      root.setAttribute("data-image", "");
    } else {
      root.style.removeProperty("--wall-image");
      root.toggleAttribute("data-image", !wallpaperLoaded && a.wallpaper === "image");
    }
  }

  // Le cache local n'est pas passé par le cœur : on vérifie chaque valeur.
  function validCache(a) {
    if (!a || typeof a !== "object") return false;
    const colors = ["accent", "accent2", "text", "glass_tint"].map((k) => a[k]);
    const numbers = ["glass_opacity", "blur", "saturation", "border_width", "border_opacity", "radius", "shadow", "grain", "font_scale", "wallpaper_blur", "wallpaper_dim"].map((k) => a[k]);
    return (
      colors.every(isHex) &&
      Array.isArray(a.wallpaper_colors) &&
      a.wallpaper_colors.length === 3 &&
      a.wallpaper_colors.every(isHex) &&
      numbers.every((n) => Number.isFinite(n) && n >= 0 && n <= 1000) &&
      ["mesh", "linear", "solid", "image"].includes(a.wallpaper)
    );
  }

  // Le dernier thème est gardé sur l'appareil pour s'afficher tout de suite au
  // démarrage ; la version enregistrée par le cœur fait foi juste après.
  function cache(a) {
    try {
      localStorage.setItem(CACHE_KEY, JSON.stringify(a));
    } catch {}
  }
  try {
    const cached = JSON.parse(localStorage.getItem(CACHE_KEY) ?? "null");
    if (validCache(cached)) apply(cached);
  } catch {}

  // ---------- Écran des réglages ----------

  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) return; // main.js affiche déjà le message d'erreur.

  const $ = (id) => document.getElementById(id);

  // Les écritures passent l'une après l'autre, dans l'ordre des clics.
  let writes = Promise.resolve();
  function write(command, args) {
    const result = writes.then(() => invoke(command, args));
    writes = result.catch(() => {});
    return result;
  }

  // Une action qui remplace tout le thème annule d'abord l'enregistrement
  // différé encore en attente, sinon il écraserait le nouveau thème.
  function replacing(command, args) {
    clearTimeout(saveTimer);
    saveTimer = null;
    return write(command, args);
  }
  let view = null; // { current, saved, presets, limits }
  let current = null;
  const controls = []; // fonctions qui remettent chaque contrôle à jour

  function status(message, isError = false) {
    const box = $("settings-status");
    box.textContent = message ?? "";
    box.classList.toggle("erreur", isError);
  }

  // Enregistrement différé : on applique tout de suite à l'écran, et on
  // n'écrit sur le disque qu'une fois le curseur relâché un instant.
  let saveTimer = null;
  function changed() {
    apply(current);
    cache(current);
    renderPresetState();
    clearTimeout(saveTimer);
    status("…");
    saveTimer = setTimeout(async () => {
      saveTimer = null;
      try {
        await write("set_appearance", { appearance: current });
        status("Enregistré");
      } catch (e) {
        status(String(e), true);
      }
    }, 300);
  }

  // Remplace tout le thème (préréglage, thème perso, import).
  function replace(appearance, message) {
    clearTimeout(saveTimer);
    saveTimer = null;
    current = appearance;
    apply(current);
    cache(current);
    refreshControls();
    status(message);
  }

  function setValue(key, value) {
    if (key.startsWith("wallpaper_colors.")) {
      current.wallpaper_colors[Number(key.split(".")[1])] = value;
    } else {
      current[key] = value;
    }
    changed();
  }

  function getValue(key) {
    if (key.startsWith("wallpaper_colors.")) return current.wallpaper_colors[Number(key.split(".")[1])];
    return current[key];
  }

  function el(tag, props = {}, ...children) {
    const node = document.createElement(tag);
    for (const [k, v] of Object.entries(props)) {
      if (k === "class") node.className = v;
      else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
      else if (k in node) node[k] = v;
      else node.setAttribute(k, v);
    }
    node.append(...children);
    return node;
  }

  // --- Types de contrôles ---

  function range(key, label, unit = "", format = (v) => v) {
    const { min, max } = view.limits[key];
    const id = `r-${key}`;
    const out = el("output", { htmlFor: id });
    const input = el("input", {
      type: "range",
      id,
      min,
      max,
      step: 1,
      oninput: () => {
        out.value = `${format(Number(input.value))}${unit}`;
        setValue(key, Number(input.value));
      },
    });
    controls.push(() => {
      input.value = getValue(key);
      out.value = `${format(getValue(key))}${unit}`;
    });
    return el("div", { class: "reglage" }, el("div", { class: "reglage-tete" }, el("label", { htmlFor: id }, label), out), input);
  }

  function color(key, label, swatches) {
    const id = `c-${key}`;
    const input = el("input", { type: "color", id, oninput: () => setValue(key, input.value.toLowerCase()) });
    const buttons = swatches.map((hex) => {
      const b = el("button", {
        type: "button",
        title: hex,
        "aria-label": `${label} ${hex}`,
        onclick: () => {
          setValue(key, hex);
          refreshControls();
        },
      });
      b.style.setProperty("background", hex);
      return b;
    });
    controls.push(() => {
      const v = getValue(key);
      input.value = v;
      buttons.forEach((b, i) => b.setAttribute("aria-pressed", String(swatches[i] === v)));
    });
    return el(
      "div",
      { class: "reglage" },
      el("div", { class: "reglage-tete" }, el("label", { htmlFor: id }, label)),
      el("div", { class: "couleurs" }, input, ...buttons),
    );
  }

  function choice(key, label, options) {
    const buttons = options.map(([value, text, styleFont]) => {
      const b = el("button", {
        type: "button",
        onclick: () => {
          setValue(key, value);
          refreshControls();
        },
      });
      if (styleFont) {
        const big = el("span", { class: "grand" }, "Aa");
        big.style.setProperty("font-family", styleFont);
        b.append(big, text);
      } else {
        b.append(text);
      }
      return b;
    });
    controls.push(() => buttons.forEach((b, i) => b.setAttribute("aria-pressed", String(options[i][0] === getValue(key)))));
    return el(
      "div",
      { class: "reglage", role: "group", "aria-label": label },
      el("div", { class: "reglage-tete" }, el("span", {}, label)),
      el("div", { class: "choix" }, ...buttons),
    );
  }

  function toggle(key, label) {
    const input = el("input", { type: "checkbox", role: "switch", onchange: () => setValue(key, input.checked) });
    controls.push(() => (input.checked = Boolean(getValue(key))));
    return el("label", { class: "interrupteur" }, el("span", {}, label), input);
  }

  const ACCENTS = ["#5b6cff", "#b16cff", "#e85d75", "#d6336c", "#ff7a45", "#f5b700", "#2bb673", "#0088b8", "#00e5ff", "#222222"];
  const TINTS = ["#ffffff", "#fff0f4", "#fff3ea", "#f2fdff", "#eef0ff", "#141a33", "#0a1f24", "#07050f", "#000000"];
  const TEXTS = ["#1f2333", "#1a1a1a", "#3a1020", "#0c2a33", "#ffffff", "#eef1ff", "#f4f0ff", "#e9fff7"];
  const WALLS = ["#a8c0ff", "#fbc2eb", "#c2f0e6", "#ff9a56", "#ff4f8b", "#8e54e9", "#1b2a6b", "#0fbf8a", "#ffd6a5", "#111111"];

  function buildControls() {
    const pct = "%";
    $("settings").querySelector('[data-panel="couleurs"]').append(
      color("accent", "Couleur principale", ACCENTS),
      color("accent2", "Couleur secondaire (dégradés)", ACCENTS),
      color("text", "Texte", TEXTS),
      color("glass_tint", "Teinte du verre", TINTS),
    );
    $("settings").querySelector('[data-panel="verre"]').append(
      range("glass_opacity", "Opacité", pct),
      range("blur", "Flou", " px"),
      range("saturation", "Saturation", pct),
      range("border_width", "Épaisseur de la bordure", " px"),
      range("border_opacity", "Visibilité de la bordure", pct),
      range("shadow", "Ombre", pct),
      toggle("shine", "Reflet lumineux"),
    );
    $("settings").querySelector('[data-panel="texte"]').append(
      choice(
        "font",
        "Police",
        Object.entries(FONTS).map(([id, f]) => [id, f.label, f.stack]),
      ),
      range("font_scale", "Taille du texte", pct),
      range("radius", "Arrondi des coins", " px"),
      choice("density", "Espacement", [
        ["compact", "Serré"],
        ["normal", "Normal"],
        ["spacious", "Aéré"],
      ]),
    );

    const fileInput = el("input", { type: "file", accept: "image/jpeg,image/png,image/webp", hidden: true, onchange: () => pickImage(fileInput) });
    const removeImage = el("button", { type: "button", class: "danger", onclick: () => setImage(null) }, "Retirer l'image");
    controls.push(() => (removeImage.hidden = !wallpaperImage));
    $("settings").querySelector('[data-panel="fond"]').append(
      choice("wallpaper", "Type de fond", [
        ["mesh", "Taches de couleur"],
        ["linear", "Dégradé"],
        ["solid", "Couleur unie"],
        ["image", "Mon image"],
      ]),
      el(
        "div",
        { class: "reglage" },
        el("div", { class: "actions" }, removeImage, el("button", { type: "button", onclick: () => fileInput.click() }, "Choisir une image…"), fileInput),
      ),
      color("wallpaper_colors.0", "Couleur 1", WALLS),
      color("wallpaper_colors.1", "Couleur 2", WALLS),
      color("wallpaper_colors.2", "Couleur 3", WALLS),
      range("wallpaper_blur", "Flou du fond", " px"),
      range("wallpaper_dim", "Assombrir le fond", pct),
      range("grain", "Grain", pct),
      toggle("animate", "Fond animé"),
    );
  }

  // --- Image de fond ---

  // L'image est réduite (2400 px au plus) et convertie en JPEG avant d'être
  // enregistrée, pour rester légère.
  function pickImage(input) {
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    status("Préparation de l'image…");
    const reader = new FileReader();
    reader.onerror = () => status("Impossible de lire cette image.", true);
    reader.onload = () => {
      const img = new Image();
      img.onerror = () => status("Ce fichier n'est pas une image lisible.", true);
      img.onload = () => {
        const scale = Math.min(1, 2400 / Math.max(img.width, img.height));
        const canvas = document.createElement("canvas");
        canvas.width = Math.round(img.width * scale);
        canvas.height = Math.round(img.height * scale);
        canvas.getContext("2d").drawImage(img, 0, 0, canvas.width, canvas.height);
        setImage(canvas.toDataURL("image/jpeg", 0.85));
      };
      img.src = reader.result;
    };
    reader.readAsDataURL(file);
  }

  async function setImage(dataUrl) {
    try {
      await write("set_wallpaper", { dataUrl });
    } catch (e) {
      status(String(e), true);
      return;
    }
    wallpaperImage = dataUrl;
    if (dataUrl) current.wallpaper = "image";
    else if (current.wallpaper === "image") current.wallpaper = "mesh";
    refreshControls();
    changed();
  }

  // --- Préréglages et thèmes personnels ---

  // Mini-aperçu d'un thème : son fond, un panneau de verre et ses couleurs.
  function preview(a) {
    const box = el("span", { class: "apercu" });
    const [c0, c1, c2] = a.wallpaper_colors;
    const bg = {
      mesh: `radial-gradient(at 15% 20%, ${c0}, transparent 60%), radial-gradient(at 85% 25%, ${c1}, transparent 55%), radial-gradient(at 50% 100%, ${c2}, transparent 65%), ${c1}`,
      linear: `linear-gradient(135deg, ${c0}, ${c1}, ${c2})`,
      solid: c0,
      image: `linear-gradient(135deg, ${c0}, ${c2})`,
    }[a.wallpaper];
    box.style.setProperty("background", bg);
    const glass = el("span", { class: "apercu-verre verre" });
    for (const [name, value] of Object.entries(themeVars(a))) glass.style.setProperty(name, value);
    glass.style.setProperty("border-radius", `${Math.min(a.radius, 14)}px`);
    const bar = (w, c) => {
      const i = el("i");
      i.style.setProperty("width", w);
      i.style.setProperty("background", c);
      return i;
    };
    glass.append(bar("60%", `linear-gradient(90deg, ${a.accent}, ${a.accent2})`), bar("90%", rgba(a.text, 0.5)), bar("75%", rgba(a.text, 0.3)));
    box.append(glass);
    return box;
  }

  const SKIP = new Set(["preset"]);
  const same = (a, b) => Object.keys(a).every((k) => SKIP.has(k) || JSON.stringify(a[k]) === JSON.stringify(b[k]));

  let presetCards = [];
  function renderPresets() {
    presetCards = view.presets.map((p) => {
      const card = el(
        "button",
        {
          type: "button",
          class: "carte-theme",
          "data-preset": p.id,
          onclick: async () => {
            try {
              replace(await replacing("apply_preset", { id: p.id }), `Préréglage « ${p.name} » appliqué`);
            } catch (e) {
              status(String(e), true);
            }
          },
        },
        preview(p.appearance),
        el("strong", {}, p.name),
        el("small", {}, p.description),
      );
      card.style.setProperty("font-family", (FONTS[p.appearance.font] ?? FONTS.system).stack);
      return { card, preset: p };
    });
    $("presets").replaceChildren(...presetCards.map((c) => c.card));
    renderPresetState();
  }

  function renderPresetState() {
    for (const { card, preset } of presetCards) {
      const active = preset.id === current.preset;
      card.setAttribute("aria-pressed", String(active));
      card.querySelector(".badge")?.remove();
      if (active && !same(current, preset.appearance)) card.append(el("span", { class: "badge" }, "modifié"));
    }
    const p = view.presets.find((x) => x.id === current.preset);
    $("reset-preset").textContent = p ? `Revenir à « ${p.name} »` : "Réinitialiser";
  }

  function renderSaved() {
    $("saved-empty").hidden = view.saved.length > 0;
    $("saved").replaceChildren(
      ...view.saved.map((t) => {
        const dot = el("span", { class: "pastille" });
        const [c0, c1, c2] = t.appearance.wallpaper_colors;
        dot.style.setProperty("background", `conic-gradient(${t.appearance.accent}, ${c0}, ${c1}, ${c2}, ${t.appearance.accent})`);
        return el(
          "li",
          {},
          dot,
          el("span", { title: t.name }, t.name),
          el(
            "button",
            {
              type: "button",
              onclick: async () => {
                try {
                  replace(await replacing("apply_saved_theme", { id: t.id }), `Thème « ${t.name} » appliqué`);
                } catch (e) {
                  status(String(e), true);
                }
              },
            },
            "Appliquer",
          ),
          el(
            "button",
            {
              type: "button",
              class: "danger",
              "aria-label": `Supprimer ${t.name}`,
              onclick: async () => {
                try {
                  view = await write("delete_theme", { id: t.id });
                  renderSaved();
                  status(`Thème « ${t.name} » supprimé`);
                } catch (e) {
                  status(String(e), true);
                }
              },
            },
            "Supprimer",
          ),
        );
      }),
    );
  }

  function refreshControls() {
    controls.forEach((f) => f());
    renderPresetState();
  }

  // Avant une action qui lit le thème côté cœur, on enregistre d'abord les
  // derniers réglages encore en attente.
  async function flush() {
    // Rien en attente : on attend quand même les écritures déjà parties.
    if (saveTimer === null) return writes;
    clearTimeout(saveTimer);
    saveTimer = null;
    await write("set_appearance", { appearance: current });
  }

  // --- Ouverture, onglets ---

  let panel = null;
  let openButton = null;

  function open() {
    panel.hidden = false;
    document.body.classList.add("reglages-ouverts");
    openButton.setAttribute("aria-expanded", "true");
    panel.querySelector('[aria-selected="true"]')?.focus();
  }

  function close() {
    flush().catch((e) => status(String(e), true));
    panel.hidden = true;
    document.body.classList.remove("reglages-ouverts");
    openButton.setAttribute("aria-expanded", "false");
    openButton.focus();
  }

  function selectTab(name) {
    for (const tab of panel.querySelectorAll('[role="tab"]')) tab.setAttribute("aria-selected", String(tab.dataset.tab === name));
    for (const section of panel.querySelectorAll("[data-panel]")) section.hidden = section.dataset.panel !== name;
    if (name === "partager") showExportCode();
  }

  async function showExportCode() {
    try {
      await flush();
      $("export-code").value = await write("export_theme");
    } catch (e) {
      status(String(e), true);
    }
  }

  function wire() {
    openButton.addEventListener("click", () => (panel.hidden ? open() : close()));
    $("close-settings").addEventListener("click", close);
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && !panel.hidden) close();
    });
    const tabs = [...panel.querySelectorAll('[role="tab"]')];
    tabs.forEach((tab, i) => {
      tab.addEventListener("click", () => selectTab(tab.dataset.tab));
      tab.addEventListener("keydown", (e) => {
        const step = { ArrowRight: 1, ArrowLeft: -1 }[e.key];
        if (!step) return;
        const next = tabs[(i + step + tabs.length) % tabs.length];
        next.focus();
        selectTab(next.dataset.tab);
      });
    });

    $("reset-preset").addEventListener("click", async () => {
      try {
        const p = view.presets.find((x) => x.id === current.preset) ?? view.presets[0];
        replace(await replacing("apply_preset", { id: p.id }), `Préréglage « ${p.name} » rétabli`);
      } catch (e) {
        status(String(e), true);
      }
    });

    $("save-theme").addEventListener("submit", async (e) => {
      e.preventDefault();
      const name = $("theme-name").value;
      try {
        await flush();
        view = await write("save_theme", { name });
        $("theme-name").value = "";
        renderSaved();
        status("Thème enregistré dans « Mes thèmes »");
      } catch (err) {
        status(String(err), true);
      }
    });

    $("copy-code").addEventListener("click", async () => {
      const box = $("export-code");
      box.select();
      try {
        await navigator.clipboard.writeText(box.value);
        status("Code copié");
      } catch {
        // Presse-papiers indisponible : le code est sélectionné, Ctrl+C suffit.
        status("Code sélectionné : copie-le avec Ctrl+C (ou appui long)");
      }
    });

    $("import-theme").addEventListener("click", async () => {
      try {
        replace(await replacing("import_theme", { code: $("import-code").value }), "Thème importé");
        $("import-code").value = "";
      } catch (e) {
        status(String(e), true);
      }
    });
  }

  const domReady = new Promise((resolve) => {
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", resolve, { once: true });
    else resolve();
  });

  (async () => {
    try {
      await domReady;
      panel = $("settings");
      openButton = $("open-settings");
      [view, wallpaperImage] = await Promise.all([invoke("get_appearance"), invoke("get_wallpaper")]);
      wallpaperLoaded = true;
    } catch (e) {
      console.error("Apparence indisponible :", e);
      return;
    }
    current = view.current;
    apply(current);
    cache(current);
    buildControls();
    renderPresets();
    renderSaved();
    refreshControls();
    wire();
    // Enregistre les derniers réglages si l'appli passe en arrière-plan ou se ferme.
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "hidden") flush().catch(() => {});
    });
    window.addEventListener("pagehide", () => flush().catch(() => {}));
    if (view.warning) status(view.warning, true);
    // Pour les tests de bout en bout.
    window.__apparence = { get: () => current, presets: () => view.presets.map((p) => p.id) };
  })();
})();
