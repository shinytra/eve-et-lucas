// Coût de rendu de l'interface, pour suivre la consommation de batterie.
//
// Ouvre ui/index.html dans Chromium sans carte graphique : tout le rendu
// (flous du verre dépoli compris) passe alors par le processeur, et le temps
// processeur consommé est une bonne image du travail que ferait le GPU du
// téléphone. Aucune donnée : seule l'apparence compte ici.
//
//   NODE_PATH=$(npm root -g) node batterie.mjs [secondes]   (après npm i -g playwright
//   et npx playwright install chromium)
//
// Résultat en % d'un cœur, en moyenne sur la durée, après 3 s de chargement.
import { createRequire } from "node:module";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const { chromium } = createRequire(import.meta.url)("playwright");
const UI = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../ui");
const SECONDES = Number(process.argv[2] || 15);

// Animation des taches à 60 images par seconde, comme avant l'optimisation.
const AVANT = "[data-anime] .tache { animation-timing-function: linear !important; }";
const AVANT_PULSATIONS = ".date.today, .pastille-etat.ok::before { animation-timing-function: ease-in-out !important; }";

const variantes = [
  { nom: "Tâches, fond fixe", ecran: "taches", anime: false },
  { nom: "Tâches, fond animé à 60 i/s (avant)", ecran: "taches", anime: true, css: AVANT },
  { nom: "Tâches, fond animé à 10 i/s (maintenant)", ecran: "taches", anime: true },
  { nom: "Accueil, point « synchro ok » (avant)", ecran: "accueil", anime: false, css: AVANT_PULSATIONS, point: true },
  { nom: "Accueil, point « synchro ok » (maintenant)", ecran: "accueil", anime: false, point: true },
  { nom: "PC 1920×1080, fond animé (avant)", ecran: "taches", anime: true, css: AVANT, w: 1920, h: 1080 },
  { nom: "PC 1920×1080, fond animé (maintenant)", ecran: "taches", anime: true, w: 1920, h: 1080 },
];

/** Temps processeur (s) d'un processus et de tous ses descendants. */
function cpu(racine) {
  const parent = {};
  const ticks = {};
  for (const pid of readdirSync("/proc").filter((d) => /^\d+$/.test(d))) {
    try {
      const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
      const champs = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
      parent[pid] = champs[1];
      ticks[pid] = Number(champs[11]) + Number(champs[12]);
    } catch {
      // Processus terminé entre-temps.
    }
  }
  const arbre = new Set([String(racine)]);
  for (let grandit = true; grandit; ) {
    grandit = false;
    for (const pid of Object.keys(parent)) {
      if (!arbre.has(pid) && arbre.has(parent[pid])) {
        arbre.add(pid);
        grandit = true;
      }
    }
  }
  let total = 0;
  for (const pid of arbre) total += ticks[pid] || 0;
  return total / 100;
}

for (const v of variantes) {
  const serveur = await chromium.launchServer();
  const navigateur = await chromium.connect(serveur.wsEndpoint());
  const page = await navigateur.newPage({ viewport: { width: v.w || 412, height: v.h || 915 } });
  await page.goto(`file://${UI}/index.html`);
  await page.evaluate((v) => {
    const racine = document.documentElement;
    racine.dataset.fond = "mesh";
    racine.toggleAttribute("data-anime", v.anime);
    for (const s of document.querySelectorAll("main > section[data-ecran]")) {
      s.hidden = s.dataset.ecran !== v.ecran;
    }
    if (v.point) {
      const p = document.getElementById("accueil-synchro");
      p.hidden = false;
      p.className = "pastille-etat ok";
      p.textContent = "Synchronisé";
    }
  }, v);
  if (v.css) await page.addStyleTag({ content: v.css });
  await page.waitForTimeout(3000);
  const avant = cpu(serveur.process().pid);
  await page.waitForTimeout(SECONDES * 1000);
  const apres = cpu(serveur.process().pid);
  console.log(`${v.nom.padEnd(45)} ${(((apres - avant) / SECONDES) * 100).toFixed(1)} % d'un cœur`);
  await navigateur.close();
  await serveur.close();
}
