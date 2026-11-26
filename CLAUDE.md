# CLAUDE.md

Appli de couple pour Eve et Lucas, en Rust, pour Android et PC. Lucas valide chaque étape : petites PR, une fonctionnalité à la fois, description de PR en français.

## Architecture

- **Tauri 2** : un seul code pour PC (Windows/Linux) et Android. Interface web dans `apps/desktop/ui/` (HTML/CSS/JS sans étape de build pour l'instant), cœur en Rust.
- **`crates/couple-core`** : toutes les données et règles métier. Ne dépend ni de Tauri ni du réseau. Chaque fonctionnalité a son propre document **Automerge** (notes, tâches, widgets…), dont la racine est une table `id -> élément` avec des identifiants UUID v4. Ainsi deux appareils qui n'ont jamais communiqué peuvent fusionner sans rien perdre (voir `notes.rs`).
- **`apps/desktop/src-tauri`** : plomberie uniquement. Ce qui est propre au PC (icône, lancement au démarrage, instance unique) est dans `src/desktop.rs`, compilé seulement avec `#[cfg(desktop)]` ; les plugins correspondants sont des dépendances réservées au PC dans `Cargo.toml`. Commandes Tauri qui appellent le cœur, enregistrement des documents dans le dossier de données de l'appli (écriture atomique : fichier temporaire puis renommage).
- **Synchro (à venir)** : iroh 1.0 entre les trois pairs (téléphone de Lucas, téléphone d'Eve, PC). Le PC, allumé environ 95 % du temps, est le pair « boîte aux lettres » qui garde une copie de tout et héberge ntfy pour réveiller les téléphones. PC éteint : les téléphones se synchronisent directement entre eux.
- **Kotlin (à venir)** : uniquement ce que Rust ne peut pas faire sur Android (géofencing en arrière-plan, widgets Glance, service au premier plan), sous forme de plugins Tauri maison dans `plugins/`. Jamais de code à la main dans `gen/android` (régénéré).

## Commandes

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                  # tests du cœur
cargo run -p eve-et-lucas-desktop       # lance l'appli PC
cargo run -p eve-et-lucas-desktop --features mcp-bridge   # idem, pilotable par le serveur MCP Tauri
cargo tauri dev --features mcp-bridge   # idem, avec la CLI Tauri (cargo install tauri-cli --version "^2")
cd apps/desktop/e2e && npm install && npm test   # test de bout en bout via le MCP Tauri (appli lancée avec --features mcp-bridge)
cargo tauri android init                # plus tard : génère le projet Android
cargo tauri android dev
```

Sous Linux, Tauri a besoin de `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev`.

## Conventions

- Textes de l'interface, messages d'erreur et commentaires en français.
- Pas de `unwrap()` hors des tests. Erreurs du cœur via `couple_core::Error` (thiserror), converties en `String` dans les commandes Tauri.
- Toute nouvelle donnée partagée : tests dans le cœur, dont au moins un test de fusion entre deux appareils. Pour simuler un deuxième appareil, utiliser `fork()`, jamais `clone()` (même identifiant d'appareil Automerge).
- Une entrée illisible venue d'un autre appareil est ignorée, jamais bloquante. Un fichier local illisible est mis de côté (`.corrompu-<date>`) avec un avertissement, sans empêcher l'appli de démarrer.
- Une modification se fait sur une copie du document, enregistrée (avec `sync_all`) avant de remplacer celle en mémoire.
- Les dates sont des millisecondes Unix (`i64`) passées en paramètre au cœur, pour des tests déterministes.
- CSP : `csp` (versions distribuées) reste stricte. `devCsp` autorise `unsafe-inline`/`unsafe-eval` uniquement pour que le pont MCP injecte ses scripts en développement. Jamais de script inline dans `ui/`.
- L'interface doit rester utilisable à 360 px de large (elle servira telle quelle sur Android).
- Avant un commit : skill `verifier` (fmt, clippy, tests). La CI GitHub fait la même chose.

## Outillage Claude Code du dépôt

Déclaré dans `.claude/settings.json` et `.mcp.json` ; Claude Code propose de l'installer à l'ouverture du dépôt.

- **Plugins officiels** (anthropics/claude-plugins-official) : `rust-analyzer-lsp` (erreurs et types Rust en direct), `kotlin-lsp` (modules Android), `context7` (doc à jour de Tauri 2, iroh, Automerge : à consulter avant d'utiliser une API récente), `feature-dev`, `pr-review-toolkit`, `commit-commands`, `claude-md-management`, `security-guidance`, `frontend-design`, `skill-creator`.
- **Plugins communautaires** (wshobson/agents) : `systems-programming` (agent Rust), `multi-platform-apps` (mobile + PC), `tdd-workflows`.
- **Serveurs MCP** (versions épinglées, à mettre à jour avec `tauri-plugin-mcp-bridge`) : `tauri` (hypothesi/mcp-server-tauri : captures, clics, logs et IPC de l'appli qui tourne ; le plugin `tauri-plugin-mcp-bridge` n'est compilé qu'avec `--features mcp-bridge` et n'écoute qu'en local. Il n'a pas d'authentification : ne jamais distribuer un build avec cette option) et `mobile` (mobile-next/mobile-mcp : piloter l'émulateur ou un téléphone Android).
- **Hook** : `cargo fmt` après chaque modification de fichier.
- **Agent du projet** : `tauri-android` (`.claude/agents/`).
- **Skills du projet** : `verifier`, `nouvelle-fonctionnalite` (`.claude/skills/`).
- **GitHub** : `@claude` dans une issue ou une PR (`.github/workflows/claude.yml`, anthropics/claude-code-action).

## Dépôts de référence à réutiliser

Plutôt que de réécrire, s'inspirer de (ou dépendre de) :

- Synchro : `n0-computer/iroh`, `iroh-blobs` (fichiers et images), `iroh-gossip` (mises à jour en direct), `automerge/automerge`, `automerge/autosurgeon`.
- Tauri : `tauri-apps/plugins-workspace` (notification, geolocation, fs, store…), `Choochmeque/tauri-plugin-notifications` (rappels de tâches).
- Android : `home-assistant/android` et `owntracks/android` (géofencing), `s00d/tauri-plugin-widgets` (widgets).
- Architecture : `AppFlowy-IO/AppFlowy` (cœur Rust partagé PC + mobile).
- Plan B si Tauri coince sur Android : `mozilla/uniffi-rs` + interface Kotlin/Compose.

## Feuille de route

1. ✅ Base Tauri 2 + notes partagées en local.
2. a. ✅ Appli PC en tâche de fond : icône dans la barre des tâches, fermer la fenêtre la cache, lancement au démarrage (option du menu de l'icône, démarre fenêtre cachée), une seule instance.
   b. Synchro iroh entre appareils + appairage par QR code.
3. Appli Android avec la même base.
4. Tâches communes et rappels (notifications).
5. Partage de fichiers P2P (iroh-blobs).
6. Sonnerie à l'arrivée (géofencing, plugin Kotlin).
7. Widgets interactifs partagés (Glance, plugin Kotlin), réveil par ntfy.
