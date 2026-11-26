// Test de bout en bout des dates importantes, via le serveur MCP Tauri.
// Prérequis : l'appli lancée avec `cargo run -p eve-et-lucas-desktop --features mcp-bridge`.
// Puis : `node dates.mjs [capture.png]`.
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
const out = process.argv[2] ?? "capture-dates.png";
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

console.log(await call("driver_session", { action: "start", host: "127.0.0.1", port: 9223 }));
const count = async () => Number((await js("(() => document.querySelectorAll('#dates li').length)()")).match(/\d+/)?.[0] ?? -1);
const n0 = await count();
const target = (name) => `[...document.querySelectorAll('#dates li')].find(li => li.querySelector('.title').textContent === ${JSON.stringify(name)})`;

// Date qui tombe aujourd'hui, il y a 3 ans : « Aujourd'hui », « 3 ans ».
await call("webview_keyboard", { action: "type", selector: "#date-title", text: "Notre anniversaire MCP" });
await js(`(() => {
  const d = new Date(), p = n => String(n).padStart(2, '0');
  document.getElementById('date-day').value = (d.getFullYear() - 3) + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate());
  document.getElementById('date-kind').value = 'couple';
  return true })()`);
await call("webview_interact", { action: "click", selector: "#date-save" });
await sleep(800);
check("ajout d'une date par l'interface", (await count()) === n0 + 1);
const meta = await js(`(() => ${target("Notre anniversaire MCP")}?.textContent)()`);
check("compte à rebours « Aujourd'hui » et « 3 ans »", /Aujourd'hui/.test(meta) && /3 ans/.test(meta));
check("encart de la prochaine date", /Aujourd'hui !/.test(await js("(() => document.getElementById('date-next').textContent)()")));

// Date unique dans 10 jours, sans compter les années.
await call("webview_keyboard", { action: "type", selector: "#date-title", text: "Concert MCP" });
await js(`(() => {
  const d = new Date(Date.now() + 10 * 86400000), p = n => String(n).padStart(2, '0');
  document.getElementById('date-day').value = d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate());
  document.getElementById('date-yearly').click();
  document.getElementById('date-kind').value = 'autre';
  return document.getElementById('date-count-label').hidden })()`);
await call("webview_interact", { action: "click", selector: "#date-save" });
await sleep(800);
const concert = await js(`(() => ${target("Concert MCP")}?.textContent)()`);
check("date unique : « Dans 10 jours », « une seule fois »", /Dans 10 jours/.test(concert) && /une seule fois/.test(concert));

const ipc = await call("ipc_execute_command", { command: "list_dates" });
check("la commande list_dates renvoie les dates", ipc.includes("Concert MCP") && ipc.includes("Notre anniversaire MCP"));

await call("webview_screenshot", { format: "png", filePath: out });

// Sans date : message d'erreur.
await call("webview_keyboard", { action: "type", selector: "#date-title", text: "Sans date" });
await call("webview_interact", { action: "click", selector: "#date-save" });
await sleep(500);
const err = await js("(() => { const e = document.getElementById('date-error'); return e.hidden ? '' : e.textContent })()");
check("une date sans jour est refusée avec un message", /Choisis une date/.test(err));
await js("(() => { document.getElementById('date-title').value = ''; return true })()");

// Modification.
await js(`(() => { ${target("Concert MCP")}.querySelector('.actions button').click(); return true })()`);
await sleep(300);
check("le formulaire passe en modification", (await js("(() => document.getElementById('date-save').textContent)()")).includes("Enregistrer"));
await js("(() => { document.getElementById('date-title').value = 'Concert MCP modifié'; return true })()");
await call("webview_interact", { action: "click", selector: "#date-save" });
await sleep(800);
check("modification enregistrée", (await call("ipc_execute_command", { command: "list_dates" })).includes("Concert MCP modifié"));

for (const name of ["Concert MCP modifié", "Notre anniversaire MCP"]) {
  await js(`(() => { ${target(name)}.querySelector('button.danger').click(); return true })()`);
  await sleep(800);
}
check("suppression", (await count()) === n0);

const logs = await call("read_logs", { source: "console", level: "error" });
console.log("logs d'erreur console :", logs.slice(0, 400) || "(aucun)");
await call("driver_session", { action: "stop" });
await c.close();
console.log(fails ? `ÉCHECS : ${fails}` : "TOUT EST OK");
process.exit(fails ? 1 : 0);
