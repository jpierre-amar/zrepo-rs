//! PATH du processus. Sous macOS, une app lancee depuis le Finder, le Dock ou
//! Spotlight ne recoit de launchd que /usr/bin:/bin:/usr/sbin:/sbin : herdr,
//! codex ou zed n'y sont pas. `main` le complete avant toute detection.

use std::{
    env,
    ffi::{OsStr, OsString},
    path::PathBuf,
    sync::OnceLock,
};

#[cfg(target_os = "macos")]
use std::{
    process::{Command, Stdio},
    time::Duration,
};

#[cfg(target_os = "macos")]
use crate::sys;

/// Dossiers ajoutes au PATH au demarrage (macOS) ; vide si rien n'a change.
static ADDED: OnceLock<Vec<PathBuf>> = OnceLock::new();

/// A appeler une fois, depuis `main`, apres avoir applique le PATH complete.
pub fn note_added(added: Vec<PathBuf>) {
    let _ = ADDED.set(added);
}

/// None si le PATH n'a pas ete examine (hors macOS, ou pas encore).
pub fn added() -> Option<&'static [PathBuf]> {
    ADDED.get().map(Vec::as_slice)
}

/// `current`, puis les dossiers de `extra` absents, dans l'ordre et sans
/// doublon. Rend le nouveau PATH et les dossiers ajoutes ; None si rien ne manque.
pub fn merge_paths(
    current: &OsStr,
    extra: impl IntoIterator<Item = PathBuf>,
) -> Option<(OsString, Vec<PathBuf>)> {
    let mut all: Vec<PathBuf> = Vec::new();
    for p in env::split_paths(current) {
        if !p.as_os_str().is_empty() && !all.contains(&p) {
            all.push(p);
        }
    }
    let mut added = Vec::new();
    for p in extra {
        if !p.as_os_str().is_empty() && !all.contains(&p) {
            all.push(p.clone());
            added.push(p);
        }
    }
    if added.is_empty() {
        return None;
    }
    Some((env::join_paths(all).ok()?, added))
}

/// Dossiers usuels des outils installes hors de Homebrew ou par Homebrew.
#[cfg(target_os = "macos")]
fn usual_bin_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(h) = sys::home_dir() {
        v.push(h.join(".local/bin"));
        v.push(h.join(".cargo/bin"));
    }
    v.push(PathBuf::from("/opt/homebrew/bin"));
    v.push(PathBuf::from("/usr/local/bin"));
    v.retain(|p| p.is_dir());
    v
}

/// Valeur de PATH dans la sortie de `env` (la derniere, si un fichier du
/// shell ecrit aussi quelque chose).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn path_from_env_output(out: &str) -> Option<&str> {
    out.lines()
        .filter_map(|l| l.strip_prefix("PATH="))
        .next_back()
        .filter(|p| !p.is_empty())
}

/// PATH du shell de connexion ; None apres `timeout` ou en cas d'echec.
/// `env` plutot que `printf "$PATH"` : fish rendrait sa liste separee par des espaces.
#[cfg(target_os = "macos")]
fn login_shell_path(timeout: Duration) -> Option<String> {
    use std::{io::Read, time::Instant};
    let shell = env::var_os("SHELL")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/zsh".into());
    let mut child = Command::new(shell)
        .args(["-l", "-c", "/usr/bin/env"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(None) if start.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    path_from_env_output(&out).map(str::to_string)
}

/// Lancee depuis le Finder, le Dock ou Spotlight, une app ne recoit de launchd
/// que /usr/bin:/bin:/usr/sbin:/sbin. On complete avec le PATH du shell de
/// connexion, puis avec les dossiers usuels. Rien a faire (et pas de shell
/// lance) si ces dossiers y sont deja. Rend le nouveau PATH et les ajouts.
#[cfg(target_os = "macos")]
pub fn completed() -> Option<(OsString, Vec<PathBuf>)> {
    let current = env::var_os("PATH").unwrap_or_default();
    let usual = usual_bin_dirs();
    let present: Vec<PathBuf> = env::split_paths(&current).collect();
    if usual.iter().all(|d| present.contains(d)) {
        return None;
    }
    let shell = login_shell_path(Duration::from_secs(2)).unwrap_or_default();
    let extra = env::split_paths(&shell).chain(usual);
    merge_paths(&current, extra)
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::{ffi::OsStr, path::PathBuf};

    #[cfg(unix)]
    use super::merge_paths;
    use super::path_from_env_output;

    #[cfg(unix)]
    #[test]
    fn path_complete_sans_doublon_ni_reordonner() {
        let (path, added) = merge_paths(
            OsStr::new("/usr/bin:/bin:/usr/bin"),
            [
                "/opt/homebrew/bin",
                "/usr/bin",
                "/Users/u/.local/bin",
                "/opt/homebrew/bin",
            ]
            .map(PathBuf::from),
        )
        .unwrap();
        assert_eq!(path, "/usr/bin:/bin:/opt/homebrew/bin:/Users/u/.local/bin");
        assert_eq!(
            added,
            ["/opt/homebrew/bin", "/Users/u/.local/bin"].map(PathBuf::from)
        );
        // Rien ne manque : PATH inchange.
        assert!(merge_paths(OsStr::new("/a:/b"), ["/b", "/a"].map(PathBuf::from)).is_none());
    }

    #[test]
    fn path_lu_dans_la_sortie_de_env() {
        let out = "HOME=/Users/u\nbienvenue\nPATH=/opt/homebrew/bin:/usr/bin\nSHELL=/bin/zsh\n";
        assert_eq!(
            path_from_env_output(out),
            Some("/opt/homebrew/bin:/usr/bin")
        );
        assert_eq!(path_from_env_output("HOME=/Users/u\n"), None);
        assert_eq!(path_from_env_output("PATH=\n"), None);
    }
}
