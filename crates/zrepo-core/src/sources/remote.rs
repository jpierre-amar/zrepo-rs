//! Projets distants, lus dans les ssh_connections de Zed et ouverts via
//! ssh:// sans clonage.
//!
//! Toutes les lectures de settings.json echouent en silence : un fichier
//! illisible doit couter les entrees distantes, jamais le launcher entier.

use std::{collections::HashSet, fs, thread};

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
    let conns = match load(cfg) {
        Settings::Loaded(c) => c,
        _ => Vec::new(),
    };
    let base = settings_entries(&conns);

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

    let known: HashSet<String> = base.iter().map(|e| e.key.clone()).collect();
    let mut all = base;
    all.extend(scanned.into_iter().filter(|e| !known.contains(&e.key)));
    (all, warnings)
}
