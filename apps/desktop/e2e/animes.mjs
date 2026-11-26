// Test de bout en bout des animés, via le serveur MCP Tauri (hypothesi/mcp-server-tauri).
// Prérequis : l'appli lancée avec `cargo run -p eve-et-lucas-desktop --features mcp-bridge`.
// Puis : `node animes.mjs [capture.png]`. Sans accès à AniList (réseau
// bloqué), la recherche doit échouer avec un message et proposer l'ajout à la main.
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
const out = process.argv[2] ?? "capture-animes.png";
const t = new StdioClientTransport({ command: "node_modules/.bin/mcp-server-tauri", args: [] });
const c = new Client({ name: "e2e", version: "1" }); await c.connect(t);
let fails = 0;
async function call(name, args) {
  const r = await c.callTool({ name, arguments: args });
  const txt = (r.content || []).filter(x => x.type === "text").map(x => x.text).join("\n");
  if (r.isError) { console.log(`✗ ${name}: ${txt.slice(0, 300)}`); fails++; }
  return txt;
}
async function js(script) { return call("webview_execute_js", { script }); }
function check(label, cond) { console.log(`${cond ? "✓" : "✗"} ${label}`); if (!cond) fails++; }
const sleep = ms => new Promise(r => setTimeout(r, ms));
const card = title => `[...document.querySelectorAll('#animes li')].find(li => li.textContent.includes(${JSON.stringify(title)}))`;
const inCard = (title, expr) => js(`(() => { const li = ${card(title)}; return li ? ${expr} : 'absent' })()`);

console.log(await call("driver_session", { action: "start", host: "127.0.0.1", port: 9223 }));
await js("(() => { location.hash = '#animes'; return true })()");
await sleep(500);

// Un animé « venu d'AniList » (ajouté par la commande, comme le ferait la recherche).
const info = {
  anilist_id: 154587, title: "Frieren MCP", title_native: "葬送のフリーレン",
  cover_url: "https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/bx154587-n1fmjRv4JQUd.jpg",
  color: "#d6f1a1", format: "TV", year: 2023, genres: ["Adventure", "Drama", "Fantasy"], episodes: 3,
};
const added = await call("ipc_execute_command", { command: "add_anime", args: { author: "Eve", info } });
check("commande add_anime", added.includes("Frieren MCP"));
// Ajouté hors de l'interface : on la prévient comme le fait la synchro.
await js("(() => { window.__TAURI__.event.emit('docs-changed', ['animes']); return true })()");
await sleep(800);
check("animé affiché dans la liste", (await inCard("Frieren MCP", "true")).includes("true"));
check("pas commencé, 3 épisodes", /Pas commencé · 3 épisodes/.test(await inCard("Frieren MCP", "li.querySelector('.progression').textContent")));
check("affiche de remplacement (initiales) sans réseau", /FM/.test(await inCard("Frieren MCP", "li.querySelector('.affiche').textContent")));

// +1 : passe « en cours ».
await js(`(() => { ${card("Frieren MCP")}.querySelector('.episode button.principal').click(); return true })()`);
await sleep(800);
check("+1 : arrêtés à l'épisode 1 / 3, par Lucas", /Arrêtés à l'épisode 1 \/ 3 · Lucas/.test(await inCard("Frieren MCP", "li.querySelector('.progression').textContent")));
check("statut passé à « En cours »", (await inCard("Frieren MCP", "li.querySelector('select').value")).includes("en-cours"));

// Saisie directe du dernier épisode : passe « fini ».
await js(`(() => { const i = ${card("Frieren MCP")}.querySelector('.episode input'); i.value = '3'; i.dispatchEvent(new Event('change')); return true })()`);
await sleep(800);
check("dernier épisode : statut « Fini »", (await inCard("Frieren MCP", "li.querySelector('select').value")).includes("fini"));
check("+1 désactivé à la fin", (await inCard("Frieren MCP", "li.querySelector('.episode button.principal').disabled")).includes("true"));

// Épisode au-delà du total : refusé avec un message.
await js(`(() => { const i = ${card("Frieren MCP")}.querySelector('.episode input'); i.value = '9'; i.dispatchEvent(new Event('change')); return true })()`);
await sleep(800);
const tooFar = await js("(() => { const e = document.getElementById('anime-error'); return e.hidden ? '' : e.textContent })()");
check("épisode 9 sur 3 refusé avec un message", /n'a que 3 épisodes/.test(tooFar));
check("le champ reprend l'épisode enregistré", (await inCard("Frieren MCP", "li.querySelector('.episode input').value")).includes("3"));

// Recherche sans réseau : message clair et ajout à la main.
await call("webview_keyboard", { action: "type", selector: "#anime-query", text: "Film MCP" });
await call("webview_interact", { action: "click", selector: "#anime-search-go" });
await sleep(4000);
const searchErr = await js("(() => { const e = document.getElementById('anime-error'); return e.hidden ? '' : e.textContent })()");
console.log("  message de recherche :", searchErr.slice(0, 200));
check("recherche : résultats ou message d'erreur", searchErr.length > 0 || (await js("(() => document.querySelectorAll('#anime-results-list li').length)()")).match(/[1-9]/));
await call("webview_interact", { action: "click", selector: "#anime-manual" });
await sleep(800);
check("ajout à la main", (await inCard("Film MCP", "true")).includes("true"));
check("ajouté à la main : « À voir », sans bouton Mettre à jour",
  (await inCard("Film MCP", "li.querySelector('select').value + '|' + li.querySelectorAll('.actions button').length")).includes("a-voir|1"));

// Filtres.
await js("(() => { [...document.querySelectorAll('#anime-filters button')].find(b => b.textContent.startsWith('Finis')).click(); return true })()");
await sleep(300);
check("filtre « Finis »", (await js(`(() => document.querySelectorAll('#animes li').length + '|' + !!${card("Frieren MCP")} + '|' + !!${card("Film MCP")})()`)).includes("|true|false"));
await call("webview_screenshot", { format: "png", filePath: out });
await js("(() => { document.querySelector('#anime-filters button').click(); return true })()");
await sleep(300);

// Statut changé à la main.
await js(`(() => { const s = ${card("Film MCP")}.querySelector('select'); s.value = 'en-pause'; s.dispatchEvent(new Event('change')); return true })()`);
await sleep(800);
check("statut « En pause » enregistré", (await call("ipc_execute_command", { command: "list_animes" })).includes("en-pause"));

// Suppression en deux appuis.
for (const title of ["Frieren MCP", "Film MCP"]) {
  await js(`(() => { ${card(title)}.querySelector('button.danger').click(); return true })()`);
  await sleep(200);
  check(`« ${title} » : demande de confirmation`, (await inCard(title, "li.querySelector('button.danger').textContent")).includes("Confirmer"));
  await js(`(() => { ${card(title)}.querySelector('button.danger').click(); return true })()`);
  await sleep(800);
  check(`« ${title} » supprimé`, (await inCard(title, "true")).includes("absent"));
}

const logs = await call("read_logs", { source: "console", level: "error" });
console.log("logs d'erreur console :", logs.slice(0, 400) || "(aucun)");
await call("driver_session", { action: "stop" });
await c.close();
console.log(fails ? `ÉCHECS : ${fails}` : "TOUT EST OK");
process.exit(fails ? 1 : 0);
