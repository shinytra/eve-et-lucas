// Lieux et présence : où est chacun (maison, université… ou ailleurs), les
// lieux de ce téléphone (géorepérage) et les dernières arrivées.
// Sur téléphone, le plugin maison `geofence` (plugins/tauri-plugin-geofence).

(() => {
  const tauri = window.__TAURI__;
  if (!tauri?.core?.invoke) return;
  const { invoke } = tauri.core;
  const $ = (id) => document.getElementById(id);
  const errorBox = $("home-error");
  const isPhone = /Android/i.test(navigator.userAgent);
  // Rayon de chaque zone, en mètres (le GPS est précis à ~20-50 m).
  const RADIUS_M = 150;
  const LIEU = { maison: "🏠", etudes: "🎓", travail: "💼", autre: "📍" };

  function showError(message) {
    errorBox.textContent = message ?? "";
    errorBox.hidden = !message;
  }

  function el(tag, className, text) {
    const e = document.createElement(tag);
    if (className) e.className = className;
    if (text != null) e.textContent = text;
    return e;
  }

  function formatDate(ms) {
    return new Date(ms).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" });
  }

  // « depuis 18:42 », « depuis hier 18:42 », « depuis le 12 nov. » (comme le widget).
  function depuis(ms) {
    const d = new Date(ms);
    const jour = (x) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
    const ecart = Math.round((jour(new Date()) - jour(d)) / 86_400_000);
    const heure = d.toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" });
    if (ecart <= 0) return `depuis ${heure}`;
    if (ecart === 1) return `depuis hier ${heure}`;
    return `depuis le ${d.toLocaleDateString("fr-FR", { day: "numeric", month: "short" })}`;
  }

  function renderPresence(list) {
    const moi = $("author").value.trim().toLowerCase();
    $("presence").replaceChildren(
      ...list.map((p) => {
        const li = el("li", "carte-lieu");
        const ou = !p.inside ? "ailleurs" : p.place || "à la maison";
        li.append(el("span", "emoji", p.inside ? LIEU[p.place_kind] ?? "📍" : "🚶"));
        const texte = el("div", "text");
        const nom = p.who.trim().toLowerCase() === moi ? `${p.who} (toi)` : p.who;
        texte.append(el("span", "title", `${nom} : ${ou}`));
        const quitte = !p.inside && p.place ? ` · a quitté ${p.place}` : "";
        texte.append(el("span", "meta", `${depuis(p.at)}${quitte}`));
        li.append(texte);
        return li;
      }),
    );
    $("presence-empty").hidden = list.length > 0;
  }

  async function refresh() {
    try {
      const [arrivals, presence] = await Promise.all([invoke("list_arrivals"), invoke("list_presence")]);
      renderPresence(presence);
      $("arrivals").replaceChildren(
        ...arrivals.slice(0, 5).map((a) => {
          const li = document.createElement("li");
          li.textContent = `${a.who} · arrivée le ${formatDate(a.at)}`;
          return li;
        }),
      );
    } catch (e) {
      showError(String(e));
    }
  }

  // Android exige deux étapes : la position, puis « Toujours autoriser ».
  async function ensurePermissions() {
    let status = await invoke("plugin:geofence|check_permissions");
    if (status.location !== "granted") {
      status = await invoke("plugin:geofence|request_permissions", { permissions: ["location", "notifications"] });
      if (status.location !== "granted") {
        throw new Error("La position est nécessaire pour savoir dans quel lieu tu es.");
      }
    }
    if (status.backgroundLocation !== "granted") {
      showError("Android va te demander la position « en permanence » : choisis « Toujours autoriser », c'est ce qui permet de voir tes entrées et sorties appli fermée.");
      status = await invoke("plugin:geofence|request_permissions", { permissions: ["backgroundLocation"] });
      if (status.backgroundLocation !== "granted") {
        throw new Error("Choisis « Toujours autoriser » pour la position : sinon tes entrées et sorties ne sont pas vues quand l'appli est fermée.");
      }
    }
  }

  async function showWho() {
    try {
      const who = (await invoke("arrival_person")).trim();
      $("home-who").textContent = who
        ? `Ce téléphone est celui de ${who} (modifiable dans Paramètres). L'autre voit dans quel lieu tu es.`
        : "Choisis d'abord à qui est ce téléphone, dans Paramètres.";
    } catch (e) {
      showError(String(e));
    }
  }

  function placeItem(place) {
    const li = el("li", "carte-lieu");
    li.append(el("span", "emoji", LIEU[place.kind] ?? "📍"));
    const texte = el("div", "text");
    const nom = el("span", "title", place.name);
    const details = [place.inside ? "tu y es" : null, place.announce ? "sonne à l'arrivée" : "sans sonnerie"].filter(Boolean);
    texte.append(nom, el("span", "meta", details.join(" · ")));

    const edit = window.__ui.iconButton("modifier", `Renommer « ${place.name} »`);
    const del = window.__ui.iconButton("supprimer", `Retirer « ${place.name} »`, "danger");
    edit.addEventListener("click", () => {
      // Renommer sur place : champ + Enregistrer.
      const form = el("form", "renommer");
      form.noValidate = true;
      const input = el("input");
      input.value = place.name;
      input.maxLength = 40;
      input.setAttribute("aria-label", "Nouveau nom");
      const ok = el("button", null, "Enregistrer");
      ok.type = "submit";
      form.append(input, ok);
      form.addEventListener("submit", async (event) => {
        event.preventDefault();
        showError(null);
        try {
          await invoke("plugin:geofence|rename_place", { id: place.id, name: input.value });
          await refreshPlaces();
        } catch (e) {
          showError(String(e));
        }
      });
      nom.replaceWith(form);
      input.focus();
    });
    del.addEventListener("click", async () => {
      showError(null);
      del.disabled = true;
      try {
        await invoke("plugin:geofence|remove_place", { id: place.id });
        await window.__ui.sortie(li);
        await refreshPlaces();
      } catch (e) {
        del.disabled = false;
        showError(String(e));
      }
    });
    const actions = el("div", "actions");
    actions.append(edit, del);
    li.append(texte, actions);
    return li;
  }

  async function refreshPlaces() {
    try {
      const places = await invoke("plugin:geofence|list_places");
      $("places").replaceChildren(...places.map(placeItem));
      $("places-empty").hidden = places.length > 0;
    } catch (e) {
      showError(String(e));
    }
  }

  async function setup() {
    $("home-phone").hidden = false;
    // Le prénom se choisit au premier lancement, puis dans Paramètres.
    showWho();
    $("author").addEventListener("change", () => {
      showWho();
      refresh();
    });
    try {
      $("home-ring").checked = localStorage.getItem("home-ring") === "1";
    } catch {}
    refreshPlaces();

    $("home-ring").addEventListener("change", async () => {
      showError(null);
      const ring = $("home-ring").checked;
      try {
        await invoke("plugin:geofence|set_ring_on_arrival", { ring });
        try {
          localStorage.setItem("home-ring", ring ? "1" : "0");
        } catch {}
      } catch (e) {
        $("home-ring").checked = !ring;
        showError(String(e));
      }
    });

    // Une maison sonne d'office, un autre lieu non (modifiable).
    $("place-kind").addEventListener("change", () => {
      $("place-announce").checked = $("place-kind").value === "maison";
    });

    $("place-form").addEventListener("submit", async (event) => {
      event.preventDefault();
      const button = $("place-add");
      button.disabled = true;
      showError(null);
      try {
        // Pas de prénom par défaut : un lieu au mauvais prénom ferait croire
        // à l'autre que la mauvaise personne y est.
        if (!(await invoke("arrival_person")).trim()) {
          throw new Error("Choisis d'abord à qui est ce téléphone, dans Paramètres.");
        }
        const name = $("place-name").value.trim();
        if (!name) throw new Error("Donne un nom à ce lieu (Maison, Université…).");
        await ensurePermissions();
        showError(null);
        const pos = await invoke("plugin:geofence|current_position");
        await invoke("plugin:geofence|add_place", {
          name,
          kind: $("place-kind").value,
          announce: $("place-announce").checked,
          lat: pos.lat,
          lng: pos.lng,
          radiusM: RADIUS_M,
        });
        $("place-name").value = "";
        await refreshPlaces();
      } catch (e) {
        showError(String(e?.message ?? e));
      } finally {
        button.disabled = false;
      }
    });
  }

  if (isPhone) setup();
  else $("author").addEventListener("change", refresh);
  refresh();
  tauri.event?.listen("docs-changed", (event) => {
    if (event.payload?.some((d) => d === "arrivals" || d === "presence")) {
      refresh();
      if (isPhone) refreshPlaces();
    }
  });
})();
