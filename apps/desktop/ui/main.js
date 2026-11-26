// Interface minimale des notes partagées.
// Toutes les données passent par les commandes Tauri de src-tauri/src/lib.rs.

const { invoke } = window.__TAURI__.core;

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

// Le prénom choisi est mémorisé sur cet appareil.
try {
  const saved = localStorage.getItem("author");
  if (saved) authorSelect.value = saved;
} catch {}
authorSelect.addEventListener("change", () => {
  try {
    localStorage.setItem("author", authorSelect.value);
  } catch {}
});

function showError(message) {
  errorBox.textContent = message ?? "";
  errorBox.hidden = !message;
}

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
        try {
          await invoke("delete_note", { id: note.id });
          if (editingId === note.id) resetForm();
          await refresh();
        } catch (e) {
          showError(String(e));
        }
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

async function refresh() {
  render(await invoke("list_notes"));
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  showError(null);
  const title = titleInput.value;
  const body = bodyInput.value;
  try {
    if (editingId) {
      await invoke("update_note", { id: editingId, title, body });
    } else {
      await invoke("add_note", { author: authorSelect.value, title, body });
    }
    resetForm();
    await refresh();
  } catch (e) {
    showError(String(e));
  }
});

cancelButton.addEventListener("click", resetForm);

refresh().catch((e) => showError(String(e)));
