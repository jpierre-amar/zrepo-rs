//! Ouverture d'un projet : resolution de la cle (clonage si besoin), puis
//! Zed ou Herdr.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    config::Config,
    error::{EX_COLLISION, EX_DEPS, EX_FAIL, Fail, Res},
    herdr, sys,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Zed,
    Codex,
}

impl Target {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "zed" => Some(Target::Zed),
            "codex" => Some(Target::Codex),
            _ => None,
        }
    }

    pub fn other(self) -> Self {
        match self {
            Target::Zed => Target::Codex,
            Target::Codex => Target::Zed,
        }
    }
}

/// Le clonage a lieu avant de savoir dans quoi on ouvre : les deux cibles
/// veulent le meme dossier pret sur disque.
pub fn open(cfg: &Config, key: &str, target: Target) -> Res<()> {
    let resolved = resolve_target(cfg, key)?;
    match target {
        Target::Zed => open_in_zed(&resolved),
        Target::Codex => herdr::open(cfg, &resolved),
    }
}

fn open_in_zed(target: &str) -> Res<()> {
    let Some(bin) = sys::zed_bin() else {
        return Err(Fail::new(
            EX_DEPS,
            format!("Zed introuvable (ni 'zeditor' ni 'zed').\nLe projet est prêt ici : {target}"),
        ));
    };
    println!("Ouverture dans Zed : {target}");
    sys::spawn_detached(bin, &[target]);
    Ok(())
}

fn clone_into(key: &str, dir: &Path) -> Res<()> {
    eprintln!("Clonage de {key} vers {}", dir.display());
    let ok = Command::new("gh")
        .args(["repo", "clone", key])
        .arg(dir)
        .args(["--", "--quiet"])
        .status()
        .is_ok_and(|s| s.success());
    // On ne supprime jamais un dossier en cas d'echec : on se contente de signaler.
    if ok {
        Ok(())
    } else {
        Err(Fail::new(EX_FAIL, format!("Échec du clonage de {key}.")))
    }
}

fn is_repo(dir: &Path, want: &str) -> bool {
    dir.join(".git").is_dir() && sys::repo_identity(dir).as_deref() == Some(want)
}

fn path_str(p: PathBuf) -> String {
    p.to_string_lossy().into_owned()
}

/// Resout une cle en cible ouvrable — chemin absolu local ou URL ssh:// — en
/// clonant si besoin.
pub fn resolve_target(cfg: &Config, key: &str) -> Res<String> {
    // Projet distant : la cle est deja une adresse. Le serveur peut etre
    // eteint, c'est l'outil ouvert qui le dira.
    if key.starts_with("ssh://") {
        return Ok(key.into());
    }

    if key.starts_with('/') {
        return if Path::new(key).is_dir() {
            Ok(key.into())
        } else {
            Err(Fail::new(EX_FAIL, format!("Dossier introuvable : {key}")))
        };
    }

    let invalid = || Fail::new(EX_FAIL, format!("Clé invalide : {key}"));
    let (owner, repo) = key.split_once('/').ok_or_else(invalid)?;
    // Sans cette garde, un `want` vide correspondrait a tout depot sans origin.
    let want = sys::normalize_remote(key).ok_or_else(invalid)?;

    fs::create_dir_all(&cfg.projects_dir).map_err(|e| {
        Fail::new(
            EX_FAIL,
            format!("Impossible de créer {} : {e}", cfg.projects_dir.display()),
        )
    })?;
    let target = cfg.projects_dir.join(repo);

    if !target.exists() {
        clone_into(key, &target)?;
        return Ok(path_str(target));
    }
    if is_repo(&target, &want) {
        return Ok(path_str(target)); // deja clone, le bon depot
    }

    // Nom deja pris par autre chose : on se rabat sur owner-repo, sans rien ecraser.
    let alt = cfg.projects_dir.join(format!("{owner}-{repo}"));
    if alt.exists() {
        if is_repo(&alt, &want) {
            return Ok(path_str(alt));
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
    eprintln!(
        "Note : {} est déjà pris par un autre projet, clonage vers {}",
        target.display(),
        alt.display()
    );
    clone_into(key, &alt)?;
    Ok(path_str(alt))
}
