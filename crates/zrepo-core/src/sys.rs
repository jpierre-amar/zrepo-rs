//! Petits utilitaires systeme : PATH, commandes externes, lancement detache,
//! ecriture atomique. Les differences entre OS sont ici, derriere des `cfg`.

use std::{
    env,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Windows : pas de console qui clignote a chaque commande lancee.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// Seul point de creation des commandes externes du core.
pub fn command(bin: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(bin);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// Suffixes a essayer apres le nom : ceux de PATHEXT sous Windows, sauf si
/// le nom porte deja une extension.
#[cfg(windows)]
fn exe_suffixes(bin: &str) -> Vec<String> {
    if Path::new(bin).extension().is_some() {
        return vec![String::new()];
    }
    env::var("PATHEXT")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .filter(|e| !e.is_empty())
        .map(str::to_lowercase)
        .collect()
}

#[cfg(not(windows))]
fn exe_suffixes(_bin: &str) -> Vec<String> {
    vec![String::new()]
}

/// Equivalent de `command -v` : chemin du premier executable trouve dans le PATH.
pub fn which(bin: &str) -> Option<PathBuf> {
    let suffixes = exe_suffixes(bin);
    env::split_paths(&env::var_os("PATH")?)
        .flat_map(|dir| suffixes.iter().map(move |s| dir.join(format!("{bin}{s}"))))
        .find(|p| is_executable(p))
}

pub fn has(bin: &str) -> bool {
    which(bin).is_some()
}

/// Repertoire personnel ; None seulement sur un systeme tres degrade.
pub fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

/// Comment lancer Zed : programme, puis arguments a placer avant le chemin.
pub struct ZedLauncher {
    pub program: PathBuf,
    pub prefix: Vec<String>,
}

impl ZedLauncher {
    fn bin(program: PathBuf) -> Self {
        Self {
            program,
            prefix: Vec::new(),
        }
    }
}

/// Zed s'appelle `zeditor` sur Arch/Omarchy, `zed` ailleurs.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn zed_launcher() -> Option<ZedLauncher> {
    ["zeditor", "zed"]
        .into_iter()
        .find_map(which)
        .map(ZedLauncher::bin)
}

/// Le CLI `zed` n'est dans le PATH que si l'utilisateur l'a installe depuis
/// Zed ; sinon on passe par l'application.
#[cfg(target_os = "macos")]
pub fn zed_launcher() -> Option<ZedLauncher> {
    if let Some(p) = which("zed") {
        return Some(ZedLauncher::bin(p));
    }
    let app = std::iter::once(PathBuf::from("/Applications/Zed.app"))
        .chain(home_dir().map(|h| h.join("Applications/Zed.app")))
        .any(|p| p.is_dir());
    app.then(|| ZedLauncher {
        program: PathBuf::from("open"),
        prefix: vec!["-a".into(), "Zed".into()],
    })
}

/// L'installeur de Zed ne met pas toujours `zed` dans le PATH.
#[cfg(windows)]
pub fn zed_launcher() -> Option<ZedLauncher> {
    which("zed")
        .or_else(|| {
            let local = PathBuf::from(env::var_os("LOCALAPPDATA")?);
            Some(local.join(r"Programs\Zed\bin\zed.exe")).filter(|p| p.is_file())
        })
        .map(ZedLauncher::bin)
}

/// Lance un programme detache du lanceur, pour que sa fermeture ne le tue pas.
pub fn spawn_detached(bin: &Path, args: &[&str]) -> bool {
    spawn_detached_in(bin, args, None)
}

/// Comme `spawn_detached`, dans le dossier `dir` s'il est donne.
/// `setsid -f` rend la main des le fork : il ne peut pas signaler un exec rate,
/// c'est a l'appelant de verifier avant que le binaire existe.
#[cfg(target_os = "linux")]
pub fn spawn_detached_in(bin: &Path, args: &[&str], dir: Option<&Path>) -> bool {
    let setsid = has("setsid");
    let mut cmd = if setsid {
        let mut c = Command::new("setsid");
        c.arg("-f").arg(bin);
        c
    } else {
        Command::new(bin)
    };
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if setsid {
        cmd.status().is_ok_and(|s| s.success())
    } else {
        cmd.spawn().is_ok()
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
pub fn spawn_detached_in(bin: &Path, args: &[&str], dir: Option<&Path>) -> bool {
    let mut cmd = Command::new(bin);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(windows)]
pub fn spawn_detached_in(bin: &Path, args: &[&str], dir: Option<&Path>) -> bool {
    use std::os::windows::process::CommandExt;
    let mut cmd = Command::new(bin);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    cmd.args(args)
        .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// Windows : programme console (PowerShell...) dans sa propre fenetre.
#[cfg(windows)]
pub fn spawn_console(bin: &Path, args: &[&str], dir: &Path) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    Command::new(bin)
        .args(args)
        .current_dir(dir)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .is_ok()
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&tmp, content)?;
    // Les caches peuvent contenir les noms et descriptions de dépôts privés.
    // Ne pas dépendre de l'umask de la session pour leur confidentialité.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Version de git (majeure, mineure), d'apres `git --version`.
pub fn git_version() -> Option<(u32, u32)> {
    let out = output_ok(command("git").arg("--version"))?;
    parse_git_version(&String::from_utf8_lossy(&out))
}

/// « git version 2.43.0 », « git version 2.39.3 (Apple Git-146) »...
pub fn parse_git_version(s: &str) -> Option<(u32, u32)> {
    let v = s
        .split_whitespace()
        .find(|w| w.starts_with(|c: char| c.is_ascii_digit()))?;
    let mut it = v.split('.').map(|n| n.parse::<u32>().ok());
    Some((it.next()??, it.next()??))
}

/// GIT_CONFIG_COUNT (le jeton passe par l'environnement) : git 2.31 et plus.
pub const GIT_MIN_FOR_TOKEN: (u32, u32) = (2, 31);

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
        command("git")
            .arg("-C")
            .arg(dir)
            .args(["remote", "get-url", "origin"]),
    )?;
    normalize_remote(&String::from_utf8_lossy(&out))
}

#[cfg(test)]
mod tests {
    use super::{normalize_remote, parse_git_version, which};

    #[cfg(unix)]
    use super::write_atomic;

    #[test]
    fn version_de_git() {
        assert_eq!(parse_git_version("git version 2.43.0\n"), Some((2, 43)));
        assert_eq!(
            parse_git_version("git version 2.39.3 (Apple Git-146)"),
            Some((2, 39))
        );
        assert_eq!(
            parse_git_version("git version 2.30.1.windows.1"),
            Some((2, 30))
        );
        assert_eq!(parse_git_version("rien"), None);
    }

    #[test]
    fn formes_de_remote() {
        let want = Some("amar/zrepo-rs".to_string());
        assert_eq!(normalize_remote("git@github.com:Amar/zrepo-rs.git"), want);
        assert_eq!(normalize_remote("https://github.com/amar/zrepo-rs"), want);
        assert_eq!(
            normalize_remote("ssh://git@github.com/amar/zrepo-rs.git\n"),
            want
        );
        assert_eq!(normalize_remote("https://gitlab.com/amar/zrepo-rs"), None);
        assert_eq!(normalize_remote("amar/zrepo-rs/x"), None);
        assert_eq!(normalize_remote(""), None);
    }

    #[test]
    fn which_cherche_dans_le_path() {
        // git est indispensable a zrepo-rs, donc present la ou tournent les tests.
        assert!(which("git").is_some());
        assert!(which("zrepo-rs-binaire-qui-n-existe-pas").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn ecriture_atomique_privee() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("zrepo-rs-permissions-{}", std::process::id()));
        let path = dir.join("cache.tsv");
        write_atomic(&path, "dépôt privé\n").unwrap();
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
