//! Ouverture d'un projet : resolution de la cle (clonage si besoin), puis
//! Zed, Herdr, un terminal ou un editeur (voir `apps`).

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Stdio,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

use crate::{
    apps, auth,
    config::Config,
    error::{EX_COLLISION, EX_DEPS, EX_FAIL, Fail, Res},
    sys,
};

/// Ecrite dans config.toml comme une simple chaine : "zed", "herdr",
/// "terminal", ou l'id d'un editeur ("vscode", "Mon IDE"...).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Target {
    Zed,
    /// Herdr + l'agent configure (codex --yolo par defaut).
    Herdr,
    /// Un terminal ouvert dans le dossier du projet.
    Terminal,
    /// Un editeur detecte ou declare dans config.toml, par son id.
    App(String),
}

impl From<String> for Target {
    fn from(s: String) -> Self {
        Target::parse(&s)
    }
}

impl From<Target> for String {
    fn from(t: Target) -> Self {
        t.id().to_string()
    }
}

impl Target {
    /// `codex` reste accepte : c'etait le nom de la cible en v1. Tout autre
    /// nom designe un editeur ; sa presence est verifiee a l'ouverture.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "zed" => Target::Zed,
            "herdr" | "codex" => Target::Herdr,
            "terminal" => Target::Terminal,
            _ => Target::App(s.trim().to_string()),
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Target::Zed => "zed",
            Target::Herdr => "herdr",
            Target::Terminal => "terminal",
            Target::App(id) => id,
        }
    }

    /// « Zed », « Herdr + codex --yolo », « Terminal », « VS Code »...
    pub fn label(&self, cfg: &Config) -> String {
        match self {
            Target::Zed => "Zed".into(),
            Target::Herdr => format!("Herdr + {}", cfg.agent_label()),
            Target::Terminal => "Terminal".into(),
            Target::App(id) => apps::editor_name(cfg, id).unwrap_or_else(|| id.clone()),
        }
    }
}

/// Cibles installees, dans l'ordre d'affichage. Herdr seulement s'il est
/// active, installe, et hors Windows. Jamais vide : sans rien de detecte,
/// Zed reste propose (son absence est signalee a l'ouverture, projet pret).
pub fn available_targets(cfg: &Config) -> Vec<Target> {
    let mut t = Vec::new();
    if sys::zed_launcher().is_some() {
        t.push(Target::Zed);
    }
    if cfg!(not(windows)) && cfg.herdr.enabled && sys::has("herdr") {
        t.push(Target::Herdr);
    }
    if apps::terminal(cfg).is_some() {
        t.push(Target::Terminal);
    }
    t.extend(apps::editors(cfg).into_iter().map(|a| Target::App(a.id)));
    if t.is_empty() {
        t.push(Target::Zed);
    }
    t
}

/// Resout (clone si besoin) puis ouvre. `progress` recoit les messages
/// d'etape. Le clonage a lieu avant de savoir dans quoi on ouvre : toutes les
/// cibles veulent le meme dossier pret sur disque.
pub fn open(cfg: &Config, key: &str, target: Target, progress: &dyn Fn(&str)) -> Res<()> {
    enum How {
        Zed,
        Herdr,
        App(apps::App),
    }
    // Terminal et editeurs : verifies avant de cloner, une cible absente
    // n'a pas a declencher un clonage.
    let how = match &target {
        Target::Zed => How::Zed,
        Target::Herdr => How::Herdr,
        Target::Terminal => How::App(apps::terminal(cfg).ok_or_else(|| {
            Fail::new(
                EX_DEPS,
                "Aucun terminal trouvé. Indiquez-en un dans config.toml, section [terminal].",
            )
        })?),
        Target::App(id) => How::App(apps::find_editor(cfg, id).ok_or_else(|| {
            Fail::new(
                EX_DEPS,
                format!(
                    "Éditeur introuvable : {id}.\nCibles disponibles : {}",
                    ids(cfg)
                ),
            )
        })?),
    };
    if matches!(how, How::App(_)) && key.starts_with("ssh://") {
        return Err(Fail::new(
            EX_FAIL,
            format!("Un projet distant s'ouvre dans Zed ou dans Herdr.\nAdresse : {key}"),
        ));
    }
    let resolved = resolve_target(cfg, key, progress)?;
    match how {
        How::App(app) => apps::launch(&app, &resolved, progress),
        How::Zed => open_in_zed(&resolved, progress),
        #[cfg(not(windows))]
        How::Herdr => crate::herdr::open(cfg, &resolved, progress),
        #[cfg(windows)]
        How::Herdr => Err(Fail::new(
            crate::error::EX_HERDR,
            "Herdr n'est pas disponible sous Windows.",
        )),
    }
}

/// `want` s'il fait partie des cibles disponibles, sinon la premiere d'entre
/// elles (un editeur desinstalle ne bloque pas le lanceur).
pub fn pick_target(want: &Target, targets: &[Target]) -> Target {
    if targets.contains(want) {
        want.clone()
    } else {
        targets.first().cloned().unwrap_or(Target::Zed)
    }
}

/// « zed, terminal, vscode » : pour les messages d'erreur.
fn ids(cfg: &Config) -> String {
    available_targets(cfg)
        .iter()
        .map(Target::id)
        .collect::<Vec<_>>()
        .join(", ")
}

fn open_in_zed(target: &str, progress: &dyn Fn(&str)) -> Res<()> {
    let Some(zed) = sys::zed_launcher() else {
        return Err(Fail::new(
            EX_DEPS,
            format!("Zed introuvable.\nLe projet est prêt ici : {target}"),
        ));
    };
    progress(&format!("Ouverture dans Zed : {target}"));
    let mut args: Vec<&str> = zed.prefix.iter().map(String::as_str).collect();
    args.push(target);
    if sys::spawn_detached(&zed.program, &args) {
        Ok(())
    } else {
        Err(Fail::new(
            EX_FAIL,
            format!(
                "Impossible de lancer {}.\nLe projet est prêt ici : {target}",
                zed.program.display()
            ),
        ))
    }
}

/// Variables d'environnement qui donnent le jeton a git sans le mettre sur la
/// ligne de commande ni dans .git/config. `base` = GIT_CONFIG_COUNT deja
/// present dans l'environnement, pour ne pas ecraser une config existante.
fn git_auth_env(token: &str, base: usize) -> [(String, String); 3] {
    let basic = STANDARD.encode(format!("x-access-token:{token}"));
    [
        ("GIT_CONFIG_COUNT".into(), (base + 1).to_string()),
        (
            format!("GIT_CONFIG_KEY_{base}"),
            "http.https://github.com/.extraheader".into(),
        ),
        (
            format!("GIT_CONFIG_VALUE_{base}"),
            format!("AUTHORIZATION: basic {basic}"),
        ),
    ]
}

/// Sans jeton, clone https simple : seuls les depots publics passent.
fn clone_into(key: &str, dir: &Path, progress: &dyn Fn(&str)) -> Res<()> {
    progress(&format!("Clonage de {key}…"));
    let mut cmd = sys::command("git");
    cmd.args(["clone", "--quiet", &format!("https://github.com/{key}")])
        .arg(dir)
        // Pas de terminal derriere la fenetre : git ne doit rien demander.
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let token = auth::token();
    if let Some((t, _)) = &token {
        let base = std::env::var("GIT_CONFIG_COUNT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        cmd.envs(git_auth_env(t, base));
    }
    let out = cmd.output().map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => Fail::new(EX_DEPS, "git introuvable dans le PATH."),
        _ => Fail::new(EX_FAIL, format!("Impossible de lancer git : {e}")),
    })?;
    // On ne supprime jamais un dossier en cas d'echec : on se contente de signaler.
    if out.status.success() {
        progress(&format!("{key} cloné dans {}", dir.display()));
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let mut msg = format!("Échec du clonage de {key}.");
    if !detail.is_empty() {
        msg.push('\n');
        msg.push_str(&detail);
    }
    if token.is_some()
        && let Some(v) = sys::git_version()
        && v < sys::GIT_MIN_FOR_TOKEN
    {
        msg.push_str(&format!(
            "\ngit {}.{} est trop ancien : zrepo-rs passe le jeton GitHub par GIT_CONFIG_COUNT, \
             qui demande git 2.31 ou plus. Mettez git à jour.",
            v.0, v.1
        ));
    }
    if token.is_none() {
        msg.push_str(
            "\nPas de connexion GitHub : seuls les dépôts publics peuvent être clonés. \
             Lancez : zrepo-rs setup",
        );
    }
    Err(Fail::new(EX_FAIL, msg))
}

fn is_repo(dir: &Path, want: &str) -> bool {
    dir.join(".git").is_dir() && sys::repo_identity(dir).as_deref() == Some(want)
}

fn path_str(p: PathBuf) -> String {
    p.to_string_lossy().into_owned()
}

/// Ou en est un depot GitHub sur le disque.
enum Plan {
    /// Deja clone ici.
    Ready(PathBuf),
    /// A cloner ici ; `taken` = le nom simple est pris par autre chose.
    Clone {
        dir: PathBuf,
        taken: Option<PathBuf>,
    },
}

/// Ne modifie rien : sert a l'ouverture comme a l'affichage.
fn plan(cfg: &Config, key: &str) -> Res<Plan> {
    let invalid = || Fail::new(EX_FAIL, format!("Clé invalide : {key}"));
    let (owner, repo) = key.split_once('/').ok_or_else(invalid)?;
    // Sans cette garde, un `want` vide correspondrait a tout depot sans origin.
    let want = sys::normalize_remote(key).ok_or_else(invalid)?;

    let target = cfg.projects_dir.join(repo);
    if !target.exists() {
        return Ok(Plan::Clone {
            dir: target,
            taken: None,
        });
    }
    if is_repo(&target, &want) {
        return Ok(Plan::Ready(target)); // deja clone, le bon depot
    }

    // Nom deja pris par autre chose : on se rabat sur owner-repo, sans rien ecraser.
    let alt = cfg.projects_dir.join(format!("{owner}-{repo}"));
    if alt.exists() {
        if is_repo(&alt, &want) {
            return Ok(Plan::Ready(alt));
        }
        return Err(Fail::new(
            EX_COLLISION,
            format!(
                "Collision : {} et {} existent déjà et ne correspondent pas à {key}.\n\
                 Rien n'a été modifié. Renommez ou déplacez l'un des deux.",
                target.display(),
                alt.display()
            ),
        ));
    }
    Ok(Plan::Clone {
        dir: alt,
        taken: Some(target),
    })
}

/// Dossier local d'un depot GitHub (`owner/repo`), qu'il soit deja clone ou
/// qu'il soit a cloner ; None pour une autre cle ou en cas de collision.
pub fn clone_destination(cfg: &Config, key: &str) -> Option<PathBuf> {
    match plan(cfg, key).ok()? {
        Plan::Ready(dir) | Plan::Clone { dir, .. } => Some(dir),
    }
}

fn is_local_path(key: &str) -> bool {
    key.starts_with('/') || Path::new(key).is_absolute()
}

/// Resout une cle en cible ouvrable — chemin absolu local ou URL ssh:// — en
/// clonant si besoin.
fn resolve_target(cfg: &Config, key: &str, progress: &dyn Fn(&str)) -> Res<String> {
    // Projet distant : la cle est deja une adresse. Le serveur peut etre
    // eteint, c'est l'outil ouvert qui le dira.
    if key.starts_with("ssh://") {
        return Ok(key.into());
    }

    if is_local_path(key) {
        return if Path::new(key).is_dir() {
            Ok(key.into())
        } else {
            Err(Fail::new(EX_FAIL, format!("Dossier introuvable : {key}")))
        };
    }

    match plan(cfg, key)? {
        Plan::Ready(dir) => Ok(path_str(dir)),
        Plan::Clone { dir, taken } => {
            fs::create_dir_all(&cfg.projects_dir).map_err(|e| {
                Fail::new(
                    EX_FAIL,
                    format!("Impossible de créer {} : {e}", cfg.projects_dir.display()),
                )
            })?;
            if let Some(t) = taken {
                progress(&format!(
                    "Note : {} est déjà pris par un autre projet, clonage vers {}",
                    t.display(),
                    dir.display()
                ));
            }
            clone_into(key, &dir, progress)?;
            Ok(path_str(dir))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraheader_en_base64() {
        let env = git_auth_env("abc", 0);
        assert_eq!(env[0], ("GIT_CONFIG_COUNT".into(), "1".into()));
        assert_eq!(
            env[1],
            (
                "GIT_CONFIG_KEY_0".into(),
                "http.https://github.com/.extraheader".into()
            )
        );
        // base64("x-access-token:abc")
        assert_eq!(
            env[2],
            (
                "GIT_CONFIG_VALUE_0".into(),
                "AUTHORIZATION: basic eC1hY2Nlc3MtdG9rZW46YWJj".into()
            )
        );
        let decal = git_auth_env("abc", 2);
        assert_eq!(decal[0].1, "3");
        assert_eq!(decal[1].0, "GIT_CONFIG_KEY_2");
    }

    #[test]
    fn cibles() {
        assert_eq!(Target::parse("codex"), Target::Herdr);
        assert_eq!(Target::parse("herdr"), Target::Herdr);
        assert_eq!(Target::parse("Zed"), Target::Zed);
        assert_eq!(Target::parse("terminal"), Target::Terminal);
        assert_eq!(Target::parse(" Mon IDE "), Target::App("Mon IDE".into()));
        assert_eq!(Target::App("vscode".into()).id(), "vscode");
        let cfg = Config::default();
        assert_eq!(Target::App("vscode".into()).label(&cfg), "VS Code");
        assert_eq!(Target::App("vim".into()).label(&cfg), "vim");
        assert!(!available_targets(&cfg).is_empty());
    }

    #[test]
    fn cible_absente_ne_clone_pas() {
        let cfg = Config {
            projects_dir: std::env::temp_dir().join("zrepo-rs-test-ne-pas-creer"),
            ..Config::default()
        };
        let r = open(&cfg, "o/r", Target::App("zrepo-rs-absent".into()), &|_| {});
        assert_eq!(r.unwrap_err().code, EX_DEPS);
        assert!(!cfg.projects_dir.exists());
        let r = open(
            &cfg,
            "ssh://h/p",
            Target::App("zrepo-rs-absent".into()),
            &|_| {},
        );
        assert_eq!(r.unwrap_err().code, EX_DEPS);
    }

    #[test]
    fn destination_de_clonage() {
        let cfg = Config {
            projects_dir: std::env::temp_dir().join("zrepo-rs-test-inexistant"),
            ..Config::default()
        };
        assert_eq!(
            clone_destination(&cfg, "Owner/Repo"),
            Some(cfg.projects_dir.join("Repo"))
        );
        assert_eq!(clone_destination(&cfg, "pas une clé"), None);
    }
}
