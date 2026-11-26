//! Génère les permissions des commandes et rattache le module Android
//! (`android/`) au projet Gradle de l'appli.

const COMMANDS: &[&str] = &[
    "set_home",
    "clear_home",
    "current_position",
    "set_ring_on_arrival",
    "check_permissions",
    "request_permissions",
];

fn main() {
    if let Err(e) = tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .try_build()
    {
        // Un script de build ne peut que s'arrêter : on explique pourquoi.
        eprintln!("Impossible de préparer le plugin de géofencing : {e:#}");
        std::process::exit(1);
    }
}
