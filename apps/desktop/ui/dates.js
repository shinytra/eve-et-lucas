// Dates importantes : compte à rebours, liste, ajout et modification.
// Les calculs (prochaine fois, années, rappels) sont faits par le cœur Rust ;
// les rappels (notifications à 9 h) par src-tauri/src/dates.rs.

(() => {
  const tauri = window.__TAURI__;
  if (!tauri?.core?.invoke) return;
  const { invoke } = tauri.core;
  const $ = (id) => document.getElementById(id);
  const form = $("date-form");
  const titleInput = $("date-title");
  const kindSelect = $("date-kind");
  const dayInput = $("date-day");
  const remindSelect = $("date-remind");
  const yearlyBox = $("date-yearly");
  const countBox = $("date-count");
  const countLabel = $("date-count-label");
  const saveButton = $("date-save");
  const cancelButton = $("date-cancel");
  const errorBox = $("date-error");
  const list = $("dates");
  const next = $("date-next");

  const EMOJI = { couple: "💞", anniversaire: "🎂", fete: "🎉", autre: "📅" };
  const emoji = (kind) => EMOJI[kind] ?? EMOJI.autre;

  let editingId = null;

  function showError(message) {
    errorBox.textContent = message ?? "";
    errorBox.hidden = !message;
  }

  const me = () => $("author").value;
  const pad = (n) => String(n).padStart(2, "0");

  // « samedi 14 février 2027 » (l'année seulement si elle n'est pas celle en cours).
  function formatDay(day) {
    const d = new Date(day.year, day.month - 1, day.day);
    const options = { weekday: "long", day: "numeric", month: "long" };
    if (day.year !== new Date().getFullYear()) options.year = "numeric";
    return d.toLocaleDateString("fr-FR", options);
  }

  function countdown(daysLeft) {
    if (daysLeft === 0) return "Aujourd'hui";
    if (daysLeft === 1) return "Demain";
    return `Dans ${daysLeft} jours`;
  }

  function years(n) {
    return n === 1 ? "1 an" : `${n} ans`;
  }

  function meta(item) {
    if (item.next == null) {
      return `Passée : ${formatDay(item)}`;
    }
    const parts = [formatDay(item.next)];
    if (item.years != null) parts.push(years(item.years));
    if (!item.yearly) parts.push("une seule fois");
    return parts.join(" · ");
  }

  function syncCountBox() {
    countLabel.hidden = !yearlyBox.checked;
  }

  // Options ajoutées pour une modification (type ou rappel venus d'ailleurs).
  function removeExtraOptions() {
    for (const option of form.querySelectorAll("option[data-extra]")) option.remove();
  }

  function addExtraOption(select, value, label) {
    const option = new Option(label, value);
    option.dataset.extra = "1";
    select.add(option);
    select.value = value;
  }

  function resetForm() {
    editingId = null;
    removeExtraOptions();
    form.reset();
    syncCountBox();
    saveButton.textContent = "Ajouter";
    window.__ui?.fermer(form);
  }

  function startEdit(item) {
    removeExtraOptions();
    editingId = item.id;
    titleInput.value = item.title;
    // Type inconnu (version plus récente de l'appli) : gardé tel quel.
    if (EMOJI[item.kind]) kindSelect.value = item.kind;
    else addExtraOption(kindSelect, item.kind, `${EMOJI.autre} Autre`);
    const year = item.year ?? item.next?.year ?? new Date().getFullYear();
    // Sans année connue, le 29 février se saisit sur une année bissextile.
    const shownYear = item.year == null && item.month === 2 && item.day === 29 ? 2000 : year;
    dayInput.value = `${String(shownYear).padStart(4, "0")}-${pad(item.month)}-${pad(item.day)}`;
    remindSelect.value = String(item.remind_days);
    if (remindSelect.value !== String(item.remind_days)) {
      addExtraOption(remindSelect, String(item.remind_days), `${item.remind_days} jours avant`);
    }
    yearlyBox.checked = item.yearly;
    countBox.checked = item.year != null;
    syncCountBox();
    saveButton.textContent = "Enregistrer";
    window.__ui?.ouvrir(form);
    titleInput.focus();
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

  function renderNext(items) {
    const first = items.find((item) => item.next != null);
    next.hidden = !first;
    if (!first) return;
    const compte = document.createElement("span");
    compte.className = "compte";
    compte.textContent = first.days_left === 0 ? "Aujourd'hui !" : `J-${first.days_left}`;
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = `${emoji(first.kind)} ${first.title}`;
    const info = document.createElement("span");
    info.className = "meta";
    info.textContent = meta(first);
    next.replaceChildren(compte, title, info);
  }

  function render(items) {
    renderNext(items);
    list.replaceChildren(
      ...items.map((item) => {
        const li = document.createElement("li");
        li.className = "date";
        li.classList.toggle("today", item.days_left === 0);
        li.classList.toggle("passee", item.next == null);

        const icon = document.createElement("span");
        icon.className = "emoji";
        icon.setAttribute("aria-hidden", "true");
        icon.textContent = emoji(item.kind);

        const text = document.createElement("div");
        text.className = "text";
        const title = document.createElement("span");
        title.className = "title";
        title.textContent = item.title;
        const info = document.createElement("span");
        info.className = "meta";
        info.textContent = meta(item);
        text.append(title, info);

        const jours = document.createElement("span");
        jours.className = "jours";
        jours.textContent = item.days_left == null ? "" : countdown(item.days_left);

        const edit = window.__ui.iconButton("modifier", `Modifier « ${item.title} »`);
        edit.addEventListener("click", () => startEdit(item));

        const del = window.__ui.iconButton("supprimer", `Supprimer « ${item.title} »`, "danger");
        del.addEventListener("click", async () => {
          del.disabled = true;
          await window.__ui.sortie(li);
          const ok = await run(() => invoke("delete_date", { id: item.id }));
          if (!ok) {
            del.disabled = false;
            li.classList.remove("sortie");
          } else if (editingId === item.id) resetForm();
        });

        const actions = document.createElement("div");
        actions.className = "actions";
        actions.append(edit, del);

        li.append(icon, text, jours, actions);
        return li;
      }),
    );
    $("dates-empty").hidden = items.length > 0;
  }

  async function refresh() {
    try {
      render(await invoke("list_dates"));
    } catch (e) {
      showError(String(e));
    }
  }

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (saveButton.disabled) return;
    // Formulaire en `novalidate` : les règles sont vérifiées dans le cœur.
    const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(dayInput.value);
    if (!match) {
      showError(dayInput.validity.badInput ? "La date est incomplète." : "Choisis une date.");
      return;
    }
    const [, y, m, d] = match.map(Number);
    const yearly = yearlyBox.checked;
    const input = {
      title: titleInput.value,
      kind: kindSelect.value,
      month: m,
      day: d,
      year: !yearly || countBox.checked ? y : null,
      yearly,
      remind_days: Number(remindSelect.value),
    };
    saveButton.disabled = true;
    const ok = await run(() =>
      editingId
        ? invoke("update_date", { id: editingId, input })
        : invoke("add_date", { author: me(), input }),
    );
    saveButton.disabled = false;
    if (ok) resetForm();
  });

  yearlyBox.addEventListener("change", syncCountBox);
  cancelButton.addEventListener("click", resetForm);

  syncCountBox();
  refresh();
  // Le compte à rebours change à minuit, même sans modification.
  setInterval(refresh, 60_000);
  tauri.event?.listen("docs-changed", (event) => {
    if (event.payload?.includes("dates")) refresh();
  });
})();
