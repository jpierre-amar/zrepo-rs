//! Reglages, tous surchargeables par variable d'environnement (memes noms
//! que la version Bash).

use std::{env, path::PathBuf, str::FromStr};

#[derive(Clone, Debug)]
pub struct Config {
    pub projects_dir: PathBuf,
    pub cache_file: PathBuf,
    pub cache_ttl: u64, // secondes
    pub scan_depth: usize,
    /// Serveurs et projets distants : les ssh_connections de Zed, seule
    /// source de verite (pas de liste dupliquee ici).
    pub zed_settings: PathBuf,
    pub remote_cache_file: PathBuf,
    pub remote_scan_depth: usize,
    pub remote_ssh_timeout: u64, // secondes, par hote
    pub herdr_agent_kind: String,
    pub herdr_agent_args: Vec<String>,
    pub herdr_wait: u64, // secondes
}

/// Une variable vide vaut une variable absente, comme `${VAR:-defaut}`.
fn var(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.is_empty())
}

fn num<T: FromStr>(name: &str, default: T) -> T {
    var(name).and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl Config {
    pub fn from_env() -> Self {
        let home = PathBuf::from(var("HOME").unwrap_or_else(|| "/".into()));
        let cache_root = var("XDG_CACHE_HOME").map_or_else(|| home.join(".cache"), PathBuf::from);
        let config_root =
            var("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
        let cache_dir = cache_root.join("zrepo");

        Self {
            projects_dir: var("ZREPO_PROJECTS_DIR")
                .map_or_else(|| home.join("Projects"), PathBuf::from),
            cache_file: cache_dir.join("repos.tsv"),
            remote_cache_file: cache_dir.join("remotes.tsv"),

            cache_ttl: num("ZREPO_CACHE_TTL", 600),
            scan_depth: num("ZREPO_SCAN_DEPTH", 3),
            zed_settings: var("ZREPO_ZED_SETTINGS")
                .map_or_else(|| config_root.join("zed/settings.json"), PathBuf::from),
            remote_scan_depth: num("ZREPO_REMOTE_SCAN_DEPTH", 3),
            remote_ssh_timeout: num("ZREPO_REMOTE_SSH_TIMEOUT", 8),
            herdr_agent_kind: var("ZREPO_HERDR_AGENT").unwrap_or_else(|| "codex".into()),
            herdr_agent_args: var("ZREPO_HERDR_AGENT_ARGS")
                .unwrap_or_else(|| "--yolo".into())
                .split_whitespace()
                .map(String::from)
                .collect(),
            herdr_wait: num("ZREPO_HERDR_WAIT", 20),
        }
    }

    pub fn agent_label(&self) -> String {
        std::iter::once(self.herdr_agent_kind.as_str())
            .chain(self.herdr_agent_args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}
