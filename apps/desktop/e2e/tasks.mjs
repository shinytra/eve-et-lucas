// Test de bout en bout des tâches, via le serveur MCP Tauri (hypothesi/mcp-server-tauri).
// Prérequis : l'appli lancée avec `cargo run -p eve-et-lucas-desktop --features mcp-bridge`.
// Puis : `node tasks.mjs [capture.png]`. Pour vérifier aussi la notification sous
// Linux, lancer en parallèle `dbus-monitor "interface='org.freedesktop.Notifications'"`.
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
const out = process.argv[2] ?? "capture-taches.png";
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
const count = async () => Number((await js("(() => document.querySelectorAll('#tasks li').length)()")).match(/\d+/)?.[0] ?? -1);
const first = (expr) => js(`(() => { const li = document.querySelector('#tasks li'); return ${expr} })()`);
const n0 = await count();

// Tâche avec un rappel déjà passé : elle doit apparaître « en retard » et déclencher la notification.
await call("webview_keyboard", { action: "type", selector: "#task-title", text: "Tâche MCP" });
await js(`(() => {
  document.getElementById('task-assignee').value = 'Eve';
  const d = new Date(Date.now() - 60000), p = n => String(n).padStart(2, '0');
  document.getElementById('task-due').value = d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate()) + 'T' + p(d.getHours()) + ':' + p(d.getMinutes());
  return true })()`);
await call("webview_interact", { action: "click", selector: "#task-save" });
await sleep(800);
check("ajout d'une tâche par l'interface", (await count()) === n0 + 1);
check("tâche en retard affichée en premier", /En retard/.test(await first("li.querySelector('.meta').textContent")) && /Pour Eve/.test(await first("li.querySelector('.meta').textContent")));
check("classe « late »", (await first("li.classList.contains('late')")).includes("true"));
const ipc = await call("ipc_execute_command", { command: "list_tasks" });
check("la commande list_tasks renvoie la tâche", ipc.includes("Tâche MCP"));

await call("webview_screenshot", { format: "png", filePath: out });

await call("webview_interact", { action: "click", selector: "#tasks li:first-child input[type=checkbox]" });
await sleep(800);
const doneMeta = await js("(() => [...document.querySelectorAll('#tasks li')].find(li => li.textContent.includes('Tâche MCP'))?.querySelector('.meta').textContent)()");
check("tâche cochée : « Faite par Lucas »", /Faite par Lucas/.test(doneMeta));

await call("webview_interact", { action: "click", selector: "#task-save" });
await sleep(500);
const err = await js("(() => { const e = document.getElementById('task-error'); return e.hidden ? '' : e.textContent })()");
check("une tâche sans titre est refusée avec un message", /doit avoir un titre/.test(err));

const target = "[...document.querySelectorAll('#tasks li')].find(li => li.textContent.includes('Tâche MCP'))";
await js(`(() => { ${target}.querySelector('.actions button').click(); return true })()`);
await sleep(300);
check("le formulaire passe en modification", (await js("(() => document.getElementById('task-save').textContent)()")).includes("Enregistrer"));
await js("(() => { document.getElementById('task-title').value = 'Tâche MCP modifiée'; document.getElementById('task-due').value = ''; return true })()");
await call("webview_interact", { action: "click", selector: "#task-save" });
await sleep(800);
const afterEdit = await call("ipc_execute_command", { command: "list_tasks" });
check("modification enregistrée", afterEdit.includes("Tâche MCP modifiée"));

await js(`(() => { ${target}.querySelector('button.danger').click(); return true })()`);
await sleep(800);
check("suppression", (await count()) === n0);

const logs = await call("read_logs", { source: "console", level: "error" });
console.log("logs d'erreur console :", logs.slice(0, 400) || "(aucun)");
await call("driver_session", { action: "stop" });
await c.close();
console.log(fails ? `ÉCHECS : ${fails}` : "TOUT EST OK");
process.exit(fails ? 1 : 0);
