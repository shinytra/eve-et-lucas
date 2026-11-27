//! Génère les permissions (aucune commande n'est exposée à l'interface) et
//! rattache le module Android (`android/`) au projet Gradle de l'appli.

/// Vide : l'installation est demandée uniquement par le code Rust de l'appli.
const COMMANDS: &[&str] = &[];

fn main() {
    if let Err(e) = tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .try_build()
    {
        // Un script de build ne peut que s'arrêter : on explique pourquoi.
        eprintln!("Impossible de préparer le plugin d'installation des mises à jour : {e:#}");
        std::process::exit(1);
    }
}
