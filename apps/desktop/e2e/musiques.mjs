// Test de bout en bout des musiques, via le serveur MCP Tauri (hypothesi/mcp-server-tauri).
// Prérequis : l'appli lancée avec `cargo run -p eve-et-lucas-desktop --features mcp-bridge`.
// Puis : `node musiques.mjs [capture.png]`. Sans accès à YouTube (réseau
// bloqué), le titre n'est pas trouvé : la musique est ajoutée quand même.
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
const out = process.argv[2] ?? "capture-musiques.png";
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
const card = title => `[...document.querySelectorAll('#musiques li')].find(li => li.textContent.includes(${JSON.stringify(title)}))`;
const inCard = (title, expr) => js(`(() => { const li = ${card(title)}; return li ? ${expr} : 'absent' })()`);
const error = () => js("(() => { const e = document.getElementById('music-error'); return e.hidden ? '' : e.textContent })()");

async function clearAll() {
  const ids = JSON.parse(await call("ipc_execute_command", { command: "list_musics" })).result.map(m => m.id);
  for (const id of ids) await call("ipc_execute_command", { command: "delete_music", args: { id } });
}

console.log(await call("driver_session", { action: "start", host: "127.0.0.1", port: 9223 }));
// Repart d'une liste vide, filtre « À écouter ».
await clearAll();
await js("(() => { try { localStorage.removeItem('music-filter') } catch {} location.reload(); return true })()");
await sleep(2500);
await call("manage_window", { action: "resize", width: 420, height: 860, logical: true });
await js("(() => { location.hash = '#musiques'; return true })()");
await sleep(500);
check("écran Musiques affiché", (await js("(() => document.getElementById('titre-ecran').textContent)()")).includes("Musiques"));
check("tuile Musiques dans « Plus »", (await js("(() => !!document.querySelector('#plus a[href=\"#musiques\"]'))()")).includes("true"));

// Ajout par le formulaire, lien partagé par YouTube Music (avec ?si=…), titre donné.
await js("(() => { document.querySelector('.ajouter[data-form=\"music-form\"]').click(); return true })()");
await sleep(300);
await js(`(() => {
  document.getElementById('music-link').value = 'Écoute ça https://music.youtube.com/watch?v=dQw4w9WgXcQ&si=AbC';
  document.getElementById('music-title').value = 'Never Gonna MCP';
  document.getElementById('music-message').value = 'Le refrain !';
  document.getElementById('music-form').requestSubmit();
  return true })()`);
await sleep(1500);
check("musique affichée", (await inCard("Never Gonna MCP", "true")).includes("true"));
check("petit mot affiché", (await inCard("Never Gonna MCP", "li.querySelector('.mot').textContent")).includes("« Le refrain ! »"));
check("recommandée par Lucas", (await inCard("Never Gonna MCP", "li.textContent")).includes("Recommandée par Lucas"));
const listed = await call("ipc_execute_command", { command: "list_musics" });
check("lien gardé sous forme courte", listed.includes("titre:dQw4w9WgXcQ"));
check("miniature YouTube", listed.includes("https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg"));

// Même morceau, autre forme de lien : refusé.
await js("(() => { document.querySelector('.ajouter[data-form=\"music-form\"]').click(); return true })()");
await sleep(300);
await js("(() => { document.getElementById('music-link').value = 'https://youtu.be/dQw4w9WgXcQ'; document.getElementById('music-form').requestSubmit(); return true })()");
await sleep(1500);
check("doublon refusé avec un message", /déjà dans la liste/.test(await error()));

// Une playlist n'est pas un morceau : refusée.
await js("(() => { document.getElementById('music-link').value = 'https://music.youtube.com/playlist?list=PLabc123'; document.getElementById('music-form').requestSubmit(); return true })()");
await sleep(800);
check("playlist refusée", /une musique à la fois/.test(await error()));

// Sans titre et sans réseau : ajoutée quand même (le titre peut venir de YouTube si le réseau passe).
await js("(() => { document.getElementById('music-link').value = 'https://www.youtube.com/watch?v=aaaaaaaaaaa'; document.getElementById('music-title').value = ''; document.getElementById('music-message').value = ''; document.getElementById('music-form').requestSubmit(); return true })()");
await sleep(10000);
const count2 = await js("(() => document.querySelectorAll('#musiques li').length)()"); check("deuxième musique ajoutée", count2.startsWith("2"));

// Écoutée : passe dans « Écoutées ».
await js(`(() => { const b = ${card("Never Gonna MCP")}.querySelector('input[type=checkbox]'); b.click(); return true })()`);
await sleep(800);
check("plus dans « À écouter »", (await inCard("Never Gonna MCP", "true")).includes("absent"));
await js("(() => { [...document.querySelectorAll('#music-filters button')].find(b => b.textContent.startsWith('Écoutées')).click(); return true })()");
await sleep(300);
check("écoutée par Lucas", (await inCard("Never Gonna MCP", "li.textContent")).includes("écoutée par Lucas"));
await call("webview_screenshot", { format: "png", filePath: out });

// Modification du titre et de l'artiste.
await js(`(() => { ${card("Never Gonna MCP")}.querySelector('button[aria-label^="Modifier"]').click(); return true })()`);
await sleep(300);
check("le lien est caché en modification", (await js("(() => document.getElementById('music-link').hidden)()")).includes("true"));
await js("(() => { document.getElementById('music-title').value = 'Titre MCP modifié'; document.getElementById('music-artist').value = 'Rick'; document.getElementById('music-form').requestSubmit(); return true })()");
await sleep(800);
check("titre et artiste modifiés", (await inCard("Titre MCP modifié", "li.textContent")).includes("Rick"));

// Suppression des deux.
await clearAll();
await js("(() => { window.__TAURI__.event.emit('docs-changed', ['musiques']); return true })()");
await sleep(800);
check("liste vide", (await js("(() => document.querySelectorAll('#musiques li').length)()")).startsWith("0"));

const logs = await call("read_logs", { source: "console", level: "error" });
console.log("logs d'erreur console :", logs.slice(0, 400) || "(aucun)");
await call("manage_window", { action: "resize", width: 1100, height: 760, logical: true });
await call("driver_session", { action: "stop" });
await c.close();
console.log(fails ? `ÉCHECS : ${fails}` : "TOUT EST OK");
process.exit(fails ? 1 : 0);
