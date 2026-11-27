// Musiques recommandées : un morceau YouTube Music à la fois, ajouté par son
// lien (ou depuis le menu Partager d'Android, voir src-tauri/src/music.rs),
// ouvert dans YouTube Music, et noté « écoutée ».

(() => {
  const tauri = window.__TAURI__;
  if (!tauri?.core?.invoke) return;
  const { invoke } = tauri.core;
  const $ = (id) => document.getElementById(id);
  const form = $("music-form");
  const linkInput = $("music-link");
  const titleInput = $("music-title");
  const artistInput = $("music-artist");
  const messageInput = $("music-message");
  const saveButton = $("music-save");
  const statusLine = $("music-status");
  const errorBox = $("music-error");
  const filters = $("music-filters");
  const list = $("musiques");

  const FILTERS = [
    ["a-ecouter", "À écouter"],
    ["ecoutees", "Écoutées"],
    ["", "Toutes"],
  ];
  let filter = "a-ecouter";
  try {
    filter = localStorage.getItem("music-filter") ?? filter;
  } catch {}
  if (!FILTERS.some(([value]) => value === filter)) filter = "a-ecouter";

  let musics = [];
  let editingId = null;

  const me = () => $("author").value;

  function showError(message) {
    errorBox.textContent = message ?? "";
    errorBox.hidden = !message;
  }

  function showStatus(message) {
    statusLine.textContent = message ?? "";
    statusLine.hidden = !message;
  }

  function el(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text != null) node.textContent = text;
    return node;
  }

  function formatDate(ms) {
    return new Date(ms).toLocaleDateString("fr-FR", { day: "numeric", month: "short" });
  }

  async function run(action) {
    showError(null);
    try {
      await action();
    } catch (e) {
      showError(String(e));
      return false;
    }
    await refresh();
    return true;
  }

  // ---------- Formulaire (ajout et modification) ----------

  function resetForm() {
    editingId = null;
    form.reset();
    linkInput.hidden = false;
    artistInput.hidden = true;
    titleInput.placeholder = "Titre (trouvé tout seul si tu le laisses vide)";
    saveButton.textContent = "Ajouter";
    window.__ui?.fermer(form);
  }

  function startEdit(music) {
    editingId = music.id;
    linkInput.hidden = true;
    artistInput.hidden = false;
    titleInput.placeholder = "Titre";
    titleInput.value = music.title;
    artistInput.value = music.artist;
    messageInput.value = music.message;
    saveButton.textContent = "Enregistrer";
    showError(null);
    window.__ui?.ouvrir(form);
    titleInput.focus();
  }

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (saveButton.disabled) return;
    if (!editingId && !linkInput.value.trim()) {
      showError("Colle le lien du morceau (YouTube Music ou YouTube).");
      return;
    }
    saveButton.disabled = true;
    if (!editingId) showStatus("Recherche du titre sur YouTube…");
    let added = null;
    const ok = await run(async () => {
      if (editingId) {
        await invoke("update_music", {
          id: editingId,
          title: titleInput.value,
          artist: artistInput.value,
          message: messageInput.value,
        });
      } else {
        added = await invoke("add_music", {
          author: me(),
          link: linkInput.value,
          title: titleInput.value,
          message: messageInput.value,
        });
      }
    });
    saveButton.disabled = false;
    showStatus(null);
    if (!ok) return;
    if (added) {
      showStatus(added.title ? `« ${added.title} » recommandée.` : "Musique recommandée (titre introuvable : tu peux le modifier).");
      setFilter("a-ecouter");
    }
    resetForm();
  });

  $("music-cancel").addEventListener("click", () => {
    resetForm();
    showError(null);
  });

  // ---------- Liste ----------

  function setFilter(value) {
    filter = value;
    try {
      localStorage.setItem("music-filter", value);
    } catch {}
    render();
  }

  function renderFilters() {
    filters.replaceChildren(
      ...FILTERS.map(([value, label]) => {
        const count = musics.filter((m) => (value === "a-ecouter" ? !m.listened : value === "ecoutees" ? m.listened : true)).length;
        const b = el("button", null, count ? `${label} (${count})` : label);
        b.type = "button";
        b.setAttribute("aria-pressed", String(filter === value));
        b.addEventListener("click", () => setFilter(value));
        return b;
      }),
    );
    filters.hidden = musics.length === 0;
  }

  // Miniature du morceau (YouTube), ou une note de musique sans réseau.
  function cover(music) {
    const box = el("div", "pochette");
    box.append(el("span", "note-musique", "♪"));
    if (music.thumbnail) {
      const img = el("img");
      img.alt = "";
      img.loading = "lazy";
      img.referrerPolicy = "no-referrer";
      img.addEventListener("load", () => box.replaceChildren(img));
      img.src = music.thumbnail;
    }
    return box;
  }

  function card(music) {
    const li = el("li", "musique");
    li.classList.toggle("ecoutee", music.listened);
    const name = music.title || "Morceau YouTube Music";

    const text = el("div", "texte");
    text.append(el("span", "titre", name));
    if (music.artist) text.append(el("span", "meta", music.artist));
    if (music.message) text.append(el("p", "mot", `« ${music.message} »`));
    let meta = `Recommandée par ${music.added_by} le ${formatDate(music.created_at)}`;
    if (music.listened) meta += ` · écoutée par ${music.listened_by} le ${formatDate(music.listened_at ?? music.updated_at)}`;
    text.append(el("span", "meta", meta));

    const listen = el("button", "principal", "▶ Écouter");
    listen.type = "button";
    listen.setAttribute("aria-label", `Écouter « ${name} » dans YouTube Music`);
    listen.addEventListener("click", async () => {
      showError(null);
      try {
        await invoke("open_music", { id: music.id });
      } catch (e) {
        showError(String(e));
      }
    });

    const check = el("label", "check");
    const box = el("input");
    box.type = "checkbox";
    box.checked = music.listened;
    box.addEventListener("change", async () => {
      box.disabled = true;
      const ok = await run(() => invoke("set_music_listened", { id: music.id, listened: box.checked, by: me() }));
      if (!ok) {
        box.checked = !box.checked;
        box.disabled = false;
      }
    });
    check.append(box, document.createTextNode(" Écoutée"));

    const edit = window.__ui.iconButton("modifier", `Modifier « ${name} »`);
    edit.addEventListener("click", () => startEdit(music));
    const del = window.__ui.iconButton("supprimer", `Supprimer « ${name} »`, "danger");
    del.addEventListener("click", async () => {
      del.disabled = true;
      await window.__ui.sortie(li);
      const ok = await run(() => invoke("delete_music", { id: music.id }));
      if (!ok) {
        del.disabled = false;
        li.classList.remove("sortie");
      } else if (editingId === music.id) resetForm();
    });

    const actions = el("div", "actions");
    actions.append(listen, check, edit, del);

    const body = el("div", "corps");
    body.append(text, actions);
    li.append(cover(music), body);
    return li;
  }

  function render() {
    renderFilters();
    const shown = musics.filter((m) => (filter === "a-ecouter" ? !m.listened : filter === "ecoutees" ? m.listened : true));
    list.replaceChildren(...shown.map(card));
    const empty = $("music-empty");
    empty.hidden = shown.length > 0;
    empty.textContent = !musics.length
      ? "Aucune musique pour l'instant : recommandez-vous votre premier morceau."
      : filter === "a-ecouter"
        ? "Tout est écouté 🎧"
        : "Aucune musique ici.";
  }

  async function refresh() {
    try {
      musics = await invoke("list_musics");
      render();
    } catch (e) {
      showError(String(e));
    }
  }

  // Morceaux reçus par le menu Partager : on montre l'écran et les résultats.
  function shared(results) {
    if (!results?.length) return;
    const errors = results.filter((r) => !r.ok).map((r) => r.message);
    const infos = results.filter((r) => r.ok).map((r) => r.message);
    showError(errors.join("\n") || null);
    showStatus(infos.join("\n") || null);
    if (infos.length) setFilter("a-ecouter");
    if (location.hash !== "#musiques") location.hash = "#musiques";
    refresh();
  }

  refresh();
  tauri.event?.listen("docs-changed", (event) => {
    if (event.payload?.includes("musiques")) refresh();
  });
  // L'écoute d'abord, puis le partage traité avant le chargement de l'interface.
  // Les résultats sont relus (et effacés) côté Rust : jamais montrés deux fois.
  const take = () => invoke("take_music_shares").then(shared).catch(() => {});
  Promise.resolve(tauri.event?.listen("music-shared", take))
    .catch(() => {})
    .then(take);
})();
