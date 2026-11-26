---
name: verifier
description: Vérifie que le dépôt est propre avant un commit ou une PR (format, clippy, tests). À utiliser après chaque modification de code Rust.
---

Lance, depuis la racine du dépôt, dans cet ordre, et corrige chaque erreur avant de passer à la suivante :

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo clippy --workspace --all-targets --features eve-et-lucas-desktop/mcp-bridge -- -D warnings`
4. `cargo test --workspace`

Si l'appli PC a changé côté interface, lance-la avec `cargo run -p eve-et-lucas-desktop --features mcp-bridge` et utilise le serveur MCP `tauri` (`driver_session`, `webview_screenshot`, `read_logs`) pour vérifier l'écran concerné.

Résume ensuite en une phrase ce qui passe et ce qui échoue, sans enjoliver.
