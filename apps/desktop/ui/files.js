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
  // L'enregistrement dans Téléchargements n'existe pas encore sur téléphone.
  const isPhone = /Android|iPhone|iPad/i.test(navigator.userAgent);

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
        if (file.available && !isPhone) {
          actions.append(
            button("Enregistrer", async () => {
              const path = await invoke("save_file", { id: file.id });
              showStatus(`Enregistré : ${path}`);
            }),
          );
        } else if (!file.available) {
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

  // Par morceaux en base64 : Android ne sait pas passer de binaire brut à Rust.
  const CHUNK = 2 * 1024 * 1024;

  function toBase64(bytes) {
    let binary = "";
    for (let i = 0; i < bytes.length; i += 0x8000) {
      binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    }
    return btoa(binary);
  }

  async function share(file, onProgress) {
    if (file.size > MAX_SIZE) {
      throw new Error(`« ${file.name} » dépasse 500 Mo.`);
    }
    const id = await invoke("upload_start", {
      name: file.name,
      mime: file.type || "",
      author: $("author").value,
      size: file.size,
    });
    try {
      for (let offset = 0; offset < file.size; offset += CHUNK) {
        const bytes = new Uint8Array(await file.slice(offset, offset + CHUNK).arrayBuffer());
        await invoke("upload_chunk", { id, data: toBase64(bytes) });
        onProgress(Math.min(1, (offset + CHUNK) / file.size));
      }
      await invoke("upload_finish", { id });
    } catch (e) {
      invoke("upload_cancel", { id }).catch(() => {});
      throw e;
    }
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
        const label = `Envoi de « ${file.name} » (${i + 1}/${chosen.length})`;
        showStatus(`${label}…`);
        await share(file, (p) => showStatus(`${label} : ${Math.round(p * 100)} %`));
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
