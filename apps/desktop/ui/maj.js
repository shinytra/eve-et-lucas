// Mise à jour de l'appli depuis l'appli (écran Paramètres, src/update.rs).
// Vérifie à l'ouverture et quand l'appli revient au premier plan (Rust garde
// la réponse 6 heures), ou quand on appuie sur « Rechercher ». Une nouvelle
// version est aussi signalée sur l'accueil.
// PC : téléchargée, vérifiée, installée puis l'appli redémarre.
// Téléphone : téléchargée, vérifiée, puis l'installeur d'Android s'ouvre.

(() => {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) return;
  const $ = (id) => document.getElementById(id);
  const etat = $("maj-etat");
  const notes = $("maj-notes");
  const barre = $("maj-barre");
  const installer = $("maj-installer");
  const chercher = $("maj-chercher");
  const accueil = $("accueil-maj");
  let enCours = false;

  function erreur(message) {
    $("maj-error").textContent = message ?? "";
    $("maj-error").hidden = !message;
  }

  function montrer(status) {
    const dispo = status.available;
    if (!status.supported) {
      etat.textContent = `Version installée : ${status.current}. Pas de mise à jour automatique sur ce système.`;
    } else if (dispo) {
      etat.textContent = `Version installée : ${status.current}. Nouvelle version disponible : ${dispo}.`;
    } else {
      etat.textContent = `Version installée : ${status.current}. L'appli est à jour.`;
    }
    notes.textContent = dispo ? status.notes : "";
    notes.hidden = !dispo || !status.notes;
    installer.hidden = !dispo;
    accueil.hidden = !dispo;
    accueil.textContent = dispo ? `Nouvelle version ${dispo} disponible : appuie ici pour mettre à jour.` : "";
  }

  // Message à garder affiché (installation en cours, autorisation demandée)
  // tant qu'on n'appuie pas sur un bouton.
  let message = false;

  async function verifier(force) {
    if (enCours) return;
    if (!force && message) return;
    message = false;
    if (force) erreur(null);
    chercher.disabled = true;
    try {
      const status = await invoke("check_update", { force });
      // Entre-temps, on a peut-être appuyé sur « Mettre à jour ».
      if (!enCours && !message) montrer(status);
    } catch (e) {
      // Vérification automatique : on ne dérange pas pour un réseau absent.
      if (force) erreur(String(e));
      else console.warn("Vérification des mises à jour impossible :", e);
    } finally {
      if (!enCours) chercher.disabled = false;
    }
  }

  window.__TAURI__.event?.listen("update-progress", ({ payload }) => {
    const total = payload.total;
    barre.hidden = false;
    barre.classList.toggle("inconnu", !total);
    const part = total ? payload.downloaded / total : 1;
    barre.firstElementChild.style.setProperty("--progress", String(Math.min(1, part)));
    const mo = (payload.downloaded / 1048576).toFixed(1);
    etat.textContent = total
      ? `Téléchargement : ${mo} Mo sur ${(total / 1048576).toFixed(1)} Mo…`
      : `Téléchargement : ${mo} Mo…`;
  });

  installer.addEventListener("click", async () => {
    if (enCours) return;
    enCours = true;
    erreur(null);
    installer.disabled = true;
    chercher.disabled = true;
    etat.textContent = "Téléchargement de la nouvelle version…";
    try {
      const resultat = await invoke("install_update");
      message = true;
      if (resultat === "needsPermission") {
        etat.textContent =
          "Autorise « Eve & Lucas » à installer des applis dans le réglage qui vient de s'ouvrir, puis reviens et appuie à nouveau sur « Mettre à jour ».";
      } else {
        // PC : l'appli redémarre. Téléphone : l'installeur d'Android est ouvert.
        etat.textContent =
          "Installation lancée. Sur téléphone, appuie sur « Mettre à jour » dans la fenêtre d'Android.";
      }
    } catch (e) {
      erreur(String(e));
      etat.textContent = "La mise à jour n'a pas pu être installée.";
    } finally {
      enCours = false;
      installer.disabled = false;
      chercher.disabled = false;
      barre.hidden = true;
    }
  });

  chercher.addEventListener("click", () => verifier(true));
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) verifier(false);
  });
  // Laisse l'appli démarrer et se synchroniser d'abord.
  setTimeout(() => verifier(false), 5000);
})();
