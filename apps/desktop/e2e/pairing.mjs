// Test de bout en bout de l'appairage et de la synchro, via le serveur MCP Tauri.
// Prérequis : l'appli lancée avec `cargo run -p eve-et-lucas-desktop --features mcp-bridge`
// et le faux téléphone compilé (`cargo build -p couple-sync --example peer`).
// Puis : `cd apps/desktop/e2e && npm install && node pairing.mjs`.
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { execFileSync } from "node:child_process";

const peerBin = process.env.PEER_BIN ?? "../../../target/debug/examples/peer";
const out = process.argv[2] ?? "capture-appairage.png";
const t = new StdioClientTransport({ command: "node_modules/.bin/mcp-server-tauri", args: [] });
const c = new Client({ name: "e2e", version: "1" });
await c.connect(t);
let fails = 0;
async function call(name, args) {
  const r = await c.callTool({ name, arguments: args });
  const txt = (r.content || []).filter((x) => x.type === "text").map((x) => x.text).join("\n");
  if (r.isError) { console.log(`✗ ${name}: ${txt.slice(0, 300)}`); fails++; }
  return txt;
}
function check(label, cond) { console.log(`${cond ? "✓" : "✗"} ${label}`); if (!cond) fails++; }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const js = (script) => call("webview_execute_js", { script });

await call("driver_session", { action: "start", host: "127.0.0.1", port: 9223 });
await call("ipc_execute_command", { command: "add_note", args: { author: "Lucas", title: "Écrite sur le PC", body: "" } });

// L'utilisateur clique sur « Appairer un appareil ».
await call("webview_interact", { action: "click", selector: "#pair-start" });
await sleep(500);
const code = (await js("(() => document.getElementById('pair-code').value)()")).match(/EL1:[A-Za-z0-9_-]+/)?.[0];
check("un code d'appairage est affiché", !!code);
check("le QR code est affiché", (await js("(() => !!document.querySelector('#pair-qr svg'))()")).includes("true"));

// Le « téléphone » rejoint avec ce code.
const peerOut = execFileSync(peerBin, [code, "Écrite sur le téléphone"], { encoding: "utf8", timeout: 60000 });
console.log(peerOut.trim().split("\n").map((l) => "   " + l).join("\n"));
check("le téléphone reçoit la note du PC", peerOut.includes("Écrite sur le PC"));
check("le téléphone connaît le PC", /appareil : PC/.test(peerOut));

await sleep(1500);
const notes = await call("ipc_execute_command", { command: "list_notes" });
check("le PC reçoit la note du téléphone", notes.includes("Écrite sur le téléphone"));
const shown = await js("(() => document.getElementById('notes').textContent)()");
check("l'interface du PC s'est rafraîchie toute seule", shown.includes("Écrite sur le téléphone"));
const devices = await js("(() => document.getElementById('devices').textContent)()");
check("le téléphone apparaît dans les appareils", devices.includes("Téléphone de test"));

// Le code ne sert qu'une fois.
let reused = "";
try { reused = execFileSync(peerBin, [code], { encoding: "utf8", timeout: 60000, stdio: "pipe" }); } catch (e) { reused = String(e.stderr || e); }
check("le même code est refusé une seconde fois", /refus/i.test(reused));

await call("webview_interact", { action: "click", selector: "#pair-cancel" });
await js("(() => { document.getElementById('devices-title').scrollIntoView(); return true })()");
await call("webview_screenshot", { format: "png", filePath: out });
const logs = await call("read_logs", { source: "console", level: "error" });
check("aucune erreur dans la console", !logs.trim() || /no .*log|aucun|\[\]/i.test(logs));
if (logs.trim()) console.log("   console :", logs.slice(0, 300));
await call("driver_session", { action: "stop" });
await c.close();
console.log(fails ? `ÉCHECS : ${fails}` : "TOUT EST OK");
process.exit(fails ? 1 : 0);
