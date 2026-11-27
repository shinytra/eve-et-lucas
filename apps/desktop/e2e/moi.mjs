// Test de bout en bout du choix « Je suis Lucas / Eve », via le serveur MCP Tauri.
// Prérequis : l'appli lancée avec `cargo run -p eve-et-lucas-desktop --features mcp-bridge`,
// de préférence avec un dossier de données neuf (EVE_ET_LUCAS_DATA_DIR) pour
// vérifier le premier lancement. Puis : `node moi.mjs [dossier des captures]`.
// À lancer avant les autres tests : il laisse « Lucas » choisi.
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
const outDir = resolve(process.argv[2] ?? "captures");
mkdirSync(outDir, { recursive: true });
const t = new StdioClientTransport({ command: "node_modules/.bin/mcp-server-tauri", args: [] });
const c = new Client({ name: "e2e-moi", version: "1" }); await c.connect(t);
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
const personne = async () => (await js("(async () => await window.__TAURI__.core.invoke('arrival_person'))()")).replace(/^"|"$/g, "");
const bienvenueVisible = async () => (await js("(() => !document.getElementById('bienvenue').hidden)()")) === "true";

console.log(await call("driver_session", { action: "start", host: "127.0.0.1", port: 9223 }));

// Premier lancement, sur téléphone : la fenêtre « Bienvenue » demande qui on est.
await call("manage_window", { action: "resize", width: 380, height: 780, logical: true });
await js("(() => { location.hash = ''; location.reload(); return true })()");
await sleep(1500);
if ((await personne()) === "") {
  check("premier lancement : la fenêtre Bienvenue est affichée", await bienvenueVisible());
  check("le reste de l'appli est inaccessible (inert)", (await js("(() => document.querySelector('main').inert && document.getElementById('nav').inert)()")) === "true");
  check("l'accueil ne salue personne avant le choix", (await js("(() => document.getElementById('accueil-title').textContent)()")).match(/^Bon(jour|soir) 👋$/) !== null);
  check("aucun prénom présélectionné", (await js("(() => document.querySelectorAll('#bienvenue [aria-checked=\"true\"]').length)()")) === "0");
  await call("webview_screenshot", { format: "png", filePath: `${outDir}/bienvenue-telephone.png` });
  await call("webview_interact", { action: "click", selector: '#bienvenue [data-moi="Eve"]' });
  await sleep(800);
  check("« Je suis Eve » ferme la fenêtre", !(await bienvenueVisible()));
  check("et rend l'appli accessible", (await js("(() => document.querySelector('main').inert)()")) === "false");
  check("et enregistre Eve pour cet appareil", (await personne()) === "Eve");
  check("l'accueil salue Eve", (await js("(() => document.getElementById('accueil-title').textContent)()")).includes("Eve"));
  check("la pastille de l'en-tête montre « E »", (await js("(() => document.getElementById('moi-pastille').textContent)()")) === "E");
} else {
  console.log("(prénom déjà choisi : premier lancement non vérifié, lancer l'appli avec un dossier de données neuf)");
  check("pas de fenêtre Bienvenue quand le prénom est déjà choisi", !(await bienvenueVisible()));
}

// Paramètres : on peut changer d'avis.
await call("webview_interact", { action: "click", selector: "#moi-pastille" });
await sleep(600);
check("la pastille ouvre l'écran Paramètres", (await js("(() => location.hash)()")) === "#parametres");
await call("webview_interact", { action: "click", selector: '#ecran-parametres [data-moi="Lucas"]' });
await sleep(600);
check("Paramètres : Lucas enregistré", (await personne()) === "Lucas");
check("Paramètres : Lucas coché", (await js(`(() => document.querySelector('#ecran-parametres [data-moi="Lucas"]').getAttribute('aria-checked'))()`)) === "true");
check("le prénom sert aux ajouts (#author)", (await js("(() => document.getElementById('author').value)()")) === "Lucas");
check("pas de défilement horizontal à 380 px", (await js("(() => document.documentElement.scrollWidth <= window.innerWidth)()")) === "true");
await call("webview_screenshot", { format: "png", filePath: `${outDir}/parametres-telephone.png` });

// La feuille « Plus » mène aussi aux Paramètres.
await call("webview_interact", { action: "click", selector: '#nav a[data-ecran="accueil"]' });
await sleep(400);
await call("webview_interact", { action: "click", selector: "#nav-plus" });
await sleep(400);
await call("webview_interact", { action: "click", selector: '#plus a[href="#parametres"]' });
await sleep(500);
check("« Plus » mène aux Paramètres", (await js("(() => location.hash)()")) === "#parametres");

// Le choix est gardé au redémarrage de l'interface.
await call("manage_window", { action: "resize", width: 1100, height: 760, logical: true });
await js("(() => { location.hash = ''; location.reload(); return true })()");
await sleep(1500);
check("rechargée : pas de fenêtre Bienvenue", !(await bienvenueVisible()));
check("rechargée : toujours Lucas", (await js("(() => document.getElementById('author').value)()")) === "Lucas");
await call("webview_interact", { action: "click", selector: '#nav a[data-ecran="parametres"]' });
await sleep(500);
await call("webview_screenshot", { format: "png", filePath: `${outDir}/parametres-pc.png` });

await c.close();
console.log(fails ? `${fails} échec(s)` : "tout est bon");
process.exit(fails ? 1 : 0);
