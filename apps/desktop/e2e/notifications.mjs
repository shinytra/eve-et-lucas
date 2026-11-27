// Test de bout en bout de l'écran « Notifications », via le serveur MCP Tauri.
// Prérequis : l'appli lancée avec `cargo run -p eve-et-lucas-desktop --features mcp-bridge`.
// Puis : `node notifications.mjs [dossier des captures]`.
// Coupe puis réactive les notifications des animés (réglage de cet appareil).
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
const outDir = resolve(process.argv[2] ?? "captures");
mkdirSync(outDir, { recursive: true });
const t = new StdioClientTransport({ command: "node_modules/.bin/mcp-server-tauri", args: [] });
const c = new Client({ name: "e2e-notifications", version: "1" }); await c.connect(t);
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
const reglages = async () => JSON.parse((await call("ipc_execute_command", { command: "notification_settings", args: {} })).match(/\[[\s\S]*\]/)?.[0] ?? "[]");

console.log(await call("driver_session", { action: "start", host: "127.0.0.1", port: 9223 }));
await call("manage_window", { action: "resize", width: 1100, height: 760, logical: true });
await js("(() => { location.hash = '#notifications'; location.reload(); return true })()");
await sleep(1500);
check("l'écran Notifications est affiché", (await js("(() => getComputedStyle(document.getElementById('ecran-notifications')).display)()")) !== "none");
check("un interrupteur par type (7)", (await js("(() => document.querySelectorAll('#notif-list input[type=checkbox]').length)()")) === "7");
check("tout est activé au départ", (await js("(() => [...document.querySelectorAll('#notif-list input')].every(i => i.checked))()")) === "true");

await call("webview_interact", { action: "click", selector: '#notif-list input[data-kind="anime"]' });
await sleep(500);
let r = await reglages();
check("animés coupés : enregistré", r.find(k => k.code === "anime")?.enabled === false && r.find(k => k.code === "note")?.enabled === true);
await js("(() => { location.reload(); return true })()");
await sleep(1500);
check("toujours coupé après rechargement", (await js("(() => document.querySelector('#notif-list input[data-kind=\"anime\"]').checked)()")) === "false");

await call("manage_window", { action: "resize", width: 380, height: 780, logical: true });
await sleep(600);
check("pas de défilement horizontal à 380 px", (await js("(() => document.documentElement.scrollWidth <= window.innerWidth)()")) === "true");
await call("webview_screenshot", { format: "png", filePath: `${outDir}/notifications-telephone.png` });
await call("webview_interact", { action: "click", selector: '#notif-list input[data-kind="anime"]' });
await sleep(500);
r = await reglages();
check("animés réactivés", r.every(k => k.enabled));
await call("manage_window", { action: "resize", width: 1100, height: 760, logical: true });
await sleep(400);
await call("webview_screenshot", { format: "png", filePath: `${outDir}/notifications-pc.png` });

await call("driver_session", { action: "stop" });
await c.close();
console.log(fails ? `${fails} échec(s)` : "Tout est bon");
process.exit(fails ? 1 : 0);
