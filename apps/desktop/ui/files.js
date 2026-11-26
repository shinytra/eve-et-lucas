// Fichiers partagés : envoi, liste, aperçu des images, téléchargement.
// Le contenu passe en binaire brut par les commandes de src-tauri/src/files.rs.

(() => {
  const tauri = window.__TAURI__;
  if (!tauri?.core?.invoke) return;
  const { invoke } = tauri.core;
  const $ = (id) => document.getElementById(id);
  const list = $("files");
  const input = $("file-input");
  const shareButton = $("file-share");
  const errorBox = $("file-error");
  const statusLine = $("file-status");
  const MAX_SIZE = 500 * 1024 * 1024;

  // Adresses blob: des aperçus, libérées à chaque nouvel affichage.
  let previews = [];

  function showError(message) {
    errorBox.textContent = message ?? "";
    errorBox.hidden = !message;
  }

  function showStatus(message) {
    statusLine.textContent = message ?? "";
    statusLine.hidden = !message;
  }

  function formatSize(bytes) {
    if (bytes < 1024) return `${bytes} o`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} Ko`;
    return `${(bytes / (1024 * 1024)).toFixed(1).replace(".", ",")} Mo`;
  }

  function formatDate(ms) {
    return new Date(ms).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" });
  }

  async function preview(file, li) {
    try {
      const buffer = await invoke("file_content", { id: file.id });
      const url = URL.createObjectURL(new Blob([buffer], { type: file.mime }));
      previews.push(url);
      const img = document.createElement("img");
      img.alt = file.name;
      img.src = url;
      li.insertBefore(img, li.querySelector(".actions"));
    } catch (e) {
      console.error("Aperçu impossible", e);
    }
  }

  function button(label, onClick, className) {
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = label;
    if (className) b.className = className;
    b.addEventListener("click", async () => {
      b.disabled = true;
      showError(null);
      try {
        await onClick();
      } catch (e) {
        showError(String(e));
      } finally {
        b.disabled = false;
      }
    });
    return b;
  }

  function render(files) {
    previews.forEach((url) => URL.revokeObjectURL(url));
    previews = [];
    list.replaceChildren(
      ...files.map((file) => {
        const li = document.createElement("li");
        li.className = "file";

        const name = document.createElement("p");
        name.className = "name";
        name.textContent = file.name;

        const meta = document.createElement("p");
        meta.className = "meta";
        const where = file.available ? "" : " · pas encore sur cet appareil";
        meta.textContent = `${file.author} · ${formatSize(file.size)} · ${formatDate(file.added_at)}${where}`;

        const actions = document.createElement("div");
        actions.className = "actions";
        if (file.available) {
          actions.append(
            button("Enregistrer", async () => {
              const path = await invoke("save_file", { id: file.id });
              showStatus(`Enregistré : ${path}`);
            }),
          );
        } else {
          actions.append(
            button("Télécharger", async () => {
              showStatus(`Téléchargement de « ${file.name} »…`);
              try {
                await invoke("download_file", { id: file.id });
                showStatus(null);
              } catch (e) {
                showStatus(null);
                throw e;
              }
              await refresh();
            }),
          );
        }
        actions.append(
          button(
            "Supprimer",
            async () => {
              await invoke("delete_file", { id: file.id });
              await refresh();
            },
            "danger",
          ),
        );

        li.append(name, meta, actions);
        if (file.available && file.is_image) preview(file, li);
        return li;
      }),
    );
    $("files-empty").hidden = files.length > 0;
  }

  async function refresh() {
    try {
      render(await invoke("list_files"));
    } catch (e) {
      showError(String(e));
    }
  }

  async function share(file) {
    if (file.size > MAX_SIZE) {
      throw new Error(`« ${file.name} » dépasse 500 Mo.`);
    }
    const bytes = new Uint8Array(await file.arrayBuffer());
    await invoke("share_file", bytes, {
      headers: {
        "x-name": encodeURIComponent(file.name),
        "x-mime": encodeURIComponent(file.type || ""),
        "x-author": encodeURIComponent($("author").value),
      },
    });
  }

  shareButton.addEventListener("click", () => input.click());
  input.addEventListener("change", async () => {
    const chosen = [...input.files];
    input.value = "";
    if (!chosen.length) return;
    showError(null);
    shareButton.disabled = true;
    try {
      for (const [i, file] of chosen.entries()) {
        showStatus(`Envoi de « ${file.name} » (${i + 1}/${chosen.length})…`);
        await share(file);
      }
      showStatus(null);
    } catch (e) {
      showStatus(null);
      showError(String(e?.message ?? e));
    } finally {
      shareButton.disabled = false;
    }
    await refresh();
  });

  refresh();
  tauri.event?.listen("docs-changed", (event) => {
    if (event.payload?.includes("files")) refresh();
  });
})();
