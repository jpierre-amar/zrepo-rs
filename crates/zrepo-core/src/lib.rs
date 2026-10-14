//! zrepo-core — la logique de zrepo-rs, sans interface : sources de projets
//! (locaux, distants, GitHub), configuration, acces GitHub, ouverture dans
//! Zed ou Herdr. Aucun affichage ici : les messages passent par les valeurs
//! de retour ou par le rappel `progress` de `open::open`.

pub mod auth;
pub mod config;
pub mod doctor;
pub mod entry;
pub mod error;
pub mod open;
pub mod sources;

#[cfg(not(windows))]
mod herdr;
mod jsonc;
mod net;
mod sys;
