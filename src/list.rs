//! Fusion des trois sources : locaux d'abord, puis distants, puis GitHub.

use std::collections::HashSet;

use crate::{config::Config, entry::Entry, error::Res, github, local, remote};

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
    let locals = local::entries(cfg);
    let (remotes, mut warnings) = remote::entries(cfg, refresh);
    let gh = match github::entries(cfg, refresh) {
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
