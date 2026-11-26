// Animés regardés ensemble : recherche sur AniList, liste avec affiches,
// épisode où l'on s'est arrêté et statut.
// Les affiches sont gardées sur l'appareil par Rust (src-tauri/src/animes.rs).

(() => {
  const tauri = window.__TAURI__;
  if (!tauri?.core?.invoke) return;
  const { invoke } = tauri.core;
  const $ = (id) => document.getElementById(id);
  const searchForm = $("anime-search");
  const queryInput = $("anime-query");
  const searchButton = $("anime-search-go");
  const statusLine = $("anime-status");
  const errorBox = $("anime-error");
  const results = $("anime-results");
  const resultsList = $("anime-results-list");
  const filters = $("anime-filters");
  const list = $("animes");

  const LABELS = {
    "en-cours": "En cours",
    "a-voir": "À voir",
    "en-pause": "En pause",
    fini: "Fini",
    abandonne: "Abandonné",
  };
  const FILTERS = [
    ["", "Tous"],
    ["en-cours", "En cours"],
    ["a-voir", "À voir"],
    ["en-pause", "En pause"],
    ["fini", "Finis"],
    ["abandonne", "Abandonnés"],
  ];

  let animes = [];
  let filter = "";
  try {
    filter = localStorage.getItem("anime-filter") ?? "";
  } catch {}
  if (!FILTERS.some(([value]) => value === filter)) filter = "";

  // Affiches déjà chargées : identifiant AniList -> adresse blob:.
  const covers = new Map();
  // Affiches pas encore téléchargées par Rust (à redemander plus tard).
  const missing = new Set();

  const me = () => $("author").value;

  function showError(message) {
    errorBox.textContent = message ?? "";
    errorBox.hidden = !message;
  }

  function showStatus(message) {
    statusLine.textContent = message ?? "";
    statusLine.hidden = !message;
  }

  function formatDate(ms) {
    return new Date(ms).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" });
  }

  function el(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text != null) node.textContent = text;
    return node;
  }

  function button(label, onClick, className) {
    const b = el("button", className, label);
    b.type = "button";
    b.addEventListener("click", onClick);
    return b;
  }

  // Type de l'image d'après ses premiers octets (Rust n'en garde que de ces sortes).
  function imageType(bytes) {
    if (bytes[0] === 0x89) return "image/png";
    if (bytes[0] === 0x47) return "image/gif";
    if (bytes[0] === 0x52) return "image/webp";
    return "image/jpeg";
  }

  async function loadCover(anilistId) {
    if (covers.has(anilistId)) return covers.get(anilistId);
    try {
      const buffer = await invoke("anime_cover", { anilistId });
      const bytes = new Uint8Array(buffer);
      const url = URL.createObjectURL(new Blob([bytes], { type: imageType(bytes) }));
      covers.set(anilistId, url);
      missing.delete(anilistId);
      return url;
    } catch {
      missing.add(anilistId);
      return null;
    }
  }

  // Affiche, ou à défaut un carré de la couleur de l'animé avec ses initiales.
  function poster(info) {
    const box = el("div", "affiche");
    if (info.color) box.style.setProperty("--anime-color", info.color);
    const initials = info.title
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0])
      .join("")
      .toUpperCase();
    box.append(el("span", "initiales", initials));
    if (info.anilist_id != null && info.cover_url) {
      loadCover(info.anilist_id).then((url) => {
        if (!url) return;
        const img = el("img");
        img.alt = "";
        img.src = url;
        box.replaceChildren(img);
      });
    }
    return box;
  }

  function details(info) {
    const parts = [];
    if (info.year) parts.push(String(info.year));
    if (info.format) parts.push(info.format.replace("_", " "));
    if (info.episodes) parts.push(`${info.episodes} ép.`);
    return parts.join(" · ");
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

  // ---------- Recherche ----------

  let lastQuery = "";

  function renderResults(found) {
    resultsList.replaceChildren(
      ...found.map((info) => {
        const li = el("li", "resultat");
        // Dans les résultats, l'affiche vient directement d'AniList (pas encore en cache).
        const box = el("div", "affiche");
        if (info.color) box.style.setProperty("--anime-color", info.color);
        if (info.cover_url) {
          const img = el("img");
          img.alt = "";
          img.loading = "lazy";
          img.referrerPolicy = "no-referrer";
          img.src = info.cover_url;
          box.append(img);
        }
        const text = el("div", "texte");
        text.append(el("span", "titre", info.title));
        const meta = details(info);
        if (meta) text.append(el("span", "meta", meta));
        const already = animes.some((a) => a.info.anilist_id === info.anilist_id);
        const add = button(already ? "Déjà dans la liste" : "Ajouter", async () => {
          add.disabled = true;
          const ok = await run(() => invoke("add_anime", { author: me(), info }));
          if (ok) {
            add.textContent = "Ajouté";
            showStatus(`« ${info.title} » ajouté à votre liste.`);
          } else {
            add.disabled = false;
          }
        });
        add.disabled = already;
        li.append(box, text, add);
        return li;
      }),
    );
    results.hidden = false;
  }

  searchForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (searchButton.disabled) return;
    const query = queryInput.value.trim();
    if (!query) {
      showError("Écris le titre de l'animé à chercher.");
      return;
    }
    showError(null);
    showStatus("Recherche sur AniList…");
    searchButton.disabled = true;
    lastQuery = query;
    try {
      const found = await invoke("search_animes", { query });
      showStatus(found.length ? null : `Rien trouvé pour « ${query} ». Tu peux l'ajouter à la main.`);
      renderResults(found);
    } catch (e) {
      showStatus(null);
      showError(`${e} Tu peux aussi l'ajouter à la main.`);
      resultsList.replaceChildren();
      results.hidden = false;
    } finally {
      searchButton.disabled = false;
    }
  });

  $("anime-manual").addEventListener("click", async () => {
    // Ce qui est écrit maintenant l'emporte sur la dernière recherche.
    const title = queryInput.value.trim() || lastQuery;
    const info = {
      anilist_id: null,
      title,
      title_native: "",
      cover_url: "",
      color: "",
      format: "",
      year: null,
      genres: [],
      episodes: null,
    };
    const ok = await run(() => invoke("add_anime", { author: me(), info }));
    if (ok) showStatus(`« ${title} » ajouté à votre liste (sans affiche).`);
  });

  $("anime-results-close").addEventListener("click", () => {
    results.hidden = true;
    resultsList.replaceChildren();
    showStatus(null);
    queryInput.value = "";
    lastQuery = "";
  });

  // ---------- Liste ----------

  function renderFilters() {
    filters.replaceChildren(
      ...FILTERS.map(([value, label]) => {
        const count = value ? animes.filter((a) => a.status === value).length : animes.length;
        const b = button(count ? `${label} (${count})` : label, () => {
          filter = value;
          try {
            localStorage.setItem("anime-filter", value);
          } catch {}
          render();
        });
        b.setAttribute("aria-pressed", String(filter === value));
        return b;
      }),
    );
    filters.hidden = animes.length === 0;
  }

  function progressText(anime) {
    const total = anime.info.episodes;
    if (anime.episode === 0) return total ? `Pas commencé · ${total} épisodes` : "Pas commencé";
    let text = total ? `Arrêtés à l'épisode ${anime.episode} / ${total}` : `Arrêtés à l'épisode ${anime.episode}`;
    if (anime.episode_by && anime.episode_at) text += ` · ${anime.episode_by}, le ${formatDate(anime.episode_at)}`;
    return text;
  }

  function card(anime) {
    const { info } = anime;
    const li = el("li", "anime");
    li.dataset.status = anime.status;

    const text = el("div", "texte");
    const title = el("span", "titre", info.title);
    text.append(title);
    if (info.title_native && info.title_native !== info.title) text.append(el("span", "meta", info.title_native));
    const meta = details(info);
    if (meta) text.append(el("span", "meta", meta));
    if (info.genres.length) text.append(el("span", "genres", info.genres.slice(0, 3).join(" · ")));

    const progress = el("p", "progression", progressText(anime));
    const total = info.episodes;
    const bar = el("div", "barre-episodes");
    bar.setAttribute("aria-hidden", "true");
    const fill = el("i");
    fill.style.setProperty("--progress", total ? String(Math.min(1, anime.episode / total)) : anime.episode ? "1" : "0");
    if (!total) bar.classList.add("inconnu");
    bar.append(fill);

    // Épisode : − / champ / +.
    const setEpisode = async (episode, control) => {
      control.disabled = true;
      const ok = await run(() => invoke("set_anime_episode", { id: anime.id, episode, by: me() }));
      // Refusé : le champ reprend l'épisode enregistré.
      if (!ok) input.value = String(anime.episode);
      control.disabled = false;
    };
    const stepper = el("div", "episode");
    const minus = button("−", () => setEpisode(anime.episode - 1, minus));
    minus.setAttribute("aria-label", `Épisode précédent de « ${info.title} »`);
    minus.disabled = anime.episode <= 0;
    const input = el("input");
    input.type = "number";
    input.inputMode = "numeric";
    input.min = "0";
    if (total) input.max = String(total);
    input.value = String(anime.episode);
    input.setAttribute("aria-label", `Épisode où vous vous êtes arrêtés dans « ${info.title} »`);
    input.addEventListener("change", () => {
      const episode = Number.parseInt(input.value, 10);
      if (!Number.isInteger(episode) || episode < 0) {
        input.value = String(anime.episode);
        showError("Indique un numéro d'épisode.");
        return;
      }
      setEpisode(episode, input);
    });
    const plus = button("+1", () => setEpisode(anime.episode + 1, plus), "principal");
    plus.setAttribute("aria-label", `Épisode suivant de « ${info.title} »`);
    plus.disabled = total != null && anime.episode >= total;
    stepper.append(minus, input, plus);

    const status = el("select");
    status.setAttribute("aria-label", `Statut de « ${info.title} »`);
    for (const [value, label] of Object.entries(LABELS)) {
      const option = el("option", null, label);
      option.value = value;
      status.append(option);
    }
    status.value = anime.status;
    status.addEventListener("change", async () => {
      status.disabled = true;
      const ok = await run(() => invoke("set_anime_status", { id: anime.id, status: status.value }));
      if (!ok) status.value = anime.status;
      status.disabled = false;
    });

    const actions = el("div", "actions");
    if (info.anilist_id != null) {
      const update = button("Mettre à jour", async () => {
        update.disabled = true;
        const ok = await run(() => invoke("refresh_anime", { id: anime.id }));
        if (ok) showStatus(`Informations de « ${info.title} » mises à jour depuis AniList.`);
        update.disabled = false;
      });
      update.title = "Relire le nombre d'épisodes et l'affiche sur AniList";
      actions.append(update);
    }
    let confirmDelete = null;
    const del = button("Supprimer", async () => {
      // Deux appuis : pas de boîte de dialogue (bloquante dans la webview).
      if (!confirmDelete) {
        del.textContent = "Confirmer ?";
        confirmDelete = setTimeout(() => {
          del.textContent = "Supprimer";
          confirmDelete = null;
        }, 4000);
        return;
      }
      clearTimeout(confirmDelete);
      confirmDelete = null;
      del.disabled = true;
      const ok = await run(() => invoke("delete_anime", { id: anime.id }));
      if (!ok) {
        del.textContent = "Supprimer";
        del.disabled = false;
      }
    }, "danger");
    actions.append(del);

    const controls = el("div", "controles");
    controls.append(stepper, status);

    const body = el("div", "corps");
    body.append(text, progress, bar, controls, actions);
    li.append(poster(info), body);
    return li;
  }

  function render() {
    renderFilters();
    const shown = filter ? animes.filter((a) => a.status === filter) : animes;
    list.replaceChildren(...shown.map(card));
    const empty = $("animes-empty");
    empty.hidden = shown.length > 0;
    empty.textContent = animes.length
      ? "Aucun animé dans cette catégorie."
      : "Aucun animé pour l'instant : cherchez-en un pour commencer votre liste.";
  }

  async function refresh() {
    try {
      animes = await invoke("list_animes");
      // Les affiches des animés retirés ne servent plus.
      const ids = new Set(animes.map((a) => a.info.anilist_id));
      for (const [id, url] of covers) {
        if (!ids.has(id)) {
          URL.revokeObjectURL(url);
          covers.delete(id);
        }
      }
      render();
    } catch (e) {
      showError(String(e));
    }
  }

  refresh();
  tauri.event?.listen("docs-changed", (event) => {
    if (event.payload?.includes("animes")) refresh();
  });
  // Rust vient de télécharger des affiches : on réaffiche celles qui manquaient.
  tauri.event?.listen("anime-covers", () => {
    if (missing.size) {
      missing.clear();
      render();
    }
  });
})();
