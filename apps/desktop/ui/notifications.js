// Réglages des notifications de cet appareil : un interrupteur par type de
// nouveauté (note ajoutée, tâche faite…). Les règles sont dans le cœur
// (couple_core::notify).

(() => {
  const tauri = window.__TAURI__;
  if (!tauri?.core?.invoke) return;
  const { invoke } = tauri.core;
  const $ = (id) => document.getElementById(id);
  const list = $("notif-list");
  const errorBox = $("notif-error");

  function showError(message) {
    errorBox.textContent = message ?? "";
    errorBox.hidden = !message;
  }

  function render(choices) {
    list.replaceChildren(
      ...choices.map((choice) => {
        const li = document.createElement("li");
        const label = document.createElement("label");
        label.className = "check";
        const input = document.createElement("input");
        input.type = "checkbox";
        input.checked = choice.enabled;
        input.dataset.kind = choice.code;
        const texts = document.createElement("span");
        const name = document.createElement("strong");
        name.textContent = choice.label;
        const description = document.createElement("span");
        description.className = "meta";
        description.textContent = choice.description;
        texts.append(name, description);
        label.append(input, texts);
        li.append(label);
        input.addEventListener("change", async () => {
          input.disabled = true;
          try {
            render(await invoke("set_notification", { kind: choice.code, enabled: input.checked }));
            showError(null);
          } catch (e) {
            input.checked = !input.checked;
            input.disabled = false;
            showError(String(e));
          }
        });
        return li;
      }),
    );
  }

  async function refresh() {
    try {
      render(await invoke("notification_settings"));
      showError(null);
    } catch (e) {
      showError(String(e));
    }
  }

  // Téléphone sans prénom choisi : il ne peut pas reconnaître les ajouts
  // faits par son propriétaire sur le PC.
  async function refreshOwner() {
    if (!/Android/i.test(navigator.userAgent)) return;
    try {
      $("notif-owner").hidden = (await invoke("arrival_person")).trim() !== "";
    } catch {
      $("notif-owner").hidden = true;
    }
  }

  refresh();
  refreshOwner();
  window.addEventListener("hashchange", () => {
    if (location.hash === "#notifications") refreshOwner();
  });
})();
