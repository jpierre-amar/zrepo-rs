//! Codes de sortie et type d'erreur commun.
//!
//! Les codes sont ceux de la version Bash : les scripts qui testent `$?`
//! continuent de fonctionner.

pub const EX_OK: i32 = 0;
pub const EX_FAIL: i32 = 1; // erreur generique
pub const EX_DEPS: i32 = 2; // dependance manquante
pub const EX_AUTH: i32 = 3; // gh non authentifie
pub const EX_NET: i32 = 4; // GitHub injoignable
pub const EX_COLLISION: i32 = 6; // dossier occupe par un autre depot
pub const EX_HERDR: i32 = 7; // Herdr indisponible ou agent non demarre
pub const EX_CANCEL: i32 = 130; // annulation utilisateur (convention Omarchy)

/// Echec porteur d'un code de sortie. Le message peut tenir sur plusieurs
/// lignes ; vide, rien n'est affiche (annulation silencieuse).
#[derive(Debug)]
pub struct Fail {
    pub code: i32,
    pub msg: String,
}

impl Fail {
    pub fn new(code: i32, msg: impl Into<String>) -> Self {
        Self {
            code,
            msg: msg.into(),
        }
    }
}

pub type Res<T> = Result<T, Fail>;
