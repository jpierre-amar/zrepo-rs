//! Petits utilitaires systeme : PATH, lancement detache, ecriture atomique.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Equivalent de `command -v` : chemin du premier executable trouve dans le PATH.
pub fn which(bin: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(bin))
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

pub fn has(bin: &str) -> bool {
    which(bin).is_some()
}

/// Zed s'appelle `zeditor` sur Arch/Omarchy, `zed` ailleurs.
pub fn zed_bin() -> Option<&'static str> {
    ["zeditor", "zed"].into_iter().find(|b| has(b))
}

/// Lance un programme detache du terminal, pour que sa fermeture ne le tue pas.
/// `setsid -f` rend la main des le fork : il ne peut pas signaler un exec rate,
/// c'est a l'appelant de verifier avant que le binaire existe.
pub fn spawn_detached(bin: &str, args: &[&str]) -> bool {
    let mut cmd = if has("setsid") {
        let mut c = Command::new("setsid");
        c.arg("-f").arg(bin);
        c
    } else {
        Command::new(bin)
    };
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if has("setsid") {
        cmd.status().is_ok_and(|s| s.success())
    } else {
        cmd.spawn().is_ok()
    }
}

/// Sortie standard d'une commande si elle reussit, stderr ignore.
pub fn output_ok(cmd: &mut Command) -> Option<Vec<u8>> {
    let out = cmd
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// Ecrit via un temporaire puis renomme : un echec laisse l'ancien fichier intact.
pub fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Normalise une URL de remote en "owner/repo" minuscule ; None si non reconnue.
pub fn normalize_remote(url: &str) -> Option<String> {
    let mut u = url.trim();
    u = u.strip_suffix(".git").unwrap_or(u);
    for prefix in [
        "git@github.com:",
        "ssh://git@github.com/",
        "https://github.com/",
        "http://github.com/",
        "git://github.com/",
    ] {
        u = u.strip_prefix(prefix).unwrap_or(u);
    }
    let valid = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    let (owner, repo) = u.split_once('/')?;
    (valid(owner) && valid(repo)).then(|| u.to_lowercase())
}

/// owner/repo du remote `origin` d'un dossier.
pub fn repo_identity(dir: &Path) -> Option<String> {
    let out = output_ok(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["remote", "get-url", "origin"]),
    )?;
    normalize_remote(&String::from_utf8_lossy(&out))
}

#[cfg(test)]
mod tests {
    use super::normalize_remote;

    #[test]
    fn formes_de_remote() {
        let want = Some("amar/zrepo".to_string());
        assert_eq!(normalize_remote("git@github.com:Amar/zrepo.git"), want);
        assert_eq!(normalize_remote("https://github.com/amar/zrepo"), want);
        assert_eq!(
            normalize_remote("ssh://git@github.com/amar/zrepo.git\n"),
            want
        );
        assert_eq!(normalize_remote("https://gitlab.com/amar/zrepo"), None);
        assert_eq!(normalize_remote("amar/zrepo/x"), None);
        assert_eq!(normalize_remote(""), None);
    }
}
