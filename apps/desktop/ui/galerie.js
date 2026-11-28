// Galerie photos : photos en vrac ou rangées dans des albums (document
// partagé `galerie`, commandes de src-tauri/src/galerie.rs). Le contenu des
// photos est celui des fichiers partagés : même envoi par morceaux que files.js.

(() => {
  const tauri = window.__TAURI__;
  if (!tauri?.core?.invoke) return;
  const { invoke } = tauri.core;
  const $ = (id) => document.getElementById(id);
  const grille = $("galerie-grille");
  const filtres = $("galerie-filtres");
  const input = $("galerie-input");
  const ajout = $("galerie-ajout");
  const errorBox = $("galerie-error");
  const statusLine = $("galerie-status");
  const MAX_SIZE = 20 * 1024 * 1024;
  const VRAC = "vrac";
  const TOUT = "tout";

  let albums = [];
  let photos = [];
  // « tout », « vrac » ou l'identifiant d'un album.
  let filtre = TOUT;
  // Édition du nom d'un album : null (rien), "nouveau" ou l'identifiant.
  let edition = null;
  // Vignettes déjà préparées, par empreinte (la grille est redessinée à
  // chaque changement sans relire les images).
  const vignettes = new Map();
  let ouverte = null; // photo affichée dans la visionneuse
  let urlOuverte = null;

  function showError(message) {
    errorBox.textContent = message ?? "";
    errorBox.hidden = !message;
  }

  function showStatus(message) {
    statusLine.textContent = message ?? "";
    statusLine.hidden = !message;
  }

  const formatDate = (ms) => new Date(ms).toLocaleString("fr-FR", { dateStyle: "medium", timeStyle: "short" });
  const albumDe = (id) => albums.find((a) => a.id === id);

  function visibles() {
    if (filtre === TOUT) return photos;
    if (filtre === VRAC) return photos.filter((p) => !p.album_id);
    return photos.filter((p) => p.album_id === filtre);
  }

  // ---------- Vignettes (chargées au fur et à mesure du défilement) ----------

  const file = [];
  let enCours = 0;
  const MAX_PARALLELE = 3;

  function pump() {
    while (enCours < MAX_PARALLELE && file.length) {
      const tache = file.shift();
      enCours += 1;
      tache().finally(() => {
        enCours -= 1;
        pump();
      });
    }
  }

  async function fabriquer(photo) {
    const buffer = await invoke("file_content", { id: photo.file.id });
    const blob = new Blob([buffer], { type: photo.file.mime });
    try {
      const bitmap = await createImageBitmap(blob);
      const echelle = Math.min(1, 360 / Math.max(bitmap.width, bitmap.height));
      const canvas = document.createElement("canvas");
      canvas.width = Math.max(1, Math.round(bitmap.width * echelle));
      canvas.height = Math.max(1, Math.round(bitmap.height * echelle));
      canvas.getContext("2d").drawImage(bitmap, 0, 0, canvas.width, canvas.height);
      bitmap.close?.();
      const petit = await new Promise((resolve) => canvas.toBlob(resolve, "image/jpeg", 0.8));
      if (petit) return URL.createObjectURL(petit);
    } catch (e) {
      console.error("Vignette impossible, image entière utilisée", e);
    }
    return URL.createObjectURL(blob);
  }

  const observateur = new IntersectionObserver(
    (entrees) => {
      for (const entree of entrees) {
        if (!entree.isIntersecting) continue;
        observateur.unobserve(entree.target);
        const { hash, photoId } = entree.target.dataset;
        const photo = photos.find((p) => p.id === photoId);
        if (!photo || vignettes.has(hash)) continue;
        const cible = entree.target;
        file.push(async () => {
          try {
            const url = await fabriquer(photo);
            vignettes.set(hash, url);
            if (cible.isConnected) cible.src = url;
          } catch (e) {
            console.error("Aperçu impossible", e);
          }
        });
        pump();
      }
    },
    { rootMargin: "200px" },
  );

  function libererVignettes() {
    const utilisees = new Set(photos.map((p) => p.file.hash));
    for (const [hash, url] of vignettes) {
      if (!utilisees.has(hash)) {
        URL.revokeObjectURL(url);
        vignettes.delete(hash);
      }
    }
  }

  // ---------- Affichage ----------

  function chip(label, valeur, compte) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "chip";
    b.setAttribute("aria-pressed", String(filtre === valeur));
    b.textContent = compte === undefined ? label : `${label} · ${compte}`;
    b.addEventListener("click", () => {
      filtre = valeur;
      fermerEdition();
      render();
    });
    return b;
  }

  function renderFiltres() {
    const nouveau = document.createElement("button");
    nouveau.type = "button";
    nouveau.className = "chip chip-nouveau";
    nouveau.textContent = "+ Album";
    nouveau.addEventListener("click", () => ouvrirEdition("nouveau"));
    filtres.replaceChildren(
      chip("Tout", TOUT, photos.length),
      chip("En vrac", VRAC, photos.filter((p) => !p.album_id).length),
      ...albums.map((a) => chip(a.name, a.id, photos.filter((p) => p.album_id === a.id).length)),
      nouveau,
    );
  }

  function renderAlbumActions() {
    const bloc = $("album-actions");
    const album = albumDe(filtre);
    bloc.hidden = !album;
    if (!album) return;
    $("album-titre").textContent = `Album créé par ${album.created_by}`;
    const modifier = window.__ui.iconButton("modifier", `Renommer l'album « ${album.name} »`);
    modifier.addEventListener("click", () => ouvrirEdition(album.id));
    const supprimer = window.__ui.iconButton("supprimer", `Supprimer l'album « ${album.name} »`, "danger");
    supprimer.addEventListener("click", async () => {
      // Les photos ne sont pas supprimées : elles repassent « en vrac ».
      if (!supprimer.dataset.confirme) {
        supprimer.dataset.confirme = "1";
        showStatus(`Touche encore la corbeille pour supprimer « ${album.name} » (ses photos restent, en vrac).`);
        setTimeout(() => {
          delete supprimer.dataset.confirme;
          showStatus(null);
        }, 5000);
        return;
      }
      showError(null);
      showStatus(null);
      try {
        await invoke("remove_album", { id: album.id });
        filtre = TOUT;
        await refresh();
      } catch (e) {
        showError(String(e));
      }
    });
    $("album-boutons").replaceChildren(modifier, supprimer);
  }

  function renderGrille() {
    const liste = visibles();
    grille.replaceChildren(
      ...liste.map((photo) => {
        const li = document.createElement("li");
        li.className = "galerie-photo";
        const bouton = document.createElement("button");
        bouton.type = "button";
        bouton.setAttribute("aria-label", `Ouvrir « ${photo.file.name} »`);
        const img = document.createElement("img");
        img.alt = "";
        img.decoding = "async";
        const url = vignettes.get(photo.file.hash);
        if (url) img.src = url;
        else if (photo.available) {
          img.dataset.hash = photo.file.hash;
          img.dataset.photoId = photo.id;
          observateur.observe(img);
        } else {
          bouton.classList.add("absente");
          bouton.dataset.texte = "Pas encore sur cet appareil";
        }
        bouton.append(img);
        bouton.addEventListener("click", () => ouvrir(photo.id));
        li.append(bouton);
        return li;
      }),
    );
    const vide = $("galerie-empty");
    vide.hidden = liste.length > 0;
    if (!liste.length) {
      vide.textContent =
        filtre === TOUT
          ? "Aucune photo pour l'instant : touche « Ajouter des photos »."
          : filtre === VRAC
            ? "Aucune photo en vrac."
            : "Cet album est vide : ajoute des photos ici, ou range-en depuis « Tout ».";
    }
  }

  function render() {
    if (filtre !== TOUT && filtre !== VRAC && !albumDe(filtre)) filtre = TOUT;
    renderFiltres();
    renderAlbumActions();
    renderGrille();
    if (ouverte) majVisionneuse();
  }

  async function refresh() {
    try {
      const data = await invoke("list_galerie");
      albums = data.albums;
      photos = data.photos;
      libererVignettes();
      render();
    } catch (e) {
      showError(String(e));
    }
  }

  // ---------- Albums : création et renommage ----------

  const form = $("album-form");

  function ouvrirEdition(quoi) {
    edition = quoi;
    form.hidden = false;
    $("album-valider").textContent = quoi === "nouveau" ? "Créer" : "Renommer";
    $("album-nom").value = quoi === "nouveau" ? "" : (albumDe(quoi)?.name ?? "");
    $("album-nom").focus({ preventScroll: true });
  }

  function fermerEdition() {
    edition = null;
    form.hidden = true;
    $("album-nom").value = "";
  }

  $("album-annuler").addEventListener("click", fermerEdition);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    showError(null);
    try {
      const name = $("album-nom").value;
      if (edition === "nouveau") {
        const album = await invoke("add_album", { author: $("author").value, name });
        filtre = album.id;
      } else if (edition) {
        await invoke("rename_album", { id: edition, name });
      }
      fermerEdition();
      await refresh();
    } catch (e) {
      showError(String(e));
    }
  });

  // ---------- Ajout de photos ----------

  const CHUNK = 2 * 1024 * 1024;

  function toBase64(bytes) {
    let binary = "";
    for (let i = 0; i < bytes.length; i += 0x8000) {
      binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    }
    return btoa(binary);
  }

  // Envoie l'image comme un fichier partagé, puis la met dans la galerie.
  async function envoyer(fichier, albumId, onProgress) {
    if (!/^image\/(png|jpeg|gif|webp)$/.test(fichier.type)) {
      throw new Error(`« ${fichier.name} » n'est pas une image (PNG, JPEG, GIF ou WebP).`);
    }
    if (fichier.size > MAX_SIZE) {
      throw new Error(`« ${fichier.name} » dépasse 20 Mo.`);
    }
    const id = await invoke("upload_start", {
      name: fichier.name,
      mime: fichier.type,
      author: $("author").value,
      size: fichier.size,
    });
    let partage;
    try {
      for (let offset = 0; offset < fichier.size; offset += CHUNK) {
        const bytes = new Uint8Array(await fichier.slice(offset, offset + CHUNK).arrayBuffer());
        await invoke("upload_chunk", { id, data: toBase64(bytes) });
        onProgress(Math.min(1, (offset + CHUNK) / fichier.size));
      }
      partage = await invoke("upload_finish", { id });
    } catch (e) {
      invoke("upload_cancel", { id }).catch(() => {});
      throw e;
    }
    try {
      await invoke("add_photo", { author: $("author").value, fileId: partage.id, albumId });
    } catch (e) {
      // Pas de fichier orphelin si la galerie refuse la photo.
      invoke("delete_file", { id: partage.id }).catch(() => {});
      throw e;
    }
  }

  ajout.addEventListener("click", () => input.click());
  input.addEventListener("change", async () => {
    const choisis = [...input.files];
    input.value = "";
    if (!choisis.length) return;
    showError(null);
    ajout.disabled = true;
    // Dans un album ouvert, les photos y vont ; sinon elles sont en vrac.
    const albumId = albumDe(filtre) ? filtre : "";
    const erreurs = [];
    for (const [i, fichier] of choisis.entries()) {
      const label = `Photo ${i + 1}/${choisis.length}`;
      showStatus(`${label}…`);
      try {
        await envoyer(fichier, albumId, (p) => showStatus(`${label} : ${Math.round(p * 100)} %`));
      } catch (e) {
        erreurs.push(String(e?.message ?? e));
      }
    }
    showStatus(null);
    if (erreurs.length) showError(erreurs.join(" "));
    ajout.disabled = false;
    await refresh();
  });

  // ---------- Visionneuse ----------

  const vis = $("visionneuse");

  function libererOuverte() {
    if (urlOuverte) URL.revokeObjectURL(urlOuverte);
    urlOuverte = null;
    $("vis-img").removeAttribute("src");
  }

  function ouvrir(photoId) {
    ouverte = photoId;
    vis.hidden = false;
    document.body.classList.add("visionneuse-ouverte");
    $("vis-fermer").focus({ preventScroll: true });
    majVisionneuse();
  }

  function fermerVisionneuse() {
    ouverte = null;
    vis.hidden = true;
    document.body.classList.remove("visionneuse-ouverte");
    libererOuverte();
  }

  let chargement = 0;

  async function majVisionneuse() {
    const liste = visibles();
    const index = liste.findIndex((p) => p.id === ouverte);
    if (index < 0) {
      fermerVisionneuse();
      return;
    }
    const photo = liste[index];
    $("vis-prec").disabled = index === 0;
    $("vis-suiv").disabled = index === liste.length - 1;
    $("vis-meta").textContent = `${photo.file.name} · ${photo.added_by} · ${formatDate(photo.added_at)}`;
    $("vis-error").hidden = true;

    const select = $("vis-album");
    select.replaceChildren(
      new Option("En vrac", ""),
      ...albums.map((a) => new Option(a.name, a.id)),
    );
    select.value = photo.album_id;
    select.onchange = async () => {
      try {
        await invoke("move_photo", { id: photo.id, albumId: select.value });
        await refresh();
      } catch (e) {
        select.value = photo.album_id;
        erreurVis(e);
      }
    };

    const boutons = [];
    if (!photo.available) {
      const dl = window.__ui.iconButton("telecharger", "Télécharger la photo");
      dl.addEventListener("click", async () => {
        dl.disabled = true;
        try {
          await invoke("download_file", { id: photo.file.id });
          await refresh();
        } catch (e) {
          erreurVis(e);
        } finally {
          dl.disabled = false;
        }
      });
      boutons.push(dl);
    }
    const supprimer = window.__ui.iconButton("supprimer", "Supprimer la photo pour tout le monde", "danger");
    supprimer.addEventListener("click", async () => {
      if (!supprimer.dataset.confirme) {
        supprimer.dataset.confirme = "1";
        erreurVis("Touche encore la corbeille : la photo sera supprimée chez vous deux.");
        return;
      }
      supprimer.disabled = true;
      try {
        await invoke("remove_photo", { id: photo.id });
        await invoke("delete_file", { id: photo.file.id }).catch((e) => console.error(e));
        await refresh();
      } catch (e) {
        supprimer.disabled = false;
        erreurVis(e);
      }
    });
    boutons.push(supprimer);
    $("vis-boutons").replaceChildren(...boutons);

    // Image entière, lue seulement à l'ouverture.
    const moi = ++chargement;
    libererOuverte();
    const attente = $("vis-attente");
    if (!photo.available) {
      attente.textContent = "Cette photo n'est pas encore sur cet appareil.";
      attente.hidden = false;
      return;
    }
    attente.hidden = true;
    try {
      const buffer = await invoke("file_content", { id: photo.file.id });
      if (moi !== chargement) return;
      urlOuverte = URL.createObjectURL(new Blob([buffer], { type: photo.file.mime }));
      $("vis-img").alt = photo.file.name;
      $("vis-img").src = urlOuverte;
    } catch (e) {
      if (moi === chargement) erreurVis(e);
    }
  }

  function erreurVis(e) {
    const box = $("vis-error");
    box.textContent = String(e?.message ?? e);
    box.hidden = false;
  }

  function voisine(sens) {
    const liste = visibles();
    const index = liste.findIndex((p) => p.id === ouverte);
    const suivante = liste[index + sens];
    if (suivante) {
      ouverte = suivante.id;
      majVisionneuse();
    }
  }

  $("vis-fermer").addEventListener("click", fermerVisionneuse);
  vis.querySelector(".visionneuse-fond").addEventListener("click", fermerVisionneuse);
  $("vis-prec").addEventListener("click", () => voisine(-1));
  $("vis-suiv").addEventListener("click", () => voisine(1));
  document.addEventListener("keydown", (event) => {
    if (vis.hidden) return;
    if (event.key === "Escape") fermerVisionneuse();
    else if (event.key === "ArrowLeft") voisine(-1);
    else if (event.key === "ArrowRight") voisine(1);
  });

  refresh();
  tauri.event?.listen("docs-changed", (event) => {
    if (event.payload?.includes("galerie") || event.payload?.includes("files")) refresh();
  });
})();
