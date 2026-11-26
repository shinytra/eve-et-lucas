// Test de bout en bout des fichiers partagés, via le serveur MCP Tauri.
// Prérequis : l'appli lancée avec `cargo run -p eve-et-lucas-desktop --features mcp-bridge`.
// Le sélecteur de fichiers du système ne se pilote pas : le test envoie le
// contenu directement par la commande `share_file`, comme le fait files.js.
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
const out = process.argv[2] ?? "capture-fichiers.png";
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
const count = async () => Number((await js("(() => document.querySelectorAll('#files li').length)()")).match(/\d+/)?.[0] ?? -1);
const n0 = await count();

// Image PNG 2×2 rouge.
const png = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAFklEQVR4nGP4z8DAwMDAxMDAwMDAAAANHQEDasKb6QAAAABJRU5ErkJggg==";
// Même chemin que files.js : envoi par morceaux en base64.
const share = (name, mime, b64) => js(`(async () => {
  const { invoke } = window.__TAURI__.core;
  try {
    const id = await invoke('upload_start', { name: ${JSON.stringify(name)}, mime: '${mime}', author: 'Eve', size: atob('${b64}').length });
    await invoke('upload_chunk', { id, data: '${b64}' });
    const f = await invoke('upload_finish', { id });
    return 'OK ' + f.hash;
  } catch (e) { return 'ERR ' + e }
})()`);
// L'interface se rafraîchit d'elle-même après un envoi par le sélecteur ;
// ici on recharge la liste en simulant un changement venu de la synchro.
const reload = () => js("(async () => { location.reload(); return true })()");

const r1 = await share("photo d'été.png", "image/png", png);
check("partage d'une image", r1.includes("OK "));
const r2 = await share("../../etc/passwd", "text/plain", btoa("contenu"));
check("un nom avec des dossiers est nettoyé", r2.includes("OK "));
const r3 = await share("   ", "text/plain", btoa("x"));
check("un nom vide est refusé", r3.includes("ERR") && /nom de fichier vide/.test(r3));
const r4 = await js(`(async () => { const { invoke } = window.__TAURI__.core;
  const id = await invoke('upload_start', { name: 'a.txt', mime: '', author: 'Eve', size: 2 });
  try { await invoke('upload_chunk', { id, data: btoa('trop long') }); return 'accepté' } catch (e) { return 'ERR ' + e } })()`);
check("un envoi plus gros qu'annoncé est refusé", r4.includes("plus gros"));
const r5 = await js("(async () => { try { await window.__TAURI__.core.invoke('upload_start', { name: 'x', mime: '', author: 'Eve', size: 600 * 1024 * 1024 }); return 'accepté' } catch (e) { return 'ERR ' + e } })()");
check("un fichier de plus de 500 Mo est refusé", r5.includes("500 Mo"));
await reload();
await sleep(3000);
check("deux fichiers listés", (await count()) === n0 + 2);
const names = await js("(() => [...document.querySelectorAll('#files .name')].map(e => e.textContent).join('|'))()");
check("noms affichés", names.includes("photo d'été.png") && names.includes("passwd") && !names.includes("/"));
check("aperçu de l'image", (await js("(() => { const img = document.querySelector('#files img'); return img ? img.naturalWidth : 0 })()")).includes("2"));
await call("webview_screenshot", { format: "png", filePath: out });

// Le vrai sélecteur de fichiers : un File construit en JS passe par share() de files.js.
const viaUi = await js(`(async () => {
  const dt = new DataTransfer();
  dt.items.add(new File([new Uint8Array(5 * 1024 * 1024).fill(65)], 'gros fichier.bin', { type: 'application/octet-stream' }));
  const input = document.getElementById('file-input');
  input.files = dt.files;
  input.dispatchEvent(new Event('change'));
  return true })()`);
await sleep(4000);
const big = await call("ipc_execute_command", { command: "list_files" });
check("envoi de 5 Mo par l'interface (3 morceaux)", big.includes("gros fichier.bin") && big.includes("5242880"));

const saved = await js(`(async () => {
  const files = await window.__TAURI__.core.invoke('list_files');
  const f = files.find(f => f.name === 'passwd');
  return await window.__TAURI__.core.invoke('save_file', { id: f.id });
})()`);
console.log("enregistré :", saved);
check("enregistrement dans Téléchargements, sans sortir du dossier", /Downloads\/passwd/.test(saved) && !saved.includes(".."));

const target = "[...document.querySelectorAll('#files li')].find(li => li.textContent.includes('passwd'))";
await js(`(() => { ${target}.querySelector('button.danger').click(); return true })()`);
await sleep(800);
check("suppression", (await count()) === n0 + 2);

const logs = await call("read_logs", { source: "console", level: "error" });
console.log("logs d'erreur console :", logs.slice(0, 400) || "(aucun)");
await call("driver_session", { action: "stop" });
await c.close();
console.log(fails ? `ÉCHECS : ${fails}` : "TOUT EST OK");
process.exit(fails ? 1 : 0);
