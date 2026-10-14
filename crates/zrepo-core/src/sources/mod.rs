//! Les trois sources de projets et leur fusion : locaux d'abord, puis
//! distants, puis GitHub. Chaque source est independante et peut etre
//! appelee depuis son propre thread.

mod github;
mod local;
pub(crate) mod remote;

use std::collections::HashSet;

use crate::{config::Config, entry::Entry, error::Res};

/// Depots git sous le dossier des projets.
pub fn locals(cfg: &Config) -> Vec<Entry> {
    local::entries(cfg)
}

/// Projets distants (ssh_connections de Zed, plus le dernier scan) et
/// avertissements (serveurs injoignables...).
pub fn remotes(cfg: &Config, refresh: bool) -> (Vec<Entry>, Vec<String>) {
    remote::entries(cfg, refresh)
}

/// Depots du compte GitHub ; vide si GitHub est desactive.
pub fn github(cfg: &Config, refresh: bool) -> Res<Vec<Entry>> {
    github::entries(cfg, refresh)
}

pub(crate) use github::id_file as github_id_file;

/// Un depot GitHub deja clone en local n'apparait pas deux fois.
pub fn combine(locals: &[Entry], remotes: &[Entry], github: &[Entry]) -> Vec<Entry> {
    // Le champ description d'un depot local porte son owner/repo d'origine.
    let cloned: HashSet<String> = locals
        .iter()
        .filter(|e| e.description.contains('/'))
        .map(|e| e.description.to_lowercase())
        .collect();
    locals
        .iter()
        .chain(remotes)
        .chain(
            github
                .iter()
                .filter(|e| !cloned.contains(&e.key.to_lowercase())),
        )
        .cloned()
        .collect()
}

/// Liste complete (sous-commande `list`). Un echec GitHub (hors ligne, non
/// authentifie...) ne prive pas l'utilisateur de ses projets deja clones.
pub fn collect(cfg: &Config, refresh: bool) -> Res<(Vec<Entry>, Vec<String>)> {
    let locals = locals(cfg);
    let (remotes, mut warnings) = remotes(cfg, refresh);
    let gh = match github(cfg, refresh) {
        Ok(g) => g,
        Err(f) if locals.is_empty() => return Err(f),
        Err(f) => {
            warnings.push(f.msg);
            warnings.push("GitHub indisponible — seuls les projets locaux sont listés.".into());
            Vec::new()
        }
    };
    Ok((combine(&locals, &remotes, &gh), warnings))
}
