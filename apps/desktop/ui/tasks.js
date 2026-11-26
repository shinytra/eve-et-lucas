// Tâches communes : liste, ajout, modification, case à cocher.
// Les rappels (notifications) sont gérés côté Rust (src-tauri/src/reminders.rs).

(() => {
  const tauri = window.__TAURI__;
  if (!tauri?.core?.invoke) return;
  const { invoke } = tauri.core;
  const $ = (id) => document.getElementById(id);
  const form = $("task-form");
  const titleInput = $("task-title");
  const assigneeSelect = $("task-assignee");
  const dueInput = $("task-due");
  const saveButton = $("task-save");
  const cancelButton = $("task-cancel");
  const errorBox = $("task-error");
  const list = $("tasks");

  let editingId = null;

  function showError(message) {
    errorBox.textContent = message ?? "";
    errorBox.hidden = !message;
  }

  // Prénom choisi en haut de l'écran (« Je suis »).
  const me = () => $("author").value;

  function formatDate(ms) {
    return new Date(ms).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" });
  }

  // <input type="datetime-local"> travaille en heure locale, sans fuseau.
  function toInputValue(ms) {
    const d = new Date(ms);
    const pad = (n) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }

  function fromInputValue(value) {
    if (!value) return null;
    const ms = new Date(value).getTime();
    return Number.isFinite(ms) ? ms : null;
  }

  function resetForm() {
    editingId = null;
    form.reset();
    saveButton.textContent = "Ajouter";
    cancelButton.hidden = true;
  }

  function startEdit(task) {
    editingId = task.id;
    titleInput.value = task.title;
    assigneeSelect.value = task.assignee;
    dueInput.value = task.due_at == null ? "" : toInputValue(task.due_at);
    saveButton.textContent = "Enregistrer";
    cancelButton.hidden = false;
    titleInput.focus();
  }

  function meta(task) {
    const parts = [task.assignee ? `Pour ${task.assignee}` : "Tous les deux"];
    if (task.done) {
      parts.push(`Faite par ${task.done_by || "?"} le ${formatDate(task.done_at ?? task.updated_at)}`);
    } else if (task.due_at != null) {
      const late = task.due_at <= Date.now();
      parts.push(`${late ? "En retard depuis" : "Rappel"} le ${formatDate(task.due_at)}`);
    }
    return parts.join(" · ");
  }

  async function run(action) {
    showError(null);
    try {
      await action();
    } catch (e) {
      showError(String(e));
      return false;
    }
    await refresh();
    return true;
  }

  function render(tasks) {
    list.replaceChildren(
      ...tasks.map((task) => {
        const li = document.createElement("li");
        li.className = "task";
        li.classList.toggle("done", task.done);
        li.classList.toggle("late", !task.done && task.due_at != null && task.due_at <= Date.now());

        const check = document.createElement("input");
        check.type = "checkbox";
        check.checked = task.done;
        check.setAttribute("aria-label", task.done ? `Décocher « ${task.title} »` : `Cocher « ${task.title} »`);
        check.addEventListener("change", async () => {
          check.disabled = true;
          const ok = await run(() => invoke("set_task_done", { id: task.id, done: check.checked, by: me() }));
          if (!ok) {
            check.checked = !check.checked;
            check.disabled = false;
          }
        });

        const text = document.createElement("div");
        text.className = "text";
        const title = document.createElement("span");
        title.className = "title";
        title.textContent = task.title;
        const info = document.createElement("span");
        info.className = "meta";
        info.textContent = meta(task);
        text.append(title, info);

        const edit = document.createElement("button");
        edit.textContent = "Modifier";
        edit.addEventListener("click", () => startEdit(task));

        const del = document.createElement("button");
        del.textContent = "Supprimer";
        del.className = "danger";
        del.addEventListener("click", async () => {
          del.disabled = true;
          const ok = await run(() => invoke("delete_task", { id: task.id }));
          if (!ok) del.disabled = false;
          else if (editingId === task.id) resetForm();
        });

        const actions = document.createElement("div");
        actions.className = "actions";
        actions.append(edit, del);

        li.append(check, text, actions);
        return li;
      }),
    );
    $("tasks-empty").hidden = tasks.length > 0;
  }

  async function refresh() {
    try {
      render(await invoke("list_tasks"));
    } catch (e) {
      showError(String(e));
    }
  }

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (saveButton.disabled) return;
    // Le formulaire est en `novalidate` : la validation native de
    // datetime-local de WebKitGTK bloque parfois l'envoi sans rien afficher.
    if (dueInput.validity.badInput) {
      showError("La date du rappel est incomplète.");
      return;
    }
    const args = {
      title: titleInput.value,
      assignee: assigneeSelect.value,
      dueAt: fromInputValue(dueInput.value),
    };
    saveButton.disabled = true;
    const ok = await run(() =>
      editingId
        ? invoke("update_task", { id: editingId, ...args })
        : invoke("add_task", { author: me(), ...args }),
    );
    saveButton.disabled = false;
    if (ok) resetForm();
  });

  cancelButton.addEventListener("click", resetForm);

  refresh();
  // « En retard » change avec l'heure, même sans modification.
  setInterval(refresh, 60_000);
  tauri.event?.listen("docs-changed", (event) => {
    if (event.payload?.includes("tasks")) refresh();
  });
})();
