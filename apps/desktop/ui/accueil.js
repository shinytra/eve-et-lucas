// Écran d'accueil : résumé du jour (prochaine date, tâches à faire, dernière
// arrivée, dernière note). Rechargé à chaque affichage et quand une synchro
// apporte des changements.

(() => {
  const tauri = window.__TAURI__;
  if (!tauri?.core?.invoke) return;
  const { invoke } = tauri.core;
  const $ = (id) => document.getElementById(id);
  const errorBox = $("accueil-error");
  const EMOJI = { couple: "💞", anniversaire: "🎂", fete: "🎉", autre: "📅" };
  const MAX_TACHES = 5;

  function showError(message) {
    errorBox.textContent = message ?? "";
    errorBox.hidden = !message;
  }

  const el = (tag, className, text) => {
    const e = document.createElement(tag);
    if (className) e.className = className;
    if (text != null) e.textContent = text;
    return e;
  };

  function depuis(ms) {
    const minutes = Math.round((Date.now() - ms) / 60_000);
    if (minutes < 1) return "à l'instant";
    if (minutes < 60) return `il y a ${minutes} min`;
    const heures = Math.round(minutes / 60);
    if (heures < 24) return `il y a ${heures} h`;
    const jours = Math.round(heures / 24);
    return jours === 1 ? "hier" : `il y a ${jours} jours`;
  }

  function salut() {
    const h = new Date().getHours();
    const mot = h >= 18 || h < 5 ? "Bonsoir" : "Bonjour";
    $("accueil-title").textContent = `${mot} ${$("author").value} 👋`.replace("  ", " ");
    const jour = new Date().toLocaleDateString("fr-FR", { weekday: "long", day: "numeric", month: "long" });
    $("accueil-jour").textContent = jour.charAt(0).toUpperCase() + jour.slice(1);
  }

  // Le nombre de jours défile jusqu'à sa valeur.
  function compter(element, cible, format) {
    const reduit = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduit || cible <= 1) {
      element.textContent = format(cible);
      return;
    }
    const debut = performance.now();
    const duree = Math.min(900, 300 + cible * 8);
    const pas = (t) => {
      const p = Math.min(1, (t - debut) / duree);
      const v = Math.round(cible * (1 - Math.pow(1 - p, 3)));
      element.textContent = format(v);
      if (p < 1) requestAnimationFrame(pas);
    };
    requestAnimationFrame(pas);
  }

  // Le compte à rebours ne défile qu'à l'affichage de l'écran ou quand il
  // change, pas à chaque rafraîchissement de la minute.
  let derniereDate = null;
  let animerDate = true;

  function renderDate(dates) {
    const carte = $("accueil-date");
    const prochaine = dates.find((d) => d.next != null);
    carte.hidden = !prochaine;
    if (!prochaine) return;
    const compte = el("span", "compte");
    const titre = el("span", "title", `${EMOJI[prochaine.kind] ?? EMOJI.autre} ${prochaine.title}`);
    const n = prochaine.next;
    const jour = new Date(n.year, n.month - 1, n.day).toLocaleDateString("fr-FR", {
      weekday: "long",
      day: "numeric",
      month: "long",
    });
    const info = el("span", "meta", prochaine.years != null ? `${jour} · ${prochaine.years} an${prochaine.years > 1 ? "s" : ""}` : jour);
    carte.replaceChildren(el("span", "etiquette", "Prochaine date"), compte, titre, info);
    const cle = `${prochaine.id}:${prochaine.days_left}`;
    const animer = animerDate || cle !== derniereDate;
    derniereDate = cle;
    animerDate = false;
    if (prochaine.days_left === 0) compte.textContent = "Aujourd'hui !";
    else if (animer) compter(compte, prochaine.days_left, (v) => `J-${v}`);
    else compte.textContent = `J-${prochaine.days_left}`;
  }

  function renderTaches(taches) {
    const aFaire = taches.filter((t) => !t.done);
    const moi = $("author").value;
    // Les miennes et celles des deux d'abord, puis par échéance.
    aFaire.sort((a, b) => {
      const pa = a.assignee && a.assignee !== moi ? 1 : 0;
      const pb = b.assignee && b.assignee !== moi ? 1 : 0;
      return pa - pb || (a.due_at ?? Infinity) - (b.due_at ?? Infinity);
    });
    const liste = $("accueil-taches");
    liste.replaceChildren(
      ...aFaire.slice(0, MAX_TACHES).map((tache, i) => {
        const li = el("li", "task");
        li.style.setProperty("--i", String(i));
        li.classList.toggle("late", tache.due_at != null && tache.due_at <= Date.now());
        const check = document.createElement("input");
        check.type = "checkbox";
        check.setAttribute("aria-label", `Cocher « ${tache.title} »`);
        check.addEventListener("change", async () => {
          check.disabled = true;
          li.classList.add("vient-cocher", "done");
          try {
            await invoke("set_task_done", { id: tache.id, done: true, by: moi });
          } catch (e) {
            check.checked = false;
            check.disabled = false;
            li.classList.remove("vient-cocher", "done");
            showError(String(e));
            return;
          }
          document.dispatchEvent(new CustomEvent("donnees-modifiees", { detail: ["tasks"] }));
          setTimeout(refresh, 600);
        });
        const texte = el("div", "text");
        texte.append(el("span", "title", tache.title));
        const qui = tache.assignee ? `Pour ${tache.assignee}` : "Tous les deux";
        if (tache.due_at != null) {
          const quand = new Date(tache.due_at).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" });
          texte.append(el("span", "meta", `${qui} · ${tache.due_at <= Date.now() ? "en retard" : "rappel"} ${quand}`));
        } else {
          texte.append(el("span", "meta", qui));
        }
        li.append(check, texte);
        return li;
      }),
    );
    $("accueil-taches-vide").hidden = aFaire.length > 0;
    const reste = aFaire.length - MAX_TACHES;
    if (reste > 0) liste.append(el("li", "meta plus-de-taches", `et ${reste} autre${reste > 1 ? "s" : ""}…`));
  }

  function renderArrivee(arrivees) {
    const carte = $("accueil-arrivee");
    const derniere = arrivees[0];
    carte.hidden = !derniere;
    if (!derniere) return;
    carte.replaceChildren(
      el("span", "emoji", "🏠"),
      el("span", "etiquette", "Dernière arrivée"),
      el("span", "title", derniere.who),
      el("span", "meta", depuis(derniere.at)),
    );
  }

  function renderNote(notes) {
    const carte = $("accueil-note");
    const derniere = [...notes].sort((a, b) => b.updated_at - a.updated_at)[0];
    carte.hidden = !derniere;
    if (!derniere) return;
    const extrait = (derniere.body || "").split("\n").filter(Boolean).slice(0, 2).join(" · ");
    carte.replaceChildren(
      el("span", "emoji", "📝"),
      el("span", "etiquette", "Dernière note"),
      el("span", "title", derniere.title || "Sans titre"),
      el("span", "meta extrait", extrait || `${derniere.author || "?"} · ${depuis(derniere.updated_at)}`),
    );
  }

  let dernierEtat = null;

  function renderSynchro(etat) {
    dernierEtat = etat;
    const p = $("accueil-synchro");
    if (!etat || etat.error || !etat.total) {
      p.hidden = true;
      return;
    }
    p.hidden = false;
    p.classList.toggle("ok", etat.reached > 0);
    p.textContent =
      etat.reached > 0
        ? `Synchronisé ${depuis(etat.at)} · ${etat.reached}/${etat.total} appareil${etat.total > 1 ? "s" : ""}`
        : "Aucun autre appareil joint pour l'instant";
  }

  // Un rafraîchissement demandé pendant un autre est relancé ensuite, pour ne
  // jamais afficher des données plus anciennes que la dernière demande.
  let enCours = false;
  let relancer = false;
  async function refresh() {
    if (enCours) {
      relancer = true;
      return;
    }
    enCours = true;
    try {
      salut();
      if (dernierEtat) renderSynchro(dernierEtat);
      // Chaque carte se remplit même si une autre source échoue.
      const sources = [
        ["list_dates", renderDate],
        ["list_tasks", renderTaches],
        ["list_arrivals", renderArrivee],
        ["list_notes", renderNote],
      ];
      const resultats = await Promise.allSettled(sources.map(([cmd]) => invoke(cmd)));
      const erreurs = [];
      resultats.forEach((r, i) => {
        try {
          if (r.status === "fulfilled") sources[i][1](r.value ?? []);
          else erreurs.push(String(r.reason));
        } catch (e) {
          erreurs.push(String(e));
        }
      });
      showError(erreurs.length ? erreurs.join("\n") : null);
    } finally {
      enCours = false;
    }
    if (relancer) {
      relancer = false;
      refresh();
    }
  }

  invoke("sync_status").then(renderSynchro).catch(() => {});
  tauri.event?.listen("sync-status", ({ payload }) => renderSynchro(payload));
  tauri.event?.listen("docs-changed", () => {
    if (window.__ui?.ecran() === "accueil") refresh();
  });
  document.addEventListener("ecran", ({ detail }) => {
    if (detail !== "accueil") return;
    animerDate = true;
    refresh();
  });
  $("author").addEventListener("change", () => {
    if (window.__ui?.ecran() === "accueil") refresh();
  });
  // Les « il y a … » et les retards changent avec l'heure.
  setInterval(() => {
    if (window.__ui?.ecran() === "accueil" && !document.hidden) refresh();
  }, 60_000);
  refresh();
})();
