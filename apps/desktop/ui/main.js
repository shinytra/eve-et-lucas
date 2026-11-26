// Interface minimale des notes partagées.
// Toutes les données passent par les commandes Tauri de src-tauri/src/lib.rs.

const $ = (id) => document.getElementById(id);
const form = $("note-form");
const titleInput = $("title");
const bodyInput = $("body");
const authorSelect = $("author");
const saveButton = $("save");
const cancelButton = $("cancel");
const errorBox = $("error");
const list = $("notes");

let editingId = null;

function showError(message) {
  errorBox.textContent = message ?? "";
  errorBox.hidden = !message;
}

const invoke = window.__TAURI__?.core?.invoke;
if (!invoke) {
  showError("Interface Tauri indisponible : ouvre l'appli Eve & Lucas, pas le fichier dans un navigateur.");
  throw new Error("window.__TAURI__ absent");
}

// Le prénom choisi est mémorisé sur cet appareil (simple confort : si le
// stockage local est indisponible, on garde le premier prénom de la liste).
try {
  const saved = localStorage.getItem("author");
  if (saved) authorSelect.value = saved;
} catch {}
if (authorSelect.selectedIndex < 0) authorSelect.selectedIndex = 0;
authorSelect.addEventListener("change", () => {
  try {
    localStorage.setItem("author", authorSelect.value);
  } catch {}
});

function formatDate(ms) {
  return new Date(ms).toLocaleString("fr-FR", {
    dateStyle: "short",
    timeStyle: "short",
  });
}

function resetForm() {
  editingId = null;
  form.reset();
  saveButton.textContent = "Ajouter";
  cancelButton.hidden = true;
}

function startEdit(note) {
  editingId = note.id;
  titleInput.value = note.title;
  bodyInput.value = note.body;
  saveButton.textContent = "Enregistrer";
  cancelButton.hidden = false;
  titleInput.focus();
}

function render(notes) {
  list.replaceChildren(
    ...notes.map((note) => {
      const li = document.createElement("li");
      li.className = "note";

      const h3 = document.createElement("h3");
      h3.textContent = note.title || "Sans titre";

      const body = document.createElement("p");
      body.className = "body";
      body.textContent = note.body;

      const meta = document.createElement("p");
      meta.className = "meta";
      meta.textContent = `${note.author || "?"} · ${formatDate(note.updated_at)}`;

      const edit = document.createElement("button");
      edit.textContent = "Modifier";
      edit.addEventListener("click", () => startEdit(note));

      const del = document.createElement("button");
      del.textContent = "Supprimer";
      del.className = "danger";
      del.addEventListener("click", async () => {
        del.disabled = true;
        try {
          await invoke("delete_note", { id: note.id });
        } catch (e) {
          del.disabled = false;
          showError(String(e));
          return;
        }
        if (editingId === note.id) resetForm();
        await refresh("Note supprimée");
      });

      const actions = document.createElement("div");
      actions.className = "actions";
      actions.append(edit, del);

      li.append(h3, ...(note.body ? [body] : []), meta, actions);
      return li;
    }),
  );
  $("empty").hidden = notes.length > 0;
}

// `done` décrit l'action déjà réussie, pour ne pas faire croire qu'elle a
// échoué si seul le rechargement de la liste échoue.
async function refresh(done) {
  try {
    render(await invoke("list_notes"));
  } catch (e) {
    showError(done ? `${done}, mais la liste n'a pas pu être rechargée : ${e}` : String(e));
  }
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (saveButton.disabled) return;
  showError(null);
  const title = titleInput.value;
  const body = bodyInput.value;
  saveButton.disabled = true;
  try {
    if (editingId) {
      await invoke("update_note", { id: editingId, title, body });
    } else {
      await invoke("add_note", { author: authorSelect.value, title, body });
    }
  } catch (e) {
    showError(String(e));
    return;
  } finally {
    saveButton.disabled = false;
  }
  resetForm();
  await refresh("Note enregistrée");
});

cancelButton.addEventListener("click", resetForm);

(async () => {
  try {
    const warning = await invoke("startup_warning");
    if (warning) showError(warning);
  } catch (e) {
    showError(String(e));
  }
  await refresh();
})();

// Une synchro a apporté des changements d'un autre appareil.
window.__TAURI__.event?.listen("docs-changed", (event) => {
  if (event.payload?.includes("notes")) refresh();
});

// Problème survenu en tâche de fond (notifications refusées…).
window.__TAURI__.event?.listen("notice", (event) => showError(String(event.payload)));
