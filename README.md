# Eve & Lucas

Appli pour notre couple, sur PC et Android : notes partagées, tâches communes avec rappels, partage de fichiers en P2P, sonnerie à l'arrivée à la maison et widgets partagés.

Écrite en Rust avec Tauri 2. Les données restent chez nous : les appareils se synchronisent directement entre eux (iroh + Automerge), et le PC sert de relais toujours allumé.

## État actuel

L'appli PC gère des notes partagées enregistrées en local et tourne en tâche de fond : fermer la fenêtre la cache, l'icône dans la barre des tâches permet de la rouvrir ou de la quitter, et son menu propose « Lancer au démarrage de l'ordinateur ».

Les appareils se synchronisent entre eux : dans la partie « Appareils », « Appairer un appareil » affiche un QR code (valable 10 minutes, une seule fois) à scanner ou à coller dans « Rejoindre avec un code » sur l'autre appareil. Ensuite, chaque modification est envoyée aux autres appareils joignables, et une synchro a lieu au moins toutes les minutes. La feuille de route complète est dans [CLAUDE.md](CLAUDE.md).

## Lancer l'appli PC

1. Installer Rust : https://rustup.rs
2. Sous Windows, WebView2 est déjà présent sur Windows 10/11. Sous Linux, installer `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev`.
3. Depuis ce dossier :

```sh
cargo run -p eve-et-lucas-desktop
```

Les notes sont enregistrées dans le dossier de données de l'appli (`%APPDATA%\fr.eveetlucas.couple` sous Windows, `~/.local/share/fr.eveetlucas.couple` sous Linux).

## Installer l'appli sur Android

Chaque modification de `main` fabrique un APK de test sur GitHub :

1. Onglet **Actions** du dépôt, workflow **Android**, dernière exécution réussie.
2. En bas de la page, télécharger **eve-et-lucas-android-debug** (fichier zip contenant l'APK).
3. Sur le téléphone, ouvrir l'APK et autoriser l'installation depuis cette source.

L'APK est une version de test (signée avec une clé de développement, pour téléphones ARM 64 bits, soit presque tous les téléphones récents). Pour appairer le téléphone, ouvrir « Appareils » sur le PC, puis « Scanner le QR code » sur le téléphone.

## Développer avec Claude Code

Ouvre le dossier dans Claude Code : il propose d'installer les plugins et serveurs MCP déclarés dans `.claude/settings.json` et `.mcp.json` (accepte-les). Le détail de cet outillage est dans [CLAUDE.md](CLAUDE.md). Les serveurs MCP ont besoin de Node.js 20 ou plus.
