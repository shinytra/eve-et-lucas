// Widget photo : images et GIF choisis pour le widget « Photo » du téléphone
// (document partagé `cadres`, commandes de src-tauri/src/photos.rs). Les
// images s'ajoutent depuis la liste des fichiers (bouton cadre de files.js).

(() => {
  const tauri = window.__TAURI__;
  if (!tauri?.core?.invoke) return;
  const { invoke } = tauri.core;
  const $ = (id) => document.getElementById(id);
  const list = $("cadres");
  const errorBox = $("cadres-error");
  const FITS = { remplir: "Remplir le widget", entiere: "Image entière" };

  // Aperçus déjà lus, par empreinte : la liste est redessinée à chaque
  // fichier reçu, sans relire les images.
  const previews = new Map();

  function showError(message) {
    errorBox.textContent = message ?? "";
    errorBox.hidden = !message;
  }

  async function preview(cadre, li) {
    try {
      let url = previews.get(cadre.file.hash);
      if (!url) {
        const buffer = await invoke("file_content", { id: cadre.file.id });
        url = URL.createObjectURL(new Blob([buffer], { type: cadre.file.mime }));
        previews.set(cadre.file.hash, url);
      }
      const img = document.createElement("img");
      img.alt = "";
      img.src = url;
      img.className = `vignette ${cadre.fit}`;
      li.prepend(img);
    } catch (e) {
      console.error("Aperçu impossible", e);
    }
  }

  function render(cadres) {
    const used = new Set(cadres.map((c) => c.file.hash));
    for (const [hash, url] of previews) {
      if (!used.has(hash)) {
        URL.revokeObjectURL(url);
        previews.delete(hash);
      }
    }
    list.replaceChildren(
      ...cadres.map((cadre) => {
        const li = document.createElement("li");
        li.className = "cadre";

        const texte = document.createElement("div");
        texte.className = "texte";
        const name = document.createElement("p");
        name.className = "name";
        name.textContent = cadre.file.name;
        const meta = document.createElement("p");
        meta.className = "meta";
        const where = cadre.available ? "" : " · pas encore sur cet appareil";
        meta.textContent = `Choisie par ${cadre.added_by}${where}`;
        texte.append(name, meta);

        const fit = document.createElement("select");
        fit.setAttribute("aria-label", `Affichage de « ${cadre.file.name} »`);
        for (const [value, label] of Object.entries(FITS)) {
          const option = document.createElement("option");
          option.value = value;
          option.textContent = label;
          fit.append(option);
        }
        fit.value = cadre.fit;
        fit.addEventListener("change", async () => {
          fit.disabled = true;
          showError(null);
          try {
            await invoke("set_cadre_fit", { id: cadre.id, fit: fit.value });
          } catch (e) {
            fit.value = cadre.fit;
            showError(String(e));
          } finally {
            fit.disabled = false;
          }
        });

        const remove = window.__ui.iconButton("supprimer", `Retirer « ${cadre.file.name} » du widget`, "danger");
        remove.addEventListener("click", async () => {
          remove.disabled = true;
          showError(null);
          await window.__ui.sortie(li);
          try {
            await invoke("remove_cadre", { id: cadre.id });
          } catch (e) {
            li.classList.remove("sortie");
            remove.disabled = false;
            showError(String(e));
          }
        });

        const actions = document.createElement("div");
        actions.className = "actions";
        actions.append(fit, remove);
        li.append(texte, actions);
        if (cadre.available) preview(cadre, li);
        return li;
      }),
    );
    $("cadres-empty").hidden = cadres.length > 0;
  }

  async function refresh() {
    try {
      render(await invoke("list_cadres"));
    } catch (e) {
      showError(String(e));
    }
  }

  refresh();
  tauri.event?.listen("docs-changed", (event) => {
    if (event.payload?.includes("cadres") || event.payload?.includes("files")) refresh();
  });
})();
