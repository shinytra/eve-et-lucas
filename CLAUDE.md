# CLAUDE.md

Appli de couple pour Eve et Lucas, en Rust, pour Android et PC. Lucas valide chaque étape : petites PR, une fonctionnalité à la fois, description de PR en français.

## Architecture

- **Tauri 2** : un seul code pour PC (Windows/Linux) et Android. Interface web dans `apps/desktop/ui/` (HTML/CSS/JS sans étape de build pour l'instant), cœur en Rust.
- **`crates/couple-core`** : toutes les données et règles métier. Ne dépend ni de Tauri ni du réseau. Chaque fonctionnalité a son propre document **Automerge** (notes, tâches, widgets…), dont la racine est une table `id -> élément` avec des identifiants UUID v4. Ainsi deux appareils qui n'ont jamais communiqué peuvent fusionner sans rien perdre (voir `notes.rs`).
- **`apps/desktop/src-tauri`** : plomberie uniquement (`store.rs` : documents sur disque et trait `Store` ; `sync.rs` : nœud iroh en tâche de fond, commandes d'appairage ; `reminders.rs` : notifications des tâches échues via `tauri-plugin-notification` : « en cours » sur téléphone (de retour 15 min après si balayées) et confiées à Android à l'avance, répétées toutes les heures sur PC). Ce qui est propre au PC (icône, lancement au démarrage, instance unique) est dans `src/desktop.rs`, compilé seulement avec `#[cfg(desktop)]` ; les plugins correspondants sont des dépendances réservées au PC dans `Cargo.toml`. Commandes Tauri qui appellent le cœur, enregistrement des documents dans le dossier de données de l'appli (écriture atomique : fichier temporaire puis renommage).
- **Dates importantes** : document `dates` (`couple_core::dates`). Le jour, le mois, l'année et la répétition sont une seule valeur Automerge (`when` = `annuel:2021-06-12`, `annuel:02-29`, `unique:2027-09-05`) pour qu'une fusion ne crée jamais une date impossible ; le 29 février est fêté le 28 les autres années. Le cœur travaille en jours (`Day`, sans fuseau) ; `src/dates.rs` donne la date locale (crate `jiff`, fuseau de l'appareil, Android compris) et affiche les rappels à 9 h, une seule fois par appareil (`rappels-dates.json`), confiés à Android 60 jours à l'avance sur téléphone.
- **Apparence** : thème « verre dépoli » propre à chaque appareil (pas d'Automerge) : `couple_core::appearance` (8 préréglages, bornes, nettoyage des valeurs, codes de thème `EVELUCAS-THEME:`), fichiers `apparence.json` et `fond-ecran.txt` (`src/appearance.rs`), variables CSS posées par `ui/appearance.js`. Chaque écran est un `section.verre` ; ses éléments sont des cartes en `var(--field-bg)` (pas de verre dans le verre).
- **Animés** (`couple_core::animes`, `couple_core::anilist`, `src/animes.rs`, `ui/animes.js`) : document `animes` (`AnimesDoc`) = informations AniList (titre, affiche, nombre d'épisodes…), épisode où l'on s'est arrêté (qui, quand) et statut (`a-voir`, `en-cours`, `en-pause`, `fini`, `abandonne`). Pas de note ni d'avis (choix de Lucas). Le cœur prépare les requêtes GraphQL et lit les réponses ; l'appli les envoie (`reqwest` avec `webpki-roots`, car le vérificateur du système demande une initialisation Java sur Android). Affiches : adresses `https://s4.anilist.co/` seulement, sans redirection, 3 Mo au plus, gardées dans `affiches/<id AniList>.img` (ménage des animés retirés) pour rester visibles hors ligne ; `img-src` n'autorise que `https://s4.anilist.co` (résultats de recherche).
- **`crates/couple-sync`** : synchro P2P avec iroh entre les trois pairs (téléphone de Lucas, téléphone d'Eve, PC). Le PC, allumé environ 95 % du temps, est le pair « boîte aux lettres » qui garde une copie de tout. PC éteint : les téléphones se synchronisent directement entre eux.
  - Identité d'un appareil = sa clé iroh (`device.key` dans le dossier de données). La liste des appareils autorisés est elle-même un document Automerge synchronisé (`DevicesDoc`) : un appareil appairé avec le PC est appris par les autres.
  - Appairage (ALPN `eve-et-lucas/pair/1`) : code `EL1:…` (QR code) à usage unique, valable 10 min, contenant l'adresse de l'hôte et un secret. Synchro (ALPN `eve-et-lucas/sync/1`) : refusée à tout appareil absent de `DevicesDoc`, un flux par document, protocole de synchro d'Automerge.
  - L'appli fournit le stockage via le trait `Store`. Pour un nouveau document partagé : l'ajouter à `doc_names()` et à `generate`/`receive` dans `apps/desktop/src-tauri/src/store.rs`.
  - Fichiers : la liste est le document `files` (`FilesDoc`, nom nettoyé : jamais de dossier), le contenu est rangé par empreinte BLAKE3 avec `iroh-blobs` (dossier `fichiers/`, ménage toutes les 10 min des contenus sans étiquette `fichier-<empreinte>`). Seuls les appareils de `DevicesDoc` peuvent en demander, personne ne peut en pousser. Après chaque synchro, une tâche à part (pour ne pas retarder les documents) télécharge ce que `Store::wanted_blobs` demande (tout sur PC, les images de 10 Mo au plus sur téléphone, le reste à la demande), puis libère les contenus des fichiers supprimés ailleurs (`retain_blobs`). Envoi depuis l'interface par morceaux en base64 (`upload_start`/`upload_chunk`/`upload_finish`) : Android ne transmet pas de binaire brut à Rust. Aperçus en `blob:` (autorisé dans `img-src`, images matricielles seulement). Enregistrement : `disk_name` (noms réservés Windows, 200 octets) ; pas encore sur Android (dossier privé de l'appli, il faudra MediaStore).
  - Réseau `Online` (relais et annuaire DNS publics de n0, chiffré de bout en bout, rien n'est stocké) dans l'appli ; `Local` (connexions directes) dans les tests.
- **Kotlin** : uniquement ce que Rust ne peut pas faire sur Android (géofencing en arrière-plan, widget d'écran d'accueil, service au premier plan), sous forme de plugins Tauri maison dans `plugins/`. Jamais de code à la main dans `gen/android` (régénéré).
  - `plugins/tauri-plugin-geofence` : zone « maison » de 150 m (Google Play Services), réenregistrée au redémarrage du téléphone. Chaque entrée est notée par Android même appli fermée ; l'appli les lit (`peek_arrivals`), les ajoute au document `arrivals` (`ArrivalsDoc`, anti-rebond de 5 min par appareil, ménage après 30 jours) puis les confirme (`ack_arrivals`) : rien n'est perdu si l'enregistrement échoue. Les autres appareils sonnent une fois par arrivée de moins de 15 min (identifiants déjà signalés dans `arrivee.json`). L'identifiant d'une arrivée est créé par Kotlin (`add_with_id`), le même dans le réveil ntfy et dans la synchro.
  - Réveil par ntfy.sh (`couple_core::push`, `src/push.rs`) : document `push` = clé AES-256-GCM commune (créée par le premier appareil, ne voyage que par iroh) + adresse ntfy de chaque appareil (`target:<id>`, https seulement). Message `EL1.` + base64(nonce ‖ chiffré ‖ tag), clair JSON `{"v":1,"kind":"arrival",id,who,device,at}` ; même format en Kotlin (`PushCrypto.kt`, vecteur de test commun dans `push.rs`). Téléphone : UnifiedPush (`org.unifiedpush.android:connector`, appli ntfy à installer) ; Kotlin envoie le réveil en entrant dans la zone et sonne à la réception, appli fermée ; `mark_rung` / `peek_received` évitent la double sonnerie. PC : sujet aléatoire `el-<32 hex>` écouté en continu (`reqwest`, `?since=15m` au démarrage). Un message illisible ou falsifié est ignoré.
  - `plugins/tauri-plugin-home-widget` : widget d'écran d'accueil (AppWidgetProvider + RemoteViews plutôt que Glance : le projet Android généré par Tauri n'a pas le plugin compilateur Compose). L'appli lui envoie le résumé calculé par `couple_core::widget::snapshot` (tâches à faire, dernière arrivée de moins de 12 h) à chaque changement et au moins toutes les minutes (`src/widget.rs`). Une case cochée sur le widget est notée par Android même appli fermée, puis appliquée (`apply_action`, au nom choisi dans « Arrivée à la maison ») et confirmée (`peek_actions`/`ack_actions`).

## Commandes

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                  # tests du cœur
cargo run -p eve-et-lucas-desktop       # lance l'appli PC
cargo run -p eve-et-lucas-desktop --features mcp-bridge   # idem, pilotable par le serveur MCP Tauri
cargo tauri dev --features mcp-bridge   # idem, avec la CLI Tauri (cargo install tauri-cli --version "^2")
cd apps/desktop/e2e && npm install && npm test   # notes + tâches + fichiers + dates + animés de bout en bout via le MCP Tauri (appli lancée avec --features mcp-bridge)
(cd apps/desktop/e2e && npm run test:apparence)   # préréglages, réglages, thèmes, image de fond + captures dans captures/
cargo build -p couple-sync --example peer && (cd apps/desktop/e2e && node pairing.mjs)   # appairage + synchro avec un faux téléphone
EVE_ET_LUCAS_DATA_DIR=/tmp/essai cargo run -p eve-et-lucas-desktop   # appli avec un autre dossier de données
cd apps/desktop/src-tauri && npm ci && npm run tauri -- android init   # (ou cargo tauri android init) génère gen/android (non versionné), besoin du SDK + NDK (NDK_HOME)
cargo tauri android dev                 # appli sur l'émulateur ou un téléphone branché
cargo tauri android build --debug --apk --target aarch64   # APK de test ; la CI le fait (workflow « Android »)
# Version à télécharger : workflow « Version à télécharger » (release.yml, lancé à la main ou par une étiquette vX.Y.Z = version de tauri.conf.json) : installateur Windows NSIS + APK dans une Release GitHub
```

Sous Linux, Tauri a besoin de `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev`.

## Conventions

- Textes de l'interface, messages d'erreur et commentaires en français.
- Pas de `unwrap()` hors des tests. Erreurs du cœur via `couple_core::Error` (thiserror), converties en `String` dans les commandes Tauri.
- Toute nouvelle donnée partagée : tests dans le cœur, dont au moins un test de fusion entre deux appareils. Pour simuler un deuxième appareil, utiliser `fork()`, jamais `clone()` (même identifiant d'appareil Automerge).
- Une entrée illisible venue d'un autre appareil est ignorée, jamais bloquante. Un fichier local illisible est mis de côté (`.corrompu-<date>`) avec un avertissement, sans empêcher l'appli de démarrer.
- Une modification se fait sur une copie du document, enregistrée (avec `sync_all`) avant de remplacer celle en mémoire.
- Les dates sont des millisecondes Unix (`i64`) passées en paramètre au cœur, pour des tests déterministes.
- Formulaires de l'interface en `novalidate` : la validation native de WebKitGTK peut bloquer l'envoi sans message (vu avec `datetime-local`). Les règles sont vérifiées dans le cœur.
- Les fichiers `ui/` sont intégrés au binaire à la compilation : recompiler avant un test de bout en bout.
- CSP : `csp` (versions distribuées) reste stricte. `devCsp` autorise `unsafe-inline`/`unsafe-eval` uniquement pour que le pont MCP injecte ses scripts en développement. Jamais de script inline dans `ui/`.
- L'interface doit rester utilisable à 360 px de large (elle servira telle quelle sur Android).
- Code réservé au téléphone : `#[cfg(mobile)]` côté Rust, dépendance sous `[target.'cfg(any(target_os = "android", target_os = "ios"))'.dependencies]`, permissions dans `capabilities/mobile.json`. Le réseau de ce conteneur bloque le SDK Android : l'APK est construit et vérifié par la CI.
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
   b. ✅ Synchro iroh entre appareils + appairage par QR code (écran « Appareils »).
3. ✅ Appli Android avec la même base (APK construit par la CI, scan du QR code d'appairage ; reste à essayer sur vos téléphones).
4. ✅ Tâches communes et rappels (notifications).
5. ✅ Partage de fichiers P2P (iroh-blobs).
6. ✅ Sonnerie à l'arrivée (géofencing, plugin Kotlin ; reste à essayer sur vos téléphones).
7. a. ✅ Widget d'écran d'accueil (tâches à cocher, dernière arrivée ; reste à essayer sur vos téléphones).
   b. ✅ Réveil des appareils par ntfy, chiffré (reste à essayer sur vos téléphones, avec l'appli ntfy).
8. a. ✅ Dates importantes : anniversaire de couple, anniversaires, fêtes, compte à rebours et rappels.
   b. Prochaine date sur le widget.
9. ✅ Animés regardés ensemble : recherche AniList, affiches gardées hors ligne, épisode où l'on s'est arrêté, statut (recherche réelle à essayer : AniList est bloqué par le réseau du conteneur).
