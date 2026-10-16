//! Recherche et installation volontaire d'une release stable de zrepo-rs.

use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    config::Config,
    error::{EX_FAIL, Fail, Res},
    net, sys,
};

const API: &str = "https://api.github.com/repos/jpierre-amar/zrepo-rs/releases/latest";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
    pub body: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    fetched_at: u64,
    release: Release,
}

#[derive(Clone, Debug)]
pub struct Status {
    pub installed: String,
    pub available: Option<Release>,
    pub stale: bool,
    pub warning: Option<String>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
fn cache_path(cfg: &Config) -> PathBuf {
    cfg.cache_file.with_file_name("release.json")
}

fn version(tag: &str) -> Option<Vec<u64>> {
    let v = tag.strip_prefix('v').unwrap_or(tag);
    let parts: Option<Vec<u64>> = v.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| p.len() == 3)
}

fn validate(r: &Release) -> Res<()> {
    if r.draft
        || r.prerelease
        || version(&r.tag_name).is_none()
        || r.html_url
            != format!(
                "https://github.com/jpierre-amar/zrepo-rs/releases/tag/{}",
                r.tag_name
            )
        || !r.assets.iter().any(|a| a.name == "SHA256SUMS")
    {
        return Err(Fail::new(
            EX_FAIL,
            "Réponse GitHub invalide : release ou artefacts inattendus.",
        ));
    }
    Ok(())
}

fn select(installed: &str, release: Release, warning: Option<String>) -> Status {
    let available = match (version(&release.tag_name), version(installed)) {
        (Some(new), Some(old)) if new > old => Some(release),
        _ => None,
    };
    Status {
        installed: installed.into(),
        available,
        stale: warning.is_some(),
        warning,
    }
}

pub fn check(cfg: &Config, refresh: bool) -> Res<Status> {
    check_with(cfg, refresh, fetch)
}

fn check_with(cfg: &Config, refresh: bool, fetcher: impl FnOnce() -> Res<Release>) -> Res<Status> {
    let installed = env!("CARGO_PKG_VERSION");
    if !cfg.updates.enabled {
        return Ok(Status {
            installed: installed.into(),
            available: None,
            stale: false,
            warning: None,
        });
    }
    let cached = fs::read_to_string(cache_path(cfg))
        .ok()
        .and_then(|s| serde_json::from_str::<Cached>(&s).ok());
    if !refresh
        && let Some(c) = &cached
        && now().saturating_sub(c.fetched_at) < cfg.updates.cache_ttl
        && validate(&c.release).is_ok()
    {
        return Ok(select(installed, c.release.clone(), None));
    }
    match fetcher().and_then(|release| {
        validate(&release)?;
        Ok(release)
    }) {
        Ok(release) => {
            let cache = Cached {
                fetched_at: now(),
                release: release.clone(),
            };
            if let Ok(text) = serde_json::to_string(&cache) {
                let _ = sys::write_atomic(&cache_path(cfg), &text);
            }
            Ok(select(installed, release, None))
        }
        Err(e) => {
            if let Some(c) = cached
                && validate(&c.release).is_ok()
            {
                Ok(select(installed, c.release, Some(e.msg)))
            } else {
                Err(e)
            }
        }
    }
}

fn fetch() -> Res<Release> {
    let mut resp = net::agent().get(API).call().map_err(net::fail)?;
    if resp.status() != 200 {
        return Err(Fail::new(
            EX_FAIL,
            format!(
                "GitHub : HTTP {} lors de la vérification de version.",
                resp.status()
            ),
        ));
    }
    let release: Release = resp.body_mut().read_json().map_err(net::fail)?;
    validate(&release)?;
    Ok(release)
}

fn asset_name() -> &'static str {
    if cfg!(windows) {
        "install.ps1"
    } else {
        "install.sh"
    }
}

fn expected_hash(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim_start().trim_start_matches('*') == name
            && hash.len() == 64
            && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| hash.to_ascii_lowercase())
    })
}

fn verify(bytes: &[u8], sums: &str, name: &str) -> Res<()> {
    let expected = expected_hash(sums, name).ok_or_else(|| {
        Fail::new(
            EX_FAIL,
            format!("Somme SHA-256 de {name} absente ou invalide."),
        )
    })?;
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != expected {
        return Err(Fail::new(
            EX_FAIL,
            format!("Somme SHA-256 incorrecte pour {name} : installation annulée."),
        ));
    }
    Ok(())
}

fn download(url: &str) -> Res<Vec<u8>> {
    let mut resp = net::agent().get(url).call().map_err(net::fail)?;
    if resp.status() != 200 {
        return Err(Fail::new(
            EX_FAIL,
            format!(
                "Téléchargement impossible : HTTP {} ({url}).",
                resp.status()
            ),
        ));
    }
    resp.body_mut().read_to_vec().map_err(net::fail)
}

/// L'utilisateur doit déclencher cet appel. Le script et l'archive sont vérifiés
/// avant écriture du binaire ; les installateurs conservent config.toml et zrepo.
pub fn install(release: &Release) -> Res<()> {
    validate(release)?;
    let script = asset_name();
    if !release.assets.iter().any(|a| a.name == script) {
        return Err(Fail::new(
            EX_FAIL,
            format!("{script} absent de la release."),
        ));
    }
    let base = format!(
        "https://github.com/jpierre-amar/zrepo-rs/releases/download/{}",
        release.tag_name
    );
    let sums = String::from_utf8(download(&format!("{base}/SHA256SUMS"))?)
        .map_err(|_| Fail::new(EX_FAIL, "SHA256SUMS illisible."))?;
    let bytes = download(&format!("{base}/{script}"))?;
    verify(&bytes, &sums, script)?;
    let current = std::env::current_exe()
        .map_err(|e| Fail::new(EX_FAIL, format!("Binaire actuel introuvable : {e}")))?;
    let dir = current
        .parent()
        .ok_or_else(|| Fail::new(EX_FAIL, "Dossier d'installation introuvable."))?;
    let temp =
        std::env::temp_dir().join(format!("zrepo-rs-update-{}-{script}", std::process::id()));
    fs::write(&temp, bytes)
        .map_err(|e| Fail::new(EX_FAIL, format!("Écriture temporaire impossible : {e}")))?;
    let result = if cfg!(windows) {
        Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&temp)
            .args(["-Yes", "-NoSetup", "-Dir"])
            .arg(dir)
            .args(["-Version", &release.tag_name])
            .status()
    } else {
        Command::new("sh")
            .arg(&temp)
            .args(["--yes", "--no-setup", "--dir"])
            .arg(dir)
            .args(["--version", &release.tag_name])
            .status()
    };
    let _ = fs::remove_file(&temp);
    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(Fail::new(
            EX_FAIL,
            format!(
                "Installation interrompue (code {status}). L'ancienne version reste disponible si la vérification a échoué."
            ),
        )),
        Err(e) => Err(Fail::new(
            EX_FAIL,
            format!("Installateur impossible à lancer : {e}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_et_reponses() {
        assert_eq!(version("v0.4.0"), Some(vec![0, 4, 0]));
        assert!(version("v0.4.0-rc1").is_none());
        let r = Release {
            tag_name: "v0.4.0".into(),
            html_url: "https://github.com/jpierre-amar/zrepo-rs/releases/tag/v0.4.0".into(),
            body: "Notes".into(),
            draft: false,
            prerelease: false,
            assets: vec![Asset {
                name: "SHA256SUMS".into(),
            }],
        };
        assert!(select("0.4.0", r.clone(), None).available.is_none());
        assert!(select("0.3.1", r.clone(), None).available.is_some());
        assert!(select("0.3.1", r.clone(), Some("hors ligne".into())).stale);
        let mut invalid = r.clone();
        invalid.html_url = "https://evil.example".into();
        assert!(validate(&invalid).is_err());
    }
    #[test]
    fn somme_incorrecte() {
        let ok = format!("{:x}  install.sh", Sha256::digest(b"script"));
        assert!(verify(b"script", &ok, "install.sh").is_ok());
        assert!(verify(b"autre", &ok, "install.sh").is_err());
    }

    #[test]
    fn cache_hors_ligne_et_reponse_invalide() {
        let root = std::env::temp_dir().join(format!("zrepo-release-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let cfg = Config {
            cache_file: root.join("repos.tsv"),
            ..Config::default()
        };
        let release = Release {
            tag_name: "v99.0.0".into(),
            html_url: "https://github.com/jpierre-amar/zrepo-rs/releases/tag/v99.0.0".into(),
            body: "Notes".into(),
            draft: false,
            prerelease: false,
            assets: vec![Asset {
                name: "SHA256SUMS".into(),
            }],
        };
        let cached = Cached {
            fetched_at: 1,
            release: release.clone(),
        };
        fs::write(cache_path(&cfg), serde_json::to_string(&cached).unwrap()).unwrap();
        let offline = check_with(&cfg, false, || Err(Fail::new(EX_FAIL, "Hors ligne"))).unwrap();
        assert!(offline.stale);
        assert_eq!(offline.available.unwrap().tag_name, "v99.0.0");
        let mut invalid = release;
        invalid.html_url = "https://example.org".into();
        let fallback = check_with(&cfg, true, || Ok(invalid)).unwrap();
        assert!(fallback.stale);
        assert!(fallback.warning.unwrap().contains("invalide"));
        fs::remove_file(cache_path(&cfg)).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
