//! Génère les permissions des commandes et rattache le module Android
//! (`android/`) au projet Gradle de l'appli.

// Seules les commandes appelables depuis l'interface sont listées. Celles
// réservées au code Rust de l'appli (`peek_arrivals`, `ack_arrivals`,
// `set_push_config`, `register_push`, `peek_received`, `ack_received`,
// `mark_rung`) passent directement par `run_mobile_plugin`, sans commande
// Tauri ni permission : l'interface ne peut pas les appeler.
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
