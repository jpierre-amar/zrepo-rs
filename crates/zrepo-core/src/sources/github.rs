//! Depots GitHub via l'API REST, avec le jeton trouve par `auth::token()`.

use std::{
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Deserialize;

use crate::{
    auth,
    config::Config,
    entry::{Entry, Kind, clean},
    error::{EX_AUTH, EX_FAIL, Fail, Res},
    net, sys,
};

const FIRST_PAGE: &str = "https://api.github.com/user/repos?per_page=100&sort=updated\
                          &affiliation=owner,collaborator,organization_member";

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

/// Depots GitHub, depuis le cache s'il est frais et s'il a ete rempli avec le
/// meme jeton (changement de compte, `gh auth switch`, autre variable
/// d'environnement : le cache d'un autre compte n'est jamais servi). Un echec
/// laisse le cache precedent intact.
pub fn entries(cfg: &Config, refresh: bool) -> Res<Vec<Entry>> {
    if !cfg.github.enabled {
        return Ok(Vec::new());
    }
    let Some((token, _)) = auth::token() else {
        return Err(Fail::new(
            EX_AUTH,
            "Pas de connexion GitHub. Lancez : zrepo-rs setup",
        ));
    };
    let id = account_id(&token);
    if !refresh
        && cache_is_fresh(cfg, &id)
        && let Ok(text) = fs::read_to_string(&cfg.cache_file)
    {
        return Ok(text.lines().filter_map(Entry::from_tsv).collect());
    }
    let list = fetch(&token)?;
    let mut text: String = list.iter().map(|e| e.to_tsv() + "\n").collect();
    if text.is_empty() {
        text.push('\n');
    }
    if sys::write_atomic(&cfg.cache_file, &text).is_ok() {
        let _ = sys::write_atomic(&id_file(&cfg.cache_file), &id);
    }
    Ok(list)
}

/// Fichier qui dit pour quel jeton le cache a ete rempli.
pub(crate) fn id_file(cache: &Path) -> PathBuf {
    let mut name = cache.as_os_str().to_owned();
    name.push(".compte");
    PathBuf::from(name)
}

/// Empreinte non reversible du jeton (jamais le jeton lui-meme sur disque).
/// Si l'algorithme change avec Rust, le cache est simplement rechargé.
fn account_id(token: &str) -> String {
    let mut h = DefaultHasher::new();
    "zrepo-rs-cache".hash(&mut h);
    token.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn cache_is_fresh(cfg: &Config, id: &str) -> bool {
    let same_account = fs::read_to_string(id_file(&cfg.cache_file)).is_ok_and(|s| s.trim() == id);
    same_account
        && fs::metadata(&cfg.cache_file).is_ok_and(|m| {
            m.len() > 0
                && m.modified()
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age < Duration::from_secs(cfg.cache_ttl))
        })
}

/// URL de la page suivante dans l'en-tete Link :
/// `<https://...&page=2>; rel="next", <https://...>; rel="last"`.
fn next_link(header: &str) -> Option<String> {
    header.split(',').find_map(|part| {
        let (url, params) = part.split_once(';')?;
        let is_next = params
            .split(';')
            .any(|p| matches!(p.trim(), "rel=\"next\"" | "rel=next"));
        let url = url.trim().strip_prefix('<')?.strip_suffix('>')?;
        // Le prochain appel réutilise le Bearer : même si l'en-tête vient de
        // GitHub sous TLS, ne jamais laisser une URL externe recevoir le jeton.
        (is_next && url.starts_with("https://api.github.com/")).then(|| url.to_string())
    })
}

fn to_entry(r: Repo) -> Entry {
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
}

/// Tous les depots accessibles au compte : les siens, ceux des orgs, ceux ou
/// il est collaborateur.
fn fetch(token: &str) -> Res<Vec<Entry>> {
    let agent = net::agent();
    let mut list = Vec::new();
    let mut url = Some(FIRST_PAGE.to_string());
    while let Some(u) = url.take() {
        let mut resp = agent
            .get(&u)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .call()
            .map_err(net::fail)?;
        match resp.status().as_u16() {
            200 => {}
            401 => {
                return Err(Fail::new(
                    EX_AUTH,
                    "Jeton GitHub refusé (invalide, expiré ou révoqué). Lancez : zrepo-rs setup",
                ));
            }
            s => {
                let msg = net::api_message(&mut resp);
                let detail = if msg.is_empty() {
                    format!("GitHub a répondu {s}.")
                } else {
                    format!("GitHub a répondu {s} : {msg}")
                };
                return Err(Fail::new(
                    EX_FAIL,
                    format!("Échec de la récupération des dépôts :\n{detail}"),
                ));
            }
        }
        url = resp
            .headers()
            .get("link")
            .and_then(|h| h.to_str().ok())
            .and_then(next_link);
        let page: Vec<Repo> = resp.body_mut().read_json().map_err(net::fail)?;
        list.extend(page.into_iter().map(to_entry));
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_d_un_autre_compte_ignore() {
        let dir = std::env::temp_dir().join(format!("zrepo-rs-cache-{}", std::process::id()));
        let cfg = Config {
            cache_file: dir.join("repos.tsv"),
            ..Config::default()
        };
        sys::write_atomic(&cfg.cache_file, "GITHUB\ta/b\tb\ta\tpublic\t\t\n").unwrap();
        let (a, b) = (account_id("jeton-a"), account_id("jeton-b"));
        assert_ne!(a, b);
        assert!(!a.contains("jeton"));
        assert!(!cache_is_fresh(&cfg, &a)); // pas d'empreinte : rechargement
        sys::write_atomic(&id_file(&cfg.cache_file), &a).unwrap();
        assert!(cache_is_fresh(&cfg, &a));
        assert!(!cache_is_fresh(&cfg, &b));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn en_tete_link() {
        let h = r#"<https://api.github.com/user/repos?per_page=100&page=2>; rel="next", <https://api.github.com/user/repos?per_page=100&page=5>; rel="last""#;
        assert_eq!(
            next_link(h).as_deref(),
            Some("https://api.github.com/user/repos?per_page=100&page=2")
        );
        let last = r#"<https://x/?page=1>; rel="prev", <https://x/?page=1>; rel="first""#;
        assert_eq!(next_link(last), None);
        assert_eq!(next_link(""), None);
        let ordre = r#"<https://x/?page=1>; rel="prev", <https://x/?page=3>; rel="next""#;
        assert_eq!(next_link(ordre), None);
        let externe = r#"<https://example.test/collect>; rel="next""#;
        assert_eq!(next_link(externe), None);
    }
}
