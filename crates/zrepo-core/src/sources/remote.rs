//! Projets distants, declares dans zrepo-rs ou Zed et ouverts via ssh://.
//!
//! Toutes les lectures de settings.json echouent en silence : un fichier
//! illisible doit couter les entrees distantes, jamais le launcher entier.

use std::{collections::HashSet, fs, process::Stdio, thread};

use serde_json::Value;

use crate::{
    config::Config,
    entry::{Entry, Kind},
    jsonc, sys,
};

pub struct Conn {
    pub host: String,
    pub label: String,
    pub paths: Vec<String>,
}

pub enum Settings {
    Absent,
    Invalid(String),
    Loaded(Vec<Conn>),
}

/// Alias explicites de ~/.ssh/config. Les motifs Host (*, ?, !) ne sont pas
/// des serveurs individuels et ne peuvent pas etre proposes dans l'interface.
pub fn parse_ssh_hosts(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    text.lines()
        .filter_map(|line| {
            let line = line.split('#').next()?.trim();
            let (key, values) = line.split_once(char::is_whitespace)?;
            key.eq_ignore_ascii_case("Host").then_some(values)
        })
        .flat_map(str::split_whitespace)
        .filter(|host| !host.contains(['*', '?', '!']) && !host.starts_with('-'))
        .filter(|host| seen.insert(host.to_ascii_lowercase()))
        .map(String::from)
        .collect()
}

/// Serveurs connus localement : configuration SSH, puis connexions Zed.
pub fn configured_hosts(cfg: &Config) -> Vec<String> {
    let mut hosts = sys::home_dir()
        .and_then(|home| fs::read_to_string(home.join(".ssh/config")).ok())
        .map(|text| parse_ssh_hosts(&text))
        .unwrap_or_default();
    if let Settings::Loaded(conns) = load(cfg) {
        let mut seen: HashSet<String> = hosts.iter().map(|h| h.to_ascii_lowercase()).collect();
        hosts.extend(
            conns
                .into_iter()
                .map(|c| c.host)
                .filter(|h| seen.insert(h.to_ascii_lowercase())),
        );
    }
    hosts
}

/// Resultat du test SSH sans invite interactive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SshStatus {
    Ready,
    HostKeyChanged,
    Unavailable,
}

pub fn ssh_status(host: &str) -> SshStatus {
    let output = sys::command("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ConnectTimeout=3",
            "-o",
            "ConnectionAttempts=1",
            host,
            "exit",
        ])
        .stdin(Stdio::null())
        .output();
    match output {
        Ok(out) if out.status.success() => SshStatus::Ready,
        Ok(out)
            if String::from_utf8_lossy(&out.stderr)
                .contains("REMOTE HOST IDENTIFICATION HAS CHANGED") =>
        {
            SshStatus::HostKeyChanged
        }
        _ => SshStatus::Unavailable,
    }
}

pub fn load(cfg: &Config) -> Settings {
    let Ok(text) = fs::read_to_string(cfg.zed_settings_path()) else {
        return Settings::Absent;
    };
    let v: Value = match serde_json::from_str(&jsonc::strip(&text)) {
        Ok(v) => v,
        Err(e) => return Settings::Invalid(e.to_string()),
    };
    let as_text = |v: &Value| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let conns = v["ssh_connections"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| !c["host"].is_null())
        .map(|c| {
            let host = as_text(&c["host"]);
            let label = if c["nickname"].is_null() {
                host.clone()
            } else {
                as_text(&c["nickname"])
            };
            let paths = c["projects"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|p| p["paths"].as_array().into_iter().flatten())
                .filter_map(Value::as_str)
                .filter(|p| p.starts_with('/') && !p.contains(['\t', '\n', '\r']))
                .map(String::from)
                .collect();
            Conn { host, label, paths }
        })
        .collect();
    Settings::Loaded(conns)
}

fn entry(host: &str, label: &str, path: &str) -> Entry {
    Entry {
        kind: Kind::Remote,
        key: format!("ssh://{host}{path}"),
        name: path
            .rsplit('/')
            .find(|s| !s.is_empty())
            .unwrap_or("")
            .into(),
        owner: label.into(),
        visibility: "ssh".into(),
        description: format!("{host}:{path}"),
        localpath: String::new(),
    }
}

/// Les projets deja declares dans Zed : instantane, hors ligne.
pub fn settings_entries(conns: &[Conn]) -> Vec<Entry> {
    conns
        .iter()
        .flat_map(|c| c.paths.iter().map(|p| entry(&c.host, &c.label, p)))
        .collect()
}

/// Scan git d'un hote. Un serveur eteint est signale par l'appelant, puis ignore.
/// `sys::output_ok` ferme stdin : ssh ne peut rien demander (BatchMode en plus).
fn scan_host(cfg: &Config, host: &str, label: &str) -> Option<Vec<Entry>> {
    let script = format!(
        "find \"$HOME\" -mindepth 1 -maxdepth {} -name .git -type d -prune 2>/dev/null | sort",
        cfg.remote.scan_depth
    );
    let out = sys::output_ok(
        sys::command("ssh")
            .args(["-o", "BatchMode=yes", "-o"])
            .arg(format!("ConnectTimeout={}", cfg.remote.ssh_timeout))
            .arg(host)
            .arg(script),
    )?;
    let text = String::from_utf8_lossy(&out);
    Some(
        text.lines()
            .filter_map(|l| l.strip_suffix("/.git"))
            .filter(|d| d.starts_with('/') && !d.contains('\t'))
            // Les depots sous un dossier cache (.nvm, .codex...) sont des
            // outils, pas des projets.
            .filter(|d| !d.contains("/."))
            .map(|d| entry(host, label, d))
            .collect(),
    )
}

/// Les hotes sont scannes en parallele : N serveurs coutent le temps du plus lent.
fn scan_all(cfg: &Config, conns: &[Conn], warnings: &mut Vec<String>) -> Vec<Entry> {
    let mut seen = HashSet::new();
    let hosts: Vec<&Conn> = conns
        .iter()
        .filter(|c| seen.insert(c.host.as_str()))
        .collect();
    let results: Vec<_> = thread::scope(|s| {
        let handles: Vec<_> = hosts
            .iter()
            .map(|c| s.spawn(|| scan_host(cfg, &c.host, &c.label)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().ok().flatten())
            .collect()
    });
    let mut all = Vec::new();
    for (c, r) in hosts.iter().zip(results) {
        match r {
            Some(list) => all.extend(list),
            None => warnings.push(format!("Serveur injoignable, ignoré : {}", c.host)),
        }
    }
    all
}

fn read_cache(cfg: &Config) -> Vec<Entry> {
    fs::read_to_string(&cfg.remote_cache_file)
        .map(|t| t.lines().filter_map(Entry::from_tsv).collect())
        .unwrap_or_default()
}

/// settings.json d'abord, puis le resultat du dernier scan (--refresh), sans doublon.
pub fn entries(cfg: &Config, refresh: bool) -> (Vec<Entry>, Vec<String>) {
    let mut warnings = Vec::new();
    if !cfg.remote.enabled {
        return (Vec::new(), warnings);
    }
    let mut conns: Vec<Conn> = cfg
        .remote
        .servers
        .iter()
        .map(|s| Conn {
            host: s.host.clone(),
            label: if s.label.trim().is_empty() {
                s.host.clone()
            } else {
                s.label.clone()
            },
            paths: s.paths.clone(),
        })
        .collect();
    if let Settings::Loaded(zed) = load(cfg) {
        conns.extend(zed);
    }
    let mut known = HashSet::new();
    let base: Vec<Entry> = settings_entries(&conns)
        .into_iter()
        .filter(|e| known.insert(e.key.clone()))
        .collect();

    let scanned = if refresh {
        let found = scan_all(cfg, &conns, &mut warnings);
        if !found.is_empty() {
            let text: String = found.iter().map(|e| e.to_tsv() + "\n").collect();
            let _ = sys::write_atomic(&cfg.remote_cache_file, &text);
            found
        } else {
            // Scan vide (serveurs eteints, hors ligne...) : on garde le cache
            // precedent plutot que de perdre la liste.
            let cached = read_cache(cfg);
            if !cached.is_empty() {
                warnings
                    .push("Aucun dépôt distant trouvé — le scan précédent est conservé.".into());
            }
            cached
        }
    } else {
        read_cache(cfg)
    };

    let mut all = base;
    all.extend(scanned.into_iter().filter(|e| {
        conns
            .iter()
            .any(|c| e.key.starts_with(&format!("ssh://{}/", c.host)))
            && known.insert(e.key.clone())
    }));
    (all, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RemoteServer;

    #[test]
    fn trouve_les_alias_sans_les_motifs() {
        let hosts = parse_ssh_hosts(
            "Host VPSHost vps-prod\n  HostName 147.79.117.138\nHost *\nHost !cache *.local\nhost VPSHost\nHost 193.108.53.92 # direct\n",
        );
        assert_eq!(hosts, ["VPSHost", "vps-prod", "193.108.53.92"]);
    }

    #[test]
    fn serveur_manuel_apparait_sans_zed_ni_scan() {
        let mut cfg = Config::default();
        cfg.remote.zed_settings = "__zrepo_missing_zed_settings__.json".into();
        cfg.remote_cache_file = "__zrepo_missing_remote_cache__.tsv".into();
        cfg.remote.servers.push(RemoteServer {
            host: "VPSHost".into(),
            label: "Production".into(),
            paths: vec!["/home/jp/app".into()],
        });
        let (projects, warnings) = entries(&cfg, false);
        assert!(warnings.is_empty());
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].key, "ssh://VPSHost/home/jp/app");
        assert_eq!(projects[0].owner, "Production");
    }
}
