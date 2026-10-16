//! Projets distants, declares dans zrepo-rs ou Zed et ouverts via ssh://.
//!
//! Toutes les lectures de settings.json echouent en silence : un fichier
//! illisible doit couter les entrees distantes, jamais le launcher entier.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
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
        .map(ssh_words)
        .filter_map(|words| {
            words
                .split_first()
                .filter(|(key, _)| key.eq_ignore_ascii_case("Host"))
                .map(|(_, values)| values.to_vec())
        })
        .flatten()
        .filter(|host| !host.contains(['*', '?', '!']) && !host.starts_with('-'))
        .filter(|host| seen.insert(host.to_ascii_lowercase()))
        .collect()
}

/// Découpe une directive SSH en respectant guillemets, échappements et commentaires.
fn ssh_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            word.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => quoted = !quoted,
            '#' if !quoted => break,
            c if c.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if escaped {
        word.push('\\');
    }
    if !word.is_empty() {
        words.push(word);
    }
    if let Some(first) = words.first().cloned()
        && let Some((key, value)) = first.split_once('=')
        && !key.is_empty()
    {
        words[0] = key.to_string();
        if !value.is_empty() {
            words.insert(1, value.to_string());
        }
    }
    if words.get(1).is_some_and(|s| s == "=") {
        words.remove(1);
    } else if let Some(value) = words
        .get(1)
        .and_then(|s| s.strip_prefix('=').map(str::to_string))
    {
        words[1] = value;
    }
    words
}

fn ssh_config_hosts(config: &Path, ssh_dir: &Path) -> Vec<String> {
    let mut hosts = Vec::new();
    let mut seen_hosts = HashSet::new();
    let mut seen_files = HashSet::new();
    walk_ssh_config(
        config,
        ssh_dir,
        0,
        &mut seen_files,
        &mut seen_hosts,
        &mut hosts,
    );
    hosts
}

fn walk_ssh_config(
    file: &Path,
    ssh_dir: &Path,
    depth: usize,
    seen_files: &mut HashSet<PathBuf>,
    seen_hosts: &mut HashSet<String>,
    hosts: &mut Vec<String>,
) {
    if depth >= 16 || seen_files.len() >= 256 {
        return;
    }
    let Ok(path) = fs::canonicalize(file) else {
        return;
    };
    if !seen_files.insert(path.clone()) {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let words = ssh_words(line);
        let Some((keyword, values)) = words.split_first() else {
            continue;
        };
        if keyword.eq_ignore_ascii_case("Host") {
            for host in parse_ssh_hosts(line) {
                if seen_hosts.insert(host.to_ascii_lowercase()) {
                    hosts.push(host);
                }
            }
        } else if keyword.eq_ignore_ascii_case("Include") {
            for pattern in values {
                let pattern = if pattern == "~" {
                    ssh_dir.parent().unwrap_or(ssh_dir).to_path_buf()
                } else if let Some(rest) = pattern.strip_prefix("~/") {
                    ssh_dir.parent().unwrap_or(ssh_dir).join(rest)
                } else if Path::new(pattern).is_absolute() {
                    PathBuf::from(pattern)
                } else {
                    ssh_dir.join(pattern)
                };
                if let Some(pattern) = pattern.to_str()
                    && let Ok(paths) = glob::glob(pattern)
                {
                    for path in paths.flatten() {
                        walk_ssh_config(&path, ssh_dir, depth + 1, seen_files, seen_hosts, hosts);
                    }
                }
            }
        }
    }
}

/// Serveurs connus localement : configuration SSH, puis connexions Zed.
pub fn configured_hosts(cfg: &Config) -> Vec<String> {
    let mut hosts = sys::home_dir()
        .map(|home| {
            let ssh_dir = home.join(".ssh");
            ssh_config_hosts(&ssh_dir.join("config"), &ssh_dir)
        })
        .unwrap_or_default();
    let mut seen: HashSet<String> = hosts.iter().map(|h| h.to_ascii_lowercase()).collect();
    if let Settings::Loaded(conns) = load(cfg) {
        hosts.extend(
            conns
                .into_iter()
                .map(|c| c.host)
                .filter(|h| seen.insert(h.to_ascii_lowercase())),
        );
    }
    hosts.extend(
        cfg.remote
            .servers
            .iter()
            .map(|s| s.host.clone())
            .filter(|h| seen.insert(h.to_ascii_lowercase())),
    );
    hosts
}

/// Resultat du test SSH sans invite interactive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SshStatus {
    Ready,
    HostKeyChanged,
    HostKeyUnverified,
    AuthenticationRejected,
    MissingPath,
    Unavailable,
}

pub fn ssh_status(host: &str) -> SshStatus {
    probe_ssh(host, "exit")
}

/// Contrôle sans invite : n'empêche jamais le lancement interactif du terminal.
pub fn ssh_project_status(key: &str) -> SshStatus {
    let Some(rest) = key.strip_prefix("ssh://") else {
        return SshStatus::Unavailable;
    };
    let Some((host, path)) = rest.split_once('/') else {
        return SshStatus::Unavailable;
    };
    if host.is_empty()
        || host.starts_with('-')
        || !host.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@' | ':' | '[' | ']')
        })
        || path.contains(['\r', '\n', '\0'])
    {
        return SshStatus::Unavailable;
    }
    let path = format!("/{path}");
    let command = format!(
        "if test -d {}; then exit 0; else printf ZREPO_RS_PATH_MISSING >&2; exit 42; fi",
        ssh_quote(&path)
    );
    probe_ssh(host, &command)
}

fn ssh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn classify_ssh(success: bool, stderr: &str) -> SshStatus {
    if success {
        return SshStatus::Ready;
    }
    if stderr.contains("REMOTE HOST IDENTIFICATION HAS CHANGED") {
        return SshStatus::HostKeyChanged;
    }
    if stderr.contains("ZREPO_RS_PATH_MISSING") {
        return SshStatus::MissingPath;
    }
    if stderr.contains("Host key verification failed") || stderr.contains("No host key is known") {
        return SshStatus::HostKeyUnverified;
    }
    if stderr.contains("Permission denied") || stderr.contains("Too many authentication failures") {
        return SshStatus::AuthenticationRejected;
    }
    SshStatus::Unavailable
}

fn probe_ssh(host: &str, command: &str) -> SshStatus {
    let output = sys::command("ssh")
        .args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ConnectTimeout=3",
            "-o",
            "ConnectionAttempts=1",
            host,
            command,
        ])
        .stdin(Stdio::null())
        .output();
    match output {
        Ok(out) => classify_ssh(out.status.success(), &String::from_utf8_lossy(&out.stderr)),
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
    let script = scan_script(cfg.remote.scan_depth);
    let out = sys::output_ok(
        sys::command("ssh")
            .args(["-o", "BatchMode=yes", "-o"])
            .arg(format!("ConnectTimeout={}", cfg.remote.ssh_timeout))
            .arg(host)
            .arg(script),
    )?;
    Some(parse_scan(&String::from_utf8_lossy(&out), host, label))
}

fn scan_script(depth: usize) -> String {
    format!(
        "find \"$HOME\" -mindepth 1 -maxdepth {} -name .git \\( -type d -o -type f \\) -print -prune 2>/dev/null | sort",
        depth
    )
}

fn parse_scan(text: &str, host: &str, label: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|l| l.strip_suffix("/.git"))
        .filter(|d| d.starts_with('/') && !d.contains('\t'))
        // Les depots sous un dossier cache (.nvm, .codex...) sont des
        // outils, pas des projets.
        .filter(|d| !d.contains("/."))
        .map(|d| entry(host, label, d))
        .collect()
}

/// Les hotes sont scannes en parallele : N serveurs coutent le temps du plus lent.
fn scan_all(cfg: &Config, conns: &[Conn]) -> Vec<(String, Option<Vec<Entry>>)> {
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
    hosts
        .iter()
        .zip(results)
        .map(|(c, r)| (c.host.clone(), r))
        .collect()
}

#[derive(Default, Serialize, Deserialize)]
struct ScanCache {
    hosts: Vec<HostCache>,
}

#[derive(Serialize, Deserialize)]
struct HostCache {
    host: String,
    last_success: Option<u64>,
    entries: Vec<String>,
}

fn entry_host(entry: &Entry) -> Option<&str> {
    entry
        .key
        .strip_prefix("ssh://")?
        .split_once('/')
        .map(|(host, _)| host)
}

fn read_cache(cfg: &Config) -> ScanCache {
    let Ok(text) = fs::read_to_string(&cfg.remote_cache_file) else {
        return ScanCache::default();
    };
    if let Ok(cache) = serde_json::from_str(&text) {
        return cache;
    }
    // Ancien format TSV : la première écriture migre le cache sans perdre les hôtes hors ligne.
    let mut cache = ScanCache::default();
    for line in text.lines() {
        if let Some(entry) = Entry::from_tsv(line)
            && let Some(host) = entry_host(&entry)
        {
            let host = host.to_string();
            if !cache.hosts.iter().any(|h| h.host == host) {
                cache.hosts.push(HostCache {
                    host: host.clone(),
                    last_success: None,
                    entries: Vec::new(),
                });
            }
            cache
                .hosts
                .iter_mut()
                .find(|h| h.host == host)
                .unwrap()
                .entries
                .push(line.to_string());
        }
    }
    cache
}

pub fn cache_summary(cfg: &Config) -> Vec<(String, usize, Option<u64>)> {
    read_cache(cfg)
        .hosts
        .into_iter()
        .map(|h| (h.host, h.entries.len(), h.last_success))
        .collect()
}

fn reconcile(
    cache: &mut ScanCache,
    results: Vec<(String, Option<Vec<Entry>>)>,
    warnings: &mut Vec<String>,
) {
    for (host, result) in results {
        match result {
            Some(entries) => {
                let items = entries.iter().map(Entry::to_tsv).collect();
                let last_success = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs());
                if let Some(old) = cache.hosts.iter_mut().find(|h| h.host == host) {
                    old.entries = items;
                    old.last_success = last_success;
                } else {
                    cache.hosts.push(HostCache {
                        host,
                        last_success,
                        entries: items,
                    });
                }
            }
            None => {
                let count = cache
                    .hosts
                    .iter()
                    .find(|h| h.host == host)
                    .map_or(0, |h| h.entries.len());
                warnings.push(format!(
                    "Serveur {host} hors ligne : {count} résultat(s) du dernier scan conservé(s)."
                ));
            }
        }
    }
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

    let mut cache = read_cache(cfg);
    if refresh {
        reconcile(&mut cache, scan_all(cfg, &conns), &mut warnings);
        if let Ok(text) = serde_json::to_string(&cache)
            && let Err(e) = sys::write_atomic(&cfg.remote_cache_file, &text)
        {
            warnings.push(format!("Cache des serveurs non enregistré : {e}"));
        }
    }
    let scanned = cache
        .hosts
        .into_iter()
        .flat_map(|h| {
            h.entries
                .into_iter()
                .filter_map(|line| Entry::from_tsv(&line))
        })
        .collect::<Vec<_>>();

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
    fn includes_glob_doublons_cycle_et_fichier_absent() {
        let root = std::env::temp_dir().join(format!("zrepo-ssh-config-{}", std::process::id()));
        let ssh = root.join(".ssh");
        fs::create_dir_all(ssh.join("config.d")).unwrap();
        fs::write(
            ssh.join("config"),
            "Host Main\nInclude config.d/*.conf missing.conf\nHost MAIN\n",
        )
        .unwrap();
        fs::write(
            ssh.join("config.d/a.conf"),
            "Host Alpha *.invalid !neg\nInclude ../config\n",
        )
        .unwrap();
        fs::write(ssh.join("config.d/b.conf"), "Host Beta alpha\n").unwrap();
        assert_eq!(
            ssh_config_hosts(&ssh.join("config"), &ssh),
            ["Main", "Alpha", "Beta"]
        );
        assert_eq!(
            ssh_words("Include \"config with spaces/*.conf\" # note"),
            ["Include", "config with spaces/*.conf"]
        );
        assert_eq!(
            ssh_words("Include=config.d/*.conf"),
            ["Include", "config.d/*.conf"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn diagnostic_ssh_distingue_les_erreurs() {
        assert_eq!(classify_ssh(true, ""), SshStatus::Ready);
        assert_eq!(
            classify_ssh(false, "Connection timed out"),
            SshStatus::Unavailable
        );
        assert_eq!(
            classify_ssh(false, "Permission denied (publickey)"),
            SshStatus::AuthenticationRejected
        );
        assert_eq!(
            classify_ssh(false, "REMOTE HOST IDENTIFICATION HAS CHANGED"),
            SshStatus::HostKeyChanged
        );
        assert_eq!(
            classify_ssh(false, "ZREPO_RS_PATH_MISSING"),
            SshStatus::MissingPath
        );
        assert_eq!(
            classify_ssh(false, "Host key verification failed"),
            SshStatus::HostKeyUnverified
        );
        assert_eq!(ssh_quote("/home/a'b"), "'/home/a'\"'\"'b'");
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

    #[test]
    fn depot_classique_et_worktree() {
        let entries = parse_scan(
            "/home/jp/classic/.git\n/home/jp/worktree/.git\n/home/jp/.cache/tool/.git\n",
            "prod",
            "Production",
        );
        assert_eq!(
            entries.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
            ["ssh://prod/home/jp/classic", "ssh://prod/home/jp/worktree"]
        );
        assert!(scan_script(3).contains("-type d -o -type f"));
    }

    #[cfg(unix)]
    #[test]
    fn find_decouvre_repertoire_et_fichier_git() {
        let root = std::env::temp_dir().join(format!("zrepo-scan-{}", std::process::id()));
        let classic = root.join("classic/.git");
        let worktree = root.join("worktree/.git");
        fs::create_dir_all(&classic).unwrap();
        fs::create_dir_all(worktree.parent().unwrap()).unwrap();
        fs::write(&worktree, "gitdir: /tmp/git/worktrees/worktree\n").unwrap();
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(scan_script(3))
            .env("HOME", &root)
            .output()
            .unwrap();
        assert!(output.status.success());
        let paths = String::from_utf8(output.stdout).unwrap();
        assert!(paths.contains(&classic.display().to_string()));
        assert!(paths.contains(&worktree.display().to_string()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scan_vide_et_hote_hors_ligne() {
        let mut cache = ScanCache {
            hosts: vec![HostCache {
                host: "prod".into(),
                last_success: Some(1),
                entries: vec![entry("prod", "Production", "/old").to_tsv()],
            }],
        };
        let mut warnings = Vec::new();
        reconcile(
            &mut cache,
            vec![("prod".into(), Some(Vec::new()))],
            &mut warnings,
        );
        assert!(cache.hosts[0].entries.is_empty());
        assert!(cache.hosts[0].last_success.unwrap() > 1);
        reconcile(&mut cache, vec![("prod".into(), None)], &mut warnings);
        assert!(cache.hosts[0].entries.is_empty());
        assert!(warnings[0].contains("hors ligne"));
    }

    #[test]
    fn plusieurs_serveurs_un_seul_repond() {
        let mut cache = ScanCache {
            hosts: vec![
                HostCache {
                    host: "a".into(),
                    last_success: Some(1),
                    entries: vec![entry("a", "a", "/old").to_tsv()],
                },
                HostCache {
                    host: "b".into(),
                    last_success: Some(2),
                    entries: vec![entry("b", "b", "/keep").to_tsv()],
                },
            ],
        };
        let mut warnings = Vec::new();
        reconcile(
            &mut cache,
            vec![
                ("a".into(), Some(vec![entry("a", "a", "/new")])),
                ("b".into(), None),
            ],
            &mut warnings,
        );
        assert!(cache.hosts[0].entries[0].contains("/new"));
        assert!(cache.hosts[1].entries[0].contains("/keep"));
        assert_eq!(cache.hosts[1].last_success, Some(2));
        assert!(warnings[0].contains("b"));
    }
}
