// Test de bout en bout de l'apparence, via le serveur MCP Tauri.
// Prérequis : l'appli lancée avec `cargo run -p eve-et-lucas-desktop --features mcp-bridge`.
// Puis : `cd apps/desktop/e2e && npm install && npm run test:apparence`.
// Attention : le test change le thème de l'appli (il remet Givre à la fin)
// et ajoute deux notes d'exemple qu'il supprime ensuite.
// Les captures sont écrites dans le dossier donné en argument (par défaut `captures/`).
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const outDir = resolve(process.argv[2] ?? "captures");
mkdirSync(outDir, { recursive: true });
const t = new StdioClientTransport({ command: "node_modules/.bin/mcp-server-tauri", args: [] });
const c = new Client({ name: "e2e-apparence", version: "1" });
await c.connect(t);
let fails = 0;
async function call(name, args) {
  const r = await c.callTool({ name, arguments: args });
  const txt = (r.content || []).filter((x) => x.type === "text").map((x) => x.text).join("\n");
  if (r.isError) {
    console.log(`✗ ${name}: ${txt.slice(0, 300)}`);
    fails++;
  }
  return txt;
}
// Le serveur MCP ajoute « [Executed in window: …] » après le résultat.
async function js(script) {
  return (await call("webview_execute_js", { script })).replace(/\n?\[Executed in window:[^\]]*\]\s*$/, "").trim();
}
function check(label, cond) {
  console.log(`${cond ? "✓" : "✗"} ${label}`);
  if (!cond) fails++;
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const shot = (name) => call("webview_screenshot", { format: "png", filePath: `${outDir}/${name}.png` });
const ipcJson = async (command, args) => {
  const txt = await call("ipc_execute_command", { command, args });
  if (process.env.DEBUG_IPC) console.log(txt.slice(0, 300));
  const start = txt.indexOf("{");
  const end = txt.lastIndexOf("}");
  try {
    const parsed = JSON.parse(txt.slice(start, end + 1));
    return parsed.result ?? parsed;
  } catch {
    return txt;
  }
};
const cssVar = (name) => js(`(() => getComputedStyle(document.documentElement).getPropertyValue('${name}').trim())()`);

console.log(await call("driver_session", { action: "start", host: "127.0.0.1", port: 9223 }));
await call("manage_window", { action: "resize", width: 1100, height: 760, logical: true });
await sleep(500);

// Deux notes d'exemple pour que les captures ressemblent à un vrai usage.
const added = [];
for (const [title, body] of [
  ["Courses", "lait\npain\nfraises"],
  ["Week-end", "Réserver le resto pour samedi soir"],
]) {
  const r = await call("ipc_execute_command", { command: "add_note", args: { author: "Lucas", title, body } });
  const id = r.match(/"id"\s*:\s*"([0-9a-f-]{36})"/)?.[1];
  if (id) added.push(id);
}
await js("(() => { location.reload(); return true })()");
await sleep(1500);

await call("webview_interact", { action: "click", selector: "#open-settings" });
await sleep(400);
check("l'écran des réglages s'ouvre", (await js("(() => !document.getElementById('settings').hidden)()")).includes("true"));

const presets = JSON.parse((await js("(() => JSON.stringify(window.__apparence.presets()))()")) || "[]");
check(`les préréglages sont affichés (${presets.length})`, presets.length >= 6);

// Chaque préréglage : clic sur sa carte, vérification, capture.
for (const id of presets) {
  await call("webview_interact", { action: "click", selector: `[data-preset="${id}"]` });
  await sleep(700);
  const saved = await ipcJson("get_appearance");
  check(`préréglage ${id} appliqué et enregistré`, saved?.current?.preset === id);
  await shot(`reglages-${id}`);
  await call("webview_interact", { action: "click", selector: "#close-settings" });
  await sleep(400);
  await shot(`preset-${id}`);
  await call("webview_interact", { action: "click", selector: "#open-settings" });
  await sleep(400);
}

// Un réglage fin : le flou change le CSS et est enregistré.
await call("webview_interact", { action: "click", selector: '[data-tab="verre"]' });
await sleep(200);
await js(`(() => { const r = document.getElementById('r-blur'); r.value = 7; r.dispatchEvent(new Event('input', { bubbles: true })); return true })()`);
await sleep(800);
check("le flou est appliqué à l'écran", (await cssVar("--blur")) === "7px");
check("le flou est enregistré par le cœur", (await ipcJson("get_appearance"))?.current?.blur === 7);
check("le préréglage est marqué « modifié »", (await js("(() => document.querySelector('.badge')?.textContent ?? '')()")).includes("modifié"));
await shot("onglet-verre");
for (const tab of ["couleurs", "texte", "fond"]) {
  await call("webview_interact", { action: "click", selector: `[data-tab="${tab}"]` });
  await sleep(300);
  await shot(`onglet-${tab}`);
}

// Le thème reste après redémarrage de l'interface.
await js("(() => { location.reload(); return true })()");
await sleep(1500);
check("le réglage survit au rechargement", (await cssVar("--blur")) === "7px");

// Thème personnel.
await call("webview_interact", { action: "click", selector: "#open-settings" });
await sleep(300);
await call("webview_keyboard", { action: "type", selector: "#theme-name", text: "Notre thème" });
await call("webview_interact", { action: "click", selector: "#save-theme button" });
await sleep(600);
const view = await ipcJson("get_appearance");
const mine = view?.saved?.find((x) => x.name === "Notre thème");
check("thème personnel enregistré", Boolean(mine));

// Export puis import du code de thème.
await call("webview_interact", { action: "click", selector: '[data-tab="partager"]' });
await sleep(500);
const code = (await js("(() => document.getElementById('export-code').value)()")).match(/EVELUCAS-THEME:\{.*\}/s)?.[0];
check("code de thème affiché", Boolean(code));
await shot("onglet-partager");
await ipcJson("apply_preset", { id: "givre" });
await js(`(() => { document.getElementById('import-code').value = ${JSON.stringify(code ?? "")}; return true })()`);
await call("webview_interact", { action: "click", selector: "#import-theme" });
await sleep(500);
check("le code importé redonne le même thème", (await ipcJson("get_appearance"))?.current?.blur === 7);
await js(`(() => { document.getElementById('import-code').value = 'pas un code'; return true })()`);
await call("webview_interact", { action: "click", selector: "#import-theme" });
await sleep(300);
check("un code invalide est refusé avec un message", /invalide/.test(await js("(() => document.getElementById('settings-status').textContent)()")));

// Image de fond : une image générée est enregistrée, choisie, puis retirée.
await js(`(async () => {
  const c = document.createElement('canvas'); c.width = 800; c.height = 500;
  const g = c.getContext('2d');
  const grad = g.createLinearGradient(0, 0, 800, 500);
  grad.addColorStop(0, '#ff9966'); grad.addColorStop(1, '#5e2bff');
  g.fillStyle = grad; g.fillRect(0, 0, 800, 500);
  for (let i = 0; i < 40; i++) { g.fillStyle = 'rgba(255,255,255,' + (i % 5) / 10 + ')'; g.beginPath(); g.arc((i * 137) % 800, (i * 89) % 500, 10 + (i % 7) * 6, 0, 7); g.fill(); }
  await window.__TAURI__.core.invoke('set_wallpaper', { dataUrl: c.toDataURL('image/jpeg', 0.85) });
  return true;
})()`);
await js("(() => { location.reload(); return true })()");
await sleep(1500);
await call("webview_interact", { action: "click", selector: "#open-settings" });
await call("webview_interact", { action: "click", selector: '[data-tab="fond"]' });
await sleep(300);
await js(`(() => { [...document.querySelectorAll('[data-panel="fond"] .choix button')].find(b => b.textContent.includes('Mon image')).click(); return true })()`);
await sleep(800);
check("l'image de fond est affichée", (await js("(() => document.documentElement.hasAttribute('data-image') && document.documentElement.dataset.fond)()")) === "image");
check("le type de fond « image » est enregistré", (await ipcJson("get_appearance"))?.current?.wallpaper === "image");
await shot("fond-image");
await js(`(() => { [...document.querySelectorAll('[data-panel="fond"] button')].find(b => b.textContent.includes("Retirer l'image")).click(); return true })()`);
await sleep(800);
check("l'image de fond est retirée", (await js("(() => document.documentElement.hasAttribute('data-image'))()")) === "false");
check("retour au fond en taches de couleur", (await ipcJson("get_appearance"))?.current?.wallpaper === "mesh");

// Format téléphone (360 px).
await call("webview_interact", { action: "click", selector: '[data-tab="themes"]' });
await call("manage_window", { action: "resize", width: 360, height: 760, logical: true });
await sleep(800);
const overflow = await js("(() => document.documentElement.scrollWidth > window.innerWidth + 1)()");
check("pas de défilement horizontal à 360 px", overflow.includes("false"));
await shot("telephone-reglages");
await call("webview_interact", { action: "click", selector: "#close-settings" });
await sleep(300);
await shot("telephone-notes");
await call("manage_window", { action: "resize", width: 1100, height: 760, logical: true });

// Remise en état.
if (mine) await ipcJson("delete_theme", { id: mine.id });
await ipcJson("apply_preset", { id: "givre" });
for (const id of added) await call("ipc_execute_command", { command: "delete_note", args: { id } });
await js("(() => { location.reload(); return true })()");

const logs = await call("read_logs", { source: "console", level: "error" });
console.log("logs d'erreur console :", logs.slice(0, 400) || "(aucun)");
await call("driver_session", { action: "stop" });
await c.close();
console.log(fails ? `ÉCHECS : ${fails}` : "TOUT EST OK");
process.exit(fails ? 1 : 0);
