//! Depots GitHub via `gh` : son authentification est reutilisee telle quelle,
//! zrepo ne stocke aucun jeton.

use std::{
    fs,
    io::{self, BufRead, IsTerminal, Write},
    process::{Command, Stdio},
    time::Duration,
};

use serde::Deserialize;

use crate::{
    config::Config,
    entry::{Entry, Kind, clean},
    error::{EX_AUTH, EX_DEPS, EX_FAIL, EX_NET, Fail, Res},
    sys,
};

#[derive(Deserialize)]
struct Owner {
    login: String,
}

#[derive(Deserialize)]
struct Repo {
    name: String,
    owner: Owner,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    archived: bool,
    description: Option<String>,
}

/// Depots GitHub, depuis le cache s'il est frais. Un echec laisse le cache
/// precedent intact.
pub fn entries(cfg: &Config, refresh: bool) -> Res<Vec<Entry>> {
    if !refresh
        && cache_is_fresh(cfg)
        && let Ok(text) = fs::read_to_string(&cfg.cache_file)
    {
        return Ok(text.lines().filter_map(Entry::from_tsv).collect());
    }
    let list = fetch()?;
    let mut text: String = list.iter().map(|e| e.to_tsv() + "\n").collect();
    if text.is_empty() {
        text.push('\n');
    }
    let _ = sys::write_atomic(&cfg.cache_file, &text);
    Ok(list)
}

fn cache_is_fresh(cfg: &Config) -> bool {
    fs::metadata(&cfg.cache_file).is_ok_and(|m| {
        m.len() > 0
            && m.modified()
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age < Duration::from_secs(cfg.cache_ttl))
    })
}

/// Tous les depots accessibles au compte : les siens, ceux des orgs, ceux ou
/// il est collaborateur. `gh repo list` ne couvre que les siens, d'ou l'API.
fn fetch() -> Res<Vec<Entry>> {
    let out = Command::new("gh")
        .args(["api", "--paginate", "-X", "GET", "user/repos"])
        .args(["-f", "per_page=100", "-f", "sort=updated"])
        .args(["-f", "affiliation=owner,collaborator,organization_member"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => Fail::new(
                EX_DEPS,
                "gh introuvable. Installer avec : omarchy pkg add github-cli",
            ),
            _ => Fail::new(EX_FAIL, format!("Impossible de lancer gh : {e}")),
        })?;

    if !out.status.success() {
        let emsg = String::from_utf8_lossy(&out.stderr);
        let any = |pats: &[&str]| pats.iter().any(|p| emsg.contains(p));
        return Err(
            if any(&["gh auth login", "authentication token", "HTTP 401"]) {
                Fail::new(
                    EX_AUTH,
                    "GitHub CLI n'est pas authentifié. Lancez : gh auth login",
                )
            } else if any(&[
                "dial tcp",
                "no such host",
                "connection refused",
                "network is unreachable",
                "i/o timeout",
                "lookup api.github.com",
            ]) {
                Fail::new(EX_NET, "GitHub injoignable (pas de réseau ?).")
            } else {
                let detail = emsg.trim();
                Fail::new(
                    EX_FAIL,
                    format!(
                        "Échec de la récupération des dépôts :\n{}",
                        if detail.is_empty() {
                            "erreur inconnue de gh"
                        } else {
                            detail
                        }
                    ),
                )
            },
        );
    }

    // --paginate concatene un tableau JSON par page : `[...][...]`.
    let mut list = Vec::new();
    for page in serde_json::Deserializer::from_slice(&out.stdout).into_iter::<Vec<Repo>>() {
        let page =
            page.map_err(|e| Fail::new(EX_FAIL, format!("Réponse GitHub illisible : {e}")))?;
        list.extend(page.into_iter().map(|r| {
            let desc = clean(r.description.as_deref().unwrap_or(""));
            Entry {
                kind: Kind::Github,
                key: format!("{}/{}", r.owner.login, r.name),
                name: r.name,
                owner: r.owner.login,
                visibility: if r.private { "private" } else { "public" }.into(),
                description: if r.archived {
                    format!("[archive] {desc}")
                } else {
                    desc
                },
                localpath: String::new(),
            }
        }));
    }
    Ok(list)
}

pub fn is_authenticated() -> bool {
    sys::output_ok(Command::new("gh").args(["auth", "status"])).is_some()
}

/// Premier lancement : gh n'est pas encore authentifie. On propose de le faire
/// ici plutot que d'echouer — gh gere seul le navigateur et le jeton.
pub fn ensure_auth_interactive() {
    if !sys::has("gh") || is_authenticated() {
        return;
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return; // pas de terminal : la liste signalera l'erreur
    }
    println!("\n  GitHub CLI n'est pas encore authentifié sur cette machine.");
    println!("  Sans cela, zrepo ne listera que les projets déjà présents dans ~/Projects.\n");
    print!("  Lancer « gh auth login » maintenant ? [O/n] ");
    let _ = io::stdout().flush();
    let mut ans = String::new();
    let _ = io::stdin().lock().read_line(&mut ans);
    if ans.trim_start().starts_with(['n', 'N']) {
        println!();
        return;
    }
    println!();
    if Command::new("gh")
        .args(["auth", "login"])
        .status()
        .is_ok_and(|s| s.success())
    {
        println!("\n  Authentification réussie.\n");
    } else {
        eprintln!("\n  Authentification abandonnée — poursuite avec les projets locaux.\n");
    }
}
