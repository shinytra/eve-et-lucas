//! Génère les permissions (aucune commande n'est exposée à l'interface) et
//! rattache le module Android (`android/`) au projet Gradle de l'appli.

/// Vide : les partages reçus sont lus uniquement par le code Rust de l'appli.
const COMMANDS: &[&str] = &[];

fn main() {
    if let Err(e) = tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .try_build()
    {
        // Un script de build ne peut que s'arrêter : on explique pourquoi.
        eprintln!("Impossible de préparer le plugin du menu Partager : {e:#}");
        std::process::exit(1);
    }
}
