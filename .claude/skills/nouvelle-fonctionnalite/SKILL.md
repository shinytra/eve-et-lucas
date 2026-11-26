---
name: nouvelle-fonctionnalite
description: Démarche pour ajouter une fonctionnalité à l'appli Eve & Lucas (notes, tâches, fichiers, géofencing, widgets) en respectant l'architecture cœur Rust + Tauri.
---

1. Relis `CLAUDE.md` et repère la ligne de la feuille de route concernée. Une PR = une étape.
2. Cherche d'abord une bibliothèque ou un exemple existant (Context7 pour la doc à jour, dépôts du rapport cités dans `CLAUDE.md`). Ne réécris pas ce qui existe déjà.
3. Données et règles : ajoute-les dans `crates/couple-core`, dans un document Automerge dédié à la fonctionnalité (un fichier `.automerge` par fonctionnalité), avec des tests, dont un test de fusion entre deux appareils.
4. Plomberie : expose le cœur via des commandes Tauri dans `apps/desktop/src-tauri/src/lib.rs` (pas de logique métier dans ce fichier).
5. Interface : `apps/desktop/ui/`, textes en français, utilisable sur téléphone (360 px de large).
6. Code spécifique Android : agent `tauri-android`.
7. Finis avec la skill `verifier`, puis ouvre une PR en français qui dit ce qui a été testé.
