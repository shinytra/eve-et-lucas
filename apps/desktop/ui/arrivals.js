// Arrivée à la maison : réglage du téléphone (géofencing) et dernières arrivées.
// Sur téléphone, le plugin maison `geofence` (plugins/tauri-plugin-geofence).

(() => {
  const tauri = window.__TAURI__;
  if (!tauri?.core?.invoke) return;
  const { invoke } = tauri.core;
  const $ = (id) => document.getElementById(id);
  const errorBox = $("home-error");
  const isPhone = /Android/i.test(navigator.userAgent);
  // Rayon de la zone « maison », en mètres (le GPS est précis à ~20-50 m).
  const RADIUS_M = 150;
  const HOME_KEY = "home-set";

  function showError(message) {
    errorBox.textContent = message ?? "";
    errorBox.hidden = !message;
  }

  function formatDate(ms) {
    return new Date(ms).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" });
  }

  async function refresh() {
    try {
      const arrivals = await invoke("list_arrivals");
      $("arrivals").replaceChildren(
        ...arrivals.slice(0, 5).map((a) => {
          const li = document.createElement("li");
          li.textContent = `${a.who} · arrivée à la maison le ${formatDate(a.at)}`;
          return li;
        }),
      );
    } catch (e) {
      showError(String(e));
    }
  }

  function remembered() {
    try {
      return localStorage.getItem(HOME_KEY) === "1";
    } catch {
      return false;
    }
  }

  function remember(on) {
    try {
      if (on) localStorage.setItem(HOME_KEY, "1");
      else localStorage.removeItem(HOME_KEY);
    } catch {}
  }

  function showState() {
    const on = remembered();
    $("home-state").textContent = on
      ? `Maison enregistrée : ce téléphone prévient les autres en arrivant (zone de ${RADIUS_M} m).`
      : "La maison n'est pas encore enregistrée sur ce téléphone.";
    $("home-clear").hidden = !on;
  }

  // Android exige deux étapes : la position, puis « Toujours autoriser ».
  async function ensurePermissions() {
    let status = await invoke("plugin:geofence|check_permissions");
    if (status.location !== "granted") {
      status = await invoke("plugin:geofence|request_permissions", { permissions: ["location", "notifications"] });
      if (status.location !== "granted") {
        throw new Error("La position est nécessaire pour savoir quand tu arrives.");
      }
    }
    if (status.backgroundLocation !== "granted") {
      alertBackground();
      status = await invoke("plugin:geofence|request_permissions", { permissions: ["backgroundLocation"] });
      if (status.backgroundLocation !== "granted") {
        throw new Error("Choisis « Toujours autoriser » pour la position : sinon l'arrivée n'est pas détectée quand l'appli est fermée.");
      }
    }
  }

  function alertBackground() {
    showError("Android va te demander la position « en permanence » : choisis « Toujours autoriser », c'est ce qui permet de détecter ton arrivée appli fermée.");
  }

  async function setup() {
    $("home-phone").hidden = false;
    try {
      const who = await invoke("arrival_person");
      // Pas de choix par défaut : une arrivée au mauvais prénom ferait
      // croire à l'autre que la mauvaise personne arrive.
      if (who) $("home-who").value = who;
    } catch (e) {
      showError(String(e));
    }
    try {
      $("home-ring").checked = localStorage.getItem("home-ring") === "1";
    } catch {}
    showState();

    $("home-who").addEventListener("change", async () => {
      showError(null);
      if (!$("home-who").value) return;
      try {
        await invoke("set_arrival_person", { who: $("home-who").value });
      } catch (e) {
        showError(String(e));
      }
    });

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

    $("home-set").addEventListener("click", async () => {
      const button = $("home-set");
      button.disabled = true;
      showError(null);
      try {
        if (!$("home-who").value) {
          throw new Error("Choisis d'abord à qui est ce téléphone.");
        }
        await ensurePermissions();
        showError(null);
        const pos = await invoke("plugin:geofence|current_position");
        await invoke("plugin:geofence|set_home", { lat: pos.lat, lng: pos.lng, radiusM: RADIUS_M });
        remember(true);
        showState();
      } catch (e) {
        showError(String(e?.message ?? e));
      } finally {
        button.disabled = false;
      }
    });

    $("home-clear").addEventListener("click", async () => {
      showError(null);
      try {
        await invoke("plugin:geofence|clear_home");
        remember(false);
        showState();
      } catch (e) {
        showError(String(e));
      }
    });
  }

  if (isPhone) setup();
  refresh();
  tauri.event?.listen("docs-changed", (event) => {
    if (event.payload?.includes("arrivals")) refresh();
  });
})();
