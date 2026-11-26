//! Commandes appelables depuis l'interface (`plugin:home-widget|…`).
//!
//! Aucune pour l'instant, volontairement : comme `peek_arrivals` et
//! `ack_arrivals` du géofencing, la mise à jour du widget et le traitement des
//! actions faites dessus sont l'affaire du code Rust de l'appli
//! ([`crate::HomeWidget`]), qui seul sait appliquer une action aux documents
//! partagés.
