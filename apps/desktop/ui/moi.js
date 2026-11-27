// Qui utilise cet appareil : Lucas ou Eve.
// Demandé au premier lancement (fenêtre « Bienvenue »), modifiable ensuite
// dans l'écran Paramètres. Le choix est enregistré côté Rust
// (`arrivee.json`, commandes `arrival_person` / `set_arrival_person`) : il
// signe les ajouts, nomme les arrivées à la maison et les cases cochées sur
// le widget, et écarte ses propres notifications sur le téléphone.
// Les autres écrans lisent le prénom dans `#author` et écoutent son
// évènement `change`.

(() => {
  const $ = (id) => document.getElementById(id);
  const author = $("author");
  const pastille = $("moi-pastille");
  const bienvenue = $("bienvenue");
  const PRENOMS = ["Lucas", "Eve"];
  const CACHE_KEY = "author";

  // Affiche le prénom partout (sans prévenir les autres écrans).
  function montrer(prenom) {
    author.value = prenom;
    pastille.textContent = prenom ? prenom.charAt(0) : "?";
    pastille.title = `Je suis ${prenom}`;
    pastille.setAttribute("aria-label", `Paramètres : je suis ${prenom}`);
    document.querySelectorAll(".choix-moi [data-moi]").forEach((b) => {
      b.setAttribute("aria-checked", b.dataset.moi === prenom ? "true" : "false");
    });
  }

  // Copie locale : le bon prénom s'affiche avant la réponse de Rust.
  function cache(prenom) {
    try {
      localStorage.setItem(CACHE_KEY, prenom);
    } catch {}
  }

  function erreur(id, message) {
    const box = $(id);
    box.textContent = message ?? "";
    box.hidden = !message;
  }

  // Prénom inconnu (premier lancement) : #author reste vide, l'accueil ne
  // salue personne et la fenêtre de bienvenue empêche tout ajout.
  let enCache = "";
  try {
    const saved = localStorage.getItem(CACHE_KEY);
    if (PRENOMS.includes(saved)) enCache = saved;
  } catch {}
  montrer(enCache);

  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) return;

  // Un choix fait ici l'emporte sur la première lecture, si elle répond après.
  let choisi = false;

  async function choisir(prenom, boiteErreur) {
    erreur(boiteErreur, null);
    choisi = true;
    try {
      await invoke("set_arrival_person", { who: prenom });
    } catch (e) {
      erreur(boiteErreur, String(e));
      return false;
    }
    montrer(prenom);
    cache(prenom);
    // Toujours prévenu : même prénom qu'avant, mais il est désormais enregistré.
    author.dispatchEvent(new Event("change"));
    return true;
  }

  // Le reste de l'appli, inaccessible (souris et clavier) tant qu'on n'a pas choisi.
  const arriere = () => document.querySelectorAll("body > :not(#bienvenue):not(script)");

  function ouvrirBienvenue() {
    // Rien de présélectionné : c'est à la personne de dire qui elle est.
    montrer("");
    author.dispatchEvent(new Event("change"));
    arriere().forEach((e) => (e.inert = true));
    bienvenue.hidden = false;
    document.body.classList.add("bienvenue-ouverte");
    bienvenue.querySelector(".bienvenue-corps")?.focus({ preventScroll: true });
  }

  function fermerBienvenue() {
    bienvenue.classList.add("sortie");
    const fin = () => {
      bienvenue.hidden = true;
      bienvenue.classList.remove("sortie");
      arriere().forEach((e) => (e.inert = false));
      document.body.classList.remove("bienvenue-ouverte");
    };
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) fin();
    else setTimeout(fin, 260);
  }

  bienvenue.querySelectorAll("[data-moi]").forEach((b) => {
    b.addEventListener("click", async () => {
      if (await choisir(b.dataset.moi, "bienvenue-error")) fermerBienvenue();
    });
  });
  // Pas de fermeture par Échap : sans choix, rien ne serait signé au bon nom.
  bienvenue.addEventListener("keydown", (event) => {
    if (event.key === "Escape") event.stopPropagation();
  });

  $("ecran-parametres")
    .querySelectorAll("[data-moi]")
    .forEach((b) => b.addEventListener("click", () => choisir(b.dataset.moi, "moi-error")));

  // Le bouton de l'en-tête ouvre ou ferme : on ne le presse que si c'est fermé.
  $("params-apparence").addEventListener("click", () => {
    if ($("settings").hidden) $("open-settings").click();
  });

  // Le choix enregistré par Rust fait foi ; s'il n'y en a pas encore, c'est
  // le premier lancement (ou une version d'avant ce réglage) : on demande.
  invoke("arrival_person")
    .then((who) => {
      if (choisi) return;
      const prenom = PRENOMS.find((p) => p.toLowerCase() === String(who).trim().toLowerCase());
      // Pas encore de prénom, ou une copie locale qui dit autre chose (ancien
      // sélecteur « Je suis » différent du choix d'« Arrivée à la maison ») :
      // on demande plutôt que de changer de prénom en silence.
      if (!prenom || (enCache && enCache !== prenom)) {
        ouvrirBienvenue();
        return;
      }
      const change = author.value !== prenom;
      montrer(prenom);
      cache(prenom);
      if (change) author.dispatchEvent(new Event("change"));
    })
    .catch((e) => {
      // Sans réponse, on garde la copie locale ; sans elle, on demande.
      console.error("Prénom de l'appareil indisponible :", e);
      if (!choisi && !enCache) ouvrirBienvenue();
    });
})();
