// Test de bout en bout de la navigation et de l'accueil, via le serveur MCP Tauri.
// Prérequis : l'appli lancée avec `cargo run -p eve-et-lucas-desktop --features mcp-bridge`.
// Puis : `node navigation.mjs [dossier des captures]`.
// Ajoute une tâche d'exemple, la coche depuis l'accueil puis la supprime.
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
const outDir = resolve(process.argv[2] ?? "captures");
mkdirSync(outDir, { recursive: true });
const t = new StdioClientTransport({ command: "node_modules/.bin/mcp-server-tauri", args: [] });
const c = new Client({ name: "e2e-navigation", version: "1" }); await c.connect(t);
let fails = 0;
async function call(name, args) {
  const r = await c.callTool({ name, arguments: args });
  const txt = (r.content || []).filter(x => x.type === "text").map(x => x.text).join("\n");
  if (r.isError) { console.log(`✗ ${name}: ${txt.slice(0, 300)}`); fails++; }
  return txt;
}
const js = async (script) => (await call("webview_execute_js", { script })).replace(/\n?\[Executed in window:[^\]]*\]\s*$/, "").trim();
function check(label, cond) { console.log(`${cond ? "✓" : "✗"} ${label}`); if (!cond) fails++; }
const sleep = ms => new Promise(r => setTimeout(r, ms));
const visibles = () => js("(() => [...document.querySelectorAll('main > section[data-ecran]')].filter(s => getComputedStyle(s).display !== 'none').map(s => s.dataset.ecran).join(','))()");

console.log(await call("driver_session", { action: "start", host: "127.0.0.1", port: 9223 }));

// PC : menu latéral.
await call("manage_window", { action: "resize", width: 1100, height: 760, logical: true });
await js("(() => { location.hash = ''; location.reload(); return true })()");
await sleep(1500);
check("l'appli s'ouvre sur l'accueil", (await visibles()) === "accueil");
check("le menu latéral est affiché sur PC", (await js("(() => getComputedStyle(document.querySelector('.nav .marque')).display)()")) !== "none");

for (const ecran of ["taches", "dates", "notes", "fichiers", "maison", "appareils"]) {
  await call("webview_interact", { action: "click", selector: `#nav a[data-ecran="${ecran}"]` });
  await sleep(500);
  check(`le menu ouvre l'écran « ${ecran} » (et lui seul)`, (await visibles()) === ecran);
  check(`l'onglet « ${ecran} » est marqué comme actif`, (await js(`(() => document.querySelector('#nav a[aria-current="page"]')?.dataset.ecran)()`)) === ecran);
}
await js("(() => { history.back(); return true })()");
await sleep(500);
check("le retour revient à l'écran précédent", (await visibles()) === "maison");

// Accueil : une tâche cochée depuis l'accueil est faite partout.
const added = await call("ipc_execute_command", { command: "add_task", args: { author: "Lucas", title: "Tâche accueil MCP", assignee: "", dueAt: null } });
const id = added.match(/"id"\s*:\s*"([0-9a-f-]{36})"/)?.[1];
await call("webview_interact", { action: "click", selector: '#nav a[data-ecran="accueil"]' });
await sleep(900);
const ligne = "[...document.querySelectorAll('#accueil-taches li')].find(li => li.textContent.includes('Tâche accueil MCP'))";
check("la tâche à faire apparaît sur l'accueil", (await js(`(() => !!${ligne})()`)) === "true");
await call("webview_screenshot", { format: "png", filePath: `${outDir}/accueil-pc.png` });
await js(`(() => { ${ligne}.querySelector('input').click(); return true })()`);
await sleep(1200);
const taches = await call("ipc_execute_command", { command: "list_tasks" });
check("cochée depuis l'accueil : enregistrée comme faite", /Tâche accueil MCP[^}]*"done"\s*:\s*true/.test(taches) || /"done"\s*:\s*true[^}]*Tâche accueil MCP/.test(taches));
check("et retirée de l'accueil", (await js(`(() => !!${ligne})()`)) === "false");
check("et cochée dans l'écran Tâches", (await js("(() => [...document.querySelectorAll('#tasks li')].find(li => li.textContent.includes('Tâche accueil MCP'))?.classList.contains('done'))()")) === "true");
if (id) await call("ipc_execute_command", { command: "delete_task", args: { id } });

// Téléphone : onglets en bas, « Plus » pour les autres écrans, « + » flottant.
await call("manage_window", { action: "resize", width: 380, height: 780, logical: true });
await sleep(600);
check("la barre d'onglets du bas montre « Plus »", (await js("(() => getComputedStyle(document.getElementById('nav-plus')).display)()")) !== "none");
await call("webview_interact", { action: "click", selector: "#nav-plus" });
await sleep(500);
check("« Plus » ouvre la feuille des autres écrans", (await js("(() => !document.getElementById('plus').hidden)()")) === "true");
await call("webview_screenshot", { format: "png", filePath: `${outDir}/plus-telephone.png` });
await call("webview_interact", { action: "click", selector: '#plus a[href="#appareils"]' });
await sleep(500);
check("la feuille mène à Appareils et se ferme", (await visibles()) === "appareils" && (await js("(() => document.getElementById('plus').hidden)()")) === "true");
await call("webview_interact", { action: "click", selector: '#nav a[data-ecran="taches"]' });
await sleep(500);
check("le « + » flottant est affiché sur Tâches", (await js("(() => !document.getElementById('fab').hidden)()")) === "true");
await call("webview_interact", { action: "click", selector: "#fab" });
await sleep(500);
check("le « + » ouvre le formulaire", (await js("(() => document.getElementById('task-form').classList.contains('ouvert'))()")) === "true");
await call("webview_screenshot", { format: "png", filePath: `${outDir}/taches-telephone.png` });
await call("webview_interact", { action: "click", selector: "#task-cancel" });
await sleep(300);
check("« Annuler » replie le formulaire", (await js("(() => document.getElementById('task-form').classList.contains('ouvert'))()")) === "false");

await call("manage_window", { action: "resize", width: 1100, height: 760, logical: true });
const logs = await call("read_logs", { source: "console", level: "error" });
console.log("logs d'erreur console :", logs.slice(0, 400) || "(aucun)");
await call("driver_session", { action: "stop" });
await c.close();
console.log(fails ? `ÉCHECS : ${fails}` : "TOUT EST OK");
process.exit(fails ? 1 : 0);
