// Test de bout en bout des notes, via le serveur MCP Tauri (hypothesi/mcp-server-tauri).
// Prérequis : l'appli lancée avec `cargo run -p eve-et-lucas-desktop --features mcp-bridge`.
// Puis : `cd apps/desktop/e2e && npm install && npm test`. Attention, le test vide le localStorage.
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
const out = process.argv[2] ?? "capture-e2e.png";
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
await js("(() => { localStorage.clear(); return true })()");
const count = async () => Number((await js("(() => document.querySelectorAll('#notes li').length)()")).match(/\d+/)?.[0] ?? -1);
const n0 = await count();

await call("webview_keyboard", { action: "type", selector: "#title", text: "Test MCP" });
await call("webview_keyboard", { action: "type", selector: "#body", text: "ajoutée par le serveur MCP Tauri" });
await call("webview_interact", { action: "click", selector: "#save" });
await sleep(800);
check("ajout d'une note par l'interface", (await count()) === n0 + 1);
const ipc = await call("ipc_execute_command", { command: "list_notes" });
check("la commande list_notes renvoie la note", ipc.includes("Test MCP"));

await call("webview_interact", { action: "click", selector: "#notes li:first-child button" });
await sleep(300);
check("le formulaire passe en modification", (await js("(() => document.getElementById('save').textContent)()")).includes("Enregistrer"));
await js("(() => { const b = document.getElementById('body'); b.value = ''; return true })()");
await call("webview_keyboard", { action: "type", selector: "#body", text: "modifiée" });
await call("webview_interact", { action: "click", selector: "#save" });
await sleep(800);
check("modification enregistrée", (await js("(() => document.querySelector('#notes li .body')?.textContent)()")).includes("modifiée"));

await call("webview_interact", { action: "click", selector: "#save" });
await sleep(500);
const err = await js("(() => { const e = document.getElementById('error'); return e.hidden ? '' : e.textContent })()");
check("une note vide est refusée avec un message", /titre ou un contenu/.test(err));

await call("webview_screenshot", { format: "png", filePath: out });
await call("webview_interact", { action: "click", selector: "#notes li:first-child button.danger" });
await sleep(800);
check("suppression", (await count()) === n0);

const logs = await call("read_logs", { source: "console", level: "error" });
console.log("logs d'erreur console :", logs.slice(0, 400) || "(aucun)");
await call("driver_session", { action: "stop" });
await c.close();
console.log(fails ? `ÉCHECS : ${fails}` : "TOUT EST OK");
process.exit(fails ? 1 : 0);
