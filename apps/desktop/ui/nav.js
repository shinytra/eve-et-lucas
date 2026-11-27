// Navigation entre les écrans et petits outils communs de l'interface.
// Un écran à la fois : `section[data-ecran]` dans <main>, choisi par l'adresse
// (#taches, #dates…). Le bouton retour d'Android revient donc à l'écran
// précédent. Barre d'onglets en bas sur téléphone, menu latéral sur PC
// (mêmes liens, la mise en page change dans style.css).

(() => {
  const $ = (id) => document.getElementById(id);
  const NS = "http://www.w3.org/2000/svg";
  const ICONES = {
    modifier: "M4 17.3V20h2.7l8-8-2.7-2.7ZM19.7 7a1 1 0 0 0 0-1.4l-1.3-1.3a1 1 0 0 0-1.4 0l-1.3 1.3 2.7 2.7Z",
    supprimer: "M9 3h6l1 1h4v2H4V4h4ZM6 8h12l-1 13H7Zm4 2v9h1.5v-9Zm2.5 0v9H14v-9Z",
    telecharger: "M11 4h2v8.2l3.3-3.3 1.4 1.4-5.7 5.7-5.7-5.7 1.4-1.4 3.3 3.3ZM5 18h14v2H5Z",
    cadre: "M21 19V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2ZM8.5 13.5l2.5 3 3.5-4.5 4.5 6H5Z",
    enregistrer: "M5 3h11l3 3v13a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2Zm2 2v4h8V5Zm5 8a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z",
  };

  const ecrans = [...document.querySelectorAll("main > section[data-ecran]")];
  const liens = [...document.querySelectorAll("#nav a[data-ecran]")];
  // L'ordre du menu donne le sens de la transition entre deux écrans.
  const ordre = liens.map((l) => l.dataset.ecran).filter((nom) => ecrans.some((s) => s.dataset.ecran === nom));
  const indicateur = document.querySelector(".nav-indicateur");
  const titre = $("titre-ecran");
  const plus = $("plus");
  const boutonPlus = $("nav-plus");
  const fab = $("fab");
  const telephone = window.matchMedia("(max-width: 899px)");
  let actuel = null;

  // Bouton rond avec une icône ; le libellé sert aux lecteurs d'écran et
  // à l'infobulle.
  function iconButton(icone, label, className) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = `icone petite ${className ?? ""}`.trim();
    b.setAttribute("aria-label", label);
    b.title = label;
    const svg = document.createElementNS(NS, "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("width", "18");
    svg.setAttribute("height", "18");
    svg.setAttribute("aria-hidden", "true");
    const path = document.createElementNS(NS, "path");
    path.setAttribute("fill", "currentColor");
    path.setAttribute("d", ICONES[icone] ?? "");
    svg.append(path);
    b.append(svg);
    return b;
  }

  // Fait disparaître un élément de liste avant sa suppression.
  function sortie(element) {
    if (!element || reduit()) return Promise.resolve();
    element.classList.add("sortie");
    return new Promise((resolve) => setTimeout(resolve, 220));
  }

  const reduit = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // ---------- Formulaires repliables ----------

  function boutonDe(form) {
    return document.querySelector(`.ajouter[data-form="${form.id}"]`);
  }

  function ouvrir(form) {
    if (!form) return;
    form.classList.add("ouvert");
    boutonDe(form)?.setAttribute("aria-expanded", "true");
    majFab();
  }

  function fermer(form) {
    if (!form) return;
    form.classList.remove("ouvert");
    boutonDe(form)?.setAttribute("aria-expanded", "false");
    majFab();
  }

  document.querySelectorAll(".ajouter[data-form]").forEach((bouton) => {
    const form = $(bouton.dataset.form);
    bouton.addEventListener("click", () => {
      if (form.classList.contains("ouvert")) {
        // Le bouton « Annuler » du formulaire remet aussi le mode édition à zéro.
        form.querySelector("button[type=button]")?.click();
        fermer(form);
      } else {
        ouvrir(form);
        form.querySelector("input, textarea")?.focus({ preventScroll: true });
        form.scrollIntoView({ block: "nearest", behavior: reduit() ? "auto" : "smooth" });
      }
    });
  });

  // ---------- Écrans ----------

  function nomValide(nom) {
    return ordre.includes(nom) ? nom : "accueil";
  }

  function afficher(nom) {
    nom = nomValide(nom);
    if (nom === actuel) return;
    const avant = ordre.indexOf(actuel);
    const apres = ordre.indexOf(nom);
    document.body.style.setProperty("--sens", avant < 0 || apres >= avant ? "1" : "-1");
    for (const section of ecrans) {
      const visible = section.dataset.ecran === nom;
      section.classList.toggle("actif", visible);
      if (visible) entree(section);
    }
    for (const lien of liens) {
      if (lien.dataset.ecran === nom) lien.setAttribute("aria-current", "page");
      else lien.removeAttribute("aria-current");
    }
    actuel = nom;
    majTitre();
    fermerPlus();
    majIndicateur();
    majFab();
    window.scrollTo({ top: 0 });
    document.dispatchEvent(new CustomEvent("ecran", { detail: nom }));
  }

  function majTitre() {
    const section = ecrans.find((s) => s.dataset.ecran === actuel);
    const h2 = actuel === "accueil" ? null : section?.querySelector("h2");
    // Sur PC, le nom de l'appli est déjà en haut du menu.
    titre.textContent = h2 ? h2.textContent : telephone.matches ? "Eve & Lucas" : "Accueil";
    document.title = h2 ? `${h2.textContent} · Eve & Lucas` : "Eve & Lucas";
  }

  // Animation d'entrée : l'écran glisse, ses éléments arrivent en cascade.
  function entree(section) {
    section.classList.remove("entree");
    section.querySelectorAll("li").forEach((li, i) => li.style.setProperty("--i", String(Math.min(i, 12))));
    // Relance l'animation même si la classe vient d'être retirée.
    void section.offsetWidth;
    section.classList.add("entree");
    clearTimeout(section._finEntree);
    section._finEntree = setTimeout(() => section.classList.remove("entree"), 900);
  }

  // Pastille qui glisse sous l'onglet actif. Sur téléphone, un écran sans
  // onglet (Fichiers, Arrivée, Appareils) allume l'onglet « Plus ».
  function majIndicateur() {
    if (!indicateur) return;
    let cible = liens.find((l) => l.dataset.ecran === actuel);
    if (cible && getComputedStyle(cible).display === "none") cible = boutonPlus;
    boutonPlus.classList.toggle("actif", cible === boutonPlus);
    if (!cible) {
      indicateur.style.opacity = "0";
      return;
    }
    const parent = indicateur.parentElement.getBoundingClientRect();
    const r = cible.getBoundingClientRect();
    indicateur.style.opacity = "1";
    indicateur.style.width = `${r.width}px`;
    indicateur.style.height = `${r.height}px`;
    indicateur.style.transform = `translate(${r.left - parent.left}px, ${r.top - parent.top}px)`;
  }

  // Le « + » flottant du téléphone ajoute sur l'écran affiché.
  function majFab() {
    const section = ecrans.find((s) => s.dataset.ecran === actuel);
    const bouton = section?.querySelector(".ajouter");
    const form = bouton?.dataset.form ? $(bouton.dataset.form) : null;
    // Un bouton dans un bloc caché (lieux : téléphone seulement) ne compte pas.
    fab.hidden = !bouton || !!bouton.closest("[hidden]") || !!form?.classList.contains("ouvert");
    if (bouton) fab.setAttribute("aria-label", bouton.textContent.trim());
  }

  fab.addEventListener("click", () => {
    ecrans.find((s) => s.dataset.ecran === actuel)?.querySelector(".ajouter")?.click();
  });

  // ---------- Feuille « Plus » (téléphone) ----------

  function ouvrirPlus() {
    plus.hidden = false;
    boutonPlus.setAttribute("aria-expanded", "true");
    plus.querySelector(".tuile")?.focus({ preventScroll: true });
  }

  function fermerPlus() {
    if (plus.hidden) return;
    plus.hidden = true;
    boutonPlus.setAttribute("aria-expanded", "false");
  }

  boutonPlus.addEventListener("click", () => (plus.hidden ? ouvrirPlus() : fermerPlus()));
  plus.querySelector(".feuille-fond").addEventListener("click", fermerPlus);
  plus.querySelectorAll("a.tuile").forEach((a) => a.addEventListener("click", fermerPlus));
  $("plus-apparence").addEventListener("click", () => {
    fermerPlus();
    $("open-settings").click();
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") fermerPlus();
  });

  window.addEventListener("hashchange", () => {
    const nom = nomValide(location.hash.slice(1));
    if (location.hash.slice(1) !== nom) history.replaceState(null, "", `#${nom}`);
    afficher(nom);
  });
  window.addEventListener("resize", majIndicateur);
  telephone.addEventListener?.("change", () => {
    fermerPlus();
    majTitre();
    majIndicateur();
  });
  // Les polices peuvent changer la largeur des onglets après le chargement.
  document.fonts?.ready.then(majIndicateur);
  new ResizeObserver(majIndicateur).observe($("nav"));

  window.__ui = { iconButton, sortie, ouvrir, fermer, afficher, ecran: () => actuel };

  const depart = nomValide(location.hash.slice(1));
  if (location.hash.slice(1) !== depart) history.replaceState(null, "", `#${depart}`);
  afficher(depart);
})();
