# Eve & Lucas

Appli pour notre couple, sur PC et Android : notes partagées, tâches communes avec rappels, partage de fichiers en P2P, sonnerie à l'arrivée à la maison et widgets partagés.

Écrite en Rust avec Tauri 2. Les données restent chez nous : les appareils se synchronisent directement entre eux (iroh + Automerge), et le PC sert de relais toujours allumé.

## État actuel

Première étape : l'appli PC avec des notes partagées enregistrées en local. La synchro entre appareils arrive à l'étape suivante. La feuille de route complète est dans [CLAUDE.md](CLAUDE.md).

## Lancer l'appli PC

1. Installer Rust : https://rustup.rs
2. Sous Windows, WebView2 est déjà présent sur Windows 10/11. Sous Linux, installer `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev`.
3. Depuis ce dossier :

```sh
cargo run -p eve-et-lucas-desktop
```

Les notes sont enregistrées dans le dossier de données de l'appli (`%APPDATA%\fr.eveetlucas.couple` sous Windows, `~/.local/share/fr.eveetlucas.couple` sous Linux).

## Développer avec Claude Code

Ouvre le dossier dans Claude Code : il propose d'installer les plugins et serveurs MCP déclarés dans `.claude/settings.json` et `.mcp.json` (accepte-les). Le détail de cet outillage est dans [CLAUDE.md](CLAUDE.md). Les serveurs MCP ont besoin de Node.js 20 ou plus.
