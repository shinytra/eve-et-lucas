# Eve & Lucas

Appli pour notre couple, sur PC et Android : notes partagées, tâches communes avec rappels, partage de fichiers en P2P, sonnerie à l'arrivée à la maison et widgets partagés.

Écrite en Rust avec Tauri 2. Les données restent chez nous : les appareils se synchronisent directement entre eux (iroh + Automerge), et le PC sert de relais toujours allumé.

## État actuel

L'appli PC gère des notes partagées enregistrées en local et tourne en tâche de fond : fermer la fenêtre la cache, l'icône dans la barre des tâches permet de la rouvrir ou de la quitter, et son menu propose « Lancer au démarrage de l'ordinateur ».

La partie « Tâches » permet de noter ce qu'il y a à faire, pour l'un, l'autre ou les deux, avec un rappel à une date et une heure. À l'échéance, une notification s'affiche sur chaque appareil : sur téléphone elle reste affichée jusqu'à ce que la tâche soit cochée (si on la balaie, elle revient 15 minutes plus tard), sur PC elle revient toutes les heures.

La partie « Fichiers partagés » envoie des fichiers (500 Mo au plus) directement d'un appareil à l'autre, sans serveur. Le PC garde une copie de tout. Les téléphones téléchargent d'office les images, et le reste quand on appuie sur « Télécharger ». Sur PC, « Enregistrer » range le fichier dans le dossier Téléchargements (sur téléphone, ça viendra plus tard).

La partie « Arrivée à la maison » (sur téléphone) : choisir à qui est le téléphone, puis, une fois chez soi, appuyer sur « Je suis à la maison : enregistrer ici » et accepter la position « Toujours autoriser ». Ensuite, dès que le téléphone entre dans un rayon de 150 m autour de la maison, même appli fermée, les autres appareils sonnent « Eve arrive à la maison » (ou Lucas). Pour que les téléphones sonnent même appli fermée, installer l'appli **ntfy** (Play Store ou F-Droid) sur chacun : le message de réveil passe par le serveur public ntfy.sh, chiffré (ntfy.sh ne voit qu'un texte illisible). Le PC écoute aussi ntfy.sh quand l'appli tourne en tâche de fond.

Sur téléphone, un widget « Eve & Lucas » peut être posé sur l'écran d'accueil (appui long sur l'écran, puis Widgets) : il montre les tâches à faire et la dernière arrivée à la maison. Cocher une tâche sur le widget fonctionne même appli fermée ; c'est enregistré à la prochaine ouverture de l'appli.

Les appareils se synchronisent entre eux : dans la partie « Appareils », « Appairer un appareil » affiche un QR code (valable 10 minutes, une seule fois) à scanner ou à coller dans « Rejoindre avec un code » sur l'autre appareil. Ensuite, chaque modification est envoyée aux autres appareils joignables, et une synchro a lieu au moins toutes les minutes. La feuille de route complète est dans [CLAUDE.md](CLAUDE.md).

## Lancer l'appli PC

1. Installer Rust : https://rustup.rs
2. Sous Windows, WebView2 est déjà présent sur Windows 10/11. Sous Linux, installer `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev`.
3. Depuis ce dossier :

```sh
cargo run -p eve-et-lucas-desktop
```

Les notes sont enregistrées dans le dossier de données de l'appli (`%APPDATA%\fr.eveetlucas.couple` sous Windows, `~/.local/share/fr.eveetlucas.couple` sous Linux).

## Installer l'appli

Les fichiers à installer sont sur la page **[Releases](https://github.com/shinytra/eve-et-lucas/releases/latest)** du dépôt :

- **PC Windows** : `Eve-et-Lucas-…-Windows.exe`. Windows affiche « Windows a protégé votre ordinateur » (SmartScreen) car l'appli n'est pas signée par un éditeur payant : **Informations complémentaires**, puis **Exécuter quand même**.
- **Android** : `Eve-et-Lucas-…-Android.apk`, à ouvrir sur le téléphone en autorisant l'installation depuis cette source. Installer aussi l'appli **ntfy** (Play Store ou F-Droid). Pour téléphones ARM 64 bits (presque tous les téléphones récents). Les versions suivantes s'installent par-dessus, sans désinstaller.

Pour appairer le téléphone, ouvrir « Appareils » sur le PC, puis « Scanner le QR code » sur le téléphone.

### Publier une nouvelle version

1. Monter `version` dans `apps/desktop/src-tauri/tauri.conf.json` (et `Cargo.toml` de l'appli).
2. Onglet **Actions**, workflow **Version à télécharger**, **Run workflow** (ou pousser une étiquette `vX.Y.Z`). Il construit l'installateur Windows et l'APK, puis crée la Release `vX.Y.Z`.

Pour que le téléphone accepte une mise à jour sans désinstaller l'appli, l'APK est signé avec la même clé à chaque version. Elle est rangée dans deux secrets du dépôt (Settings, Secrets and variables, Actions) : `ANDROID_KEYSTORE` (fichier `.jks` en base64, alias `eve-et-lucas`) et `ANDROID_KEYSTORE_PASSWORD`. Sans eux, la publication échoue (une PR signe avec une clé jetable). Pour créer une clé :

```sh
keytool -genkeypair -keystore eve-et-lucas.jks -storetype PKCS12 -alias eve-et-lucas \
  -keyalg RSA -keysize 4096 -validity 10000 -dname "CN=Eve et Lucas"
base64 -w0 eve-et-lucas.jks   # contenu du secret ANDROID_KEYSTORE
```

Garder une copie de cette clé : si elle est perdue, il faudra désinstaller l'appli une fois sur chaque téléphone pour passer à une nouvelle clé. Android refuse aussi une mise à jour dont la version est plus ancienne : toujours monter `version`.

Chaque modification de `main` fabrique aussi un APK de test dans l'onglet **Actions** (workflow **Android**, fichier **eve-et-lucas-android-debug**, gardé 14 jours).

## Développer avec Claude Code

Ouvre le dossier dans Claude Code : il propose d'installer les plugins et serveurs MCP déclarés dans `.claude/settings.json` et `.mcp.json` (accepte-les). Le détail de cet outillage est dans [CLAUDE.md](CLAUDE.md). Les serveurs MCP ont besoin de Node.js 20 ou plus.
