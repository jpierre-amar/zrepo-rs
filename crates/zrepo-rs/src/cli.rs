//! Ligne de commande : analyse des arguments et sous-commandes texte
//! (`list`, `open`, `doctor`). La fenêtre est dans `gui`.

use zrepo_core::{
    config::Config,
    doctor,
    error::{EX_FAIL, Fail, Res},
    open::{self, Target},
    sources,
};

pub const USAGE: &str = "\
zrepo-rs — ouvrir un projet dans Zed, Herdr, un terminal ou votre éditeur

  zrepo-rs                       fenêtre de lancement (assistant au premier lancement)
  zrepo-rs setup                 assistant de configuration (dossier, GitHub, cibles, raccourci)
  zrepo-rs list [--refresh]      liste TSV des projets (locaux, distants, GitHub)
  zrepo-rs open <clé> [--with <cible>]
                              ouvre un projet sans fenêtre (clone si besoin)
  zrepo-rs doctor                vérifie les dépendances, les cibles et la connexion GitHub
  zrepo-rs update [--install]    vérifie la dernière release stable ; installe sur demande
  zrepo-rs --version | --help

Clés : chemin absolu (projet local), owner/repo (GitHub), ssh://hôte/chemin (distant).
Cibles : zed, herdr, terminal, ou un éditeur (vscode, cursor, windsurf, sublime,
idea, pycharm, webstorm, phpstorm, rustrover, ou le nom d'un éditeur ajouté).
Sans --with : la cible par défaut. `--with codex` équivaut à `--with herdr`.
Les projets distants (ssh://) s'ouvrent dans Zed, Herdr ou un terminal SSH.
Options de la fenêtre : --refresh (réinterroge GitHub et rescanne les serveurs),
--codex (Herdr comme cible par défaut).

Dans la fenêtre :
  Entrée      ouvre dans la cible affichée      Tab     change de cible
  Ctrl-E      ouvre dans Zed                    Ctrl-Y  ouvre dans Herdr
  Ctrl-T      ouvre un terminal dans le dossier
  ↑ ↓ PgUp PgDn  naviguent                      Ctrl-R  rafraîchit
  Ctrl-1…4    filtres Tous, Locaux, Serveurs, GitHub
  Ctrl-,      réglages                          Échap   ferme

Configuration : zrepo-rs setup, ou config.toml (chemin affiché par zrepo-rs doctor).";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Launcher {
        refresh: bool,
        target: Option<Target>,
    },
    Setup,
    List {
        refresh: bool,
    },
    Open {
        key: String,
        /// None = cible par défaut de la configuration.
        target: Option<Target>,
    },
    Doctor,
    Update {
        install: bool,
    },
    /// Interne : écoute le raccourci global (`zrepo-rs agent <touches> --open <app>`).
    Agent {
        keys: String,
        open: String,
    },
    Help,
    Version,
}

fn unknown(opt: &str) -> Fail {
    Fail::new(EX_FAIL, format!("Option inconnue : {opt}\n\n{USAGE}"))
}

pub fn parse(args: &[String]) -> Res<Command> {
    let rest = args.get(1..).unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("setup") => no_args(rest).map(|()| Command::Setup),
        Some("list") => parse_list(rest),
        Some("open") => parse_open(rest),
        Some("doctor") => no_args(rest).map(|()| Command::Doctor),
        Some("update") => match rest {
            [] => Ok(Command::Update { install: false }),
            [arg] if arg == "--install" => Ok(Command::Update { install: true }),
            [arg, ..] => Err(unknown(arg)),
        },
        Some("help") => Ok(Command::Help),
        Some("agent") => match rest {
            [keys, flag, open] if flag == "--open" => Ok(Command::Agent {
                keys: keys.clone(),
                open: open.clone(),
            }),
            _ => Err(Fail::new(
                EX_FAIL,
                "Usage interne : zrepo-rs agent <touches> --open <app>",
            )),
        },
        _ => parse_launcher(args),
    }
}

fn no_args(args: &[String]) -> Res<()> {
    match args.first() {
        Some(a) => Err(unknown(a)),
        None => Ok(()),
    }
}

/// Options globales, et celles de la v1 (`--codex`, `--doctor`).
fn parse_launcher(args: &[String]) -> Res<Command> {
    let mut refresh = false;
    let mut target = None;
    for a in args {
        match a.as_str() {
            "--help" | "-h" => return Ok(Command::Help),
            "--version" | "-V" => return Ok(Command::Version),
            "--doctor" => return Ok(Command::Doctor),
            "--refresh" | "-r" => refresh = true,
            "--codex" | "-c" => target = Some(Target::Herdr),
            other => return Err(unknown(other)),
        }
    }
    Ok(Command::Launcher { refresh, target })
}

fn parse_list(args: &[String]) -> Res<Command> {
    let mut refresh = false;
    for a in args {
        match a.as_str() {
            "--refresh" | "-r" => refresh = true,
            other => return Err(unknown(other)),
        }
    }
    Ok(Command::List { refresh })
}

fn parse_open(args: &[String]) -> Res<Command> {
    let usage = || {
        Fail::new(
            EX_FAIL,
            "Usage : zrepo-rs open <chemin|owner/repo|ssh://...> [--with <cible>]",
        )
    };
    let mut key: Option<&str> = None;
    let mut with: Option<&str> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--with" => with = Some(it.next().map(String::as_str).ok_or_else(usage)?),
            s if s.starts_with("--with=") => with = Some(&s["--with=".len()..]),
            "--" => {}
            s if s.starts_with('-') => {
                return Err(Fail::new(EX_FAIL, format!("Option inconnue : {s}")));
            }
            s if key.is_some() => {
                return Err(Fail::new(
                    EX_FAIL,
                    format!("Une seule clé attendue (reçu aussi : {s})."),
                ));
            }
            s => key = Some(s),
        }
    }
    let key = key.ok_or_else(usage)?;
    if with.is_some_and(|w| w.trim().is_empty()) {
        return Err(usage());
    }
    Ok(Command::Open {
        key: key.into(),
        target: with.map(Target::parse),
    })
}

pub fn list(refresh: bool) -> Res<()> {
    let cfg = Config::load()?;
    let (entries, warnings) = sources::collect(&cfg, refresh)?;
    for w in warnings {
        eprintln!("{w}");
    }
    let mut out = String::new();
    for e in entries {
        out.push_str(&e.to_tsv());
        out.push('\n');
    }
    print!("{out}");
    Ok(())
}

/// Comme la v1 : le résultat de l'ouverture sur stdout (scripts), les étapes
/// intermédiaires (clonage, notes, attente de Herdr) sur stderr.
pub fn open(key: &str, target: Option<Target>) -> Res<()> {
    let cfg = Config::load()?;
    let target = target.unwrap_or_else(|| {
        open::pick_target(&cfg.ui.default_target, &open::available_targets(&cfg))
    });
    open::open(&cfg, key, target, &|msg| {
        if is_final(msg) {
            println!("{msg}");
        } else {
            eprintln!("{msg}");
        }
    })
}

/// Messages de `zrepo_core::open` qui annoncent le résultat.
fn is_final(msg: &str) -> bool {
    msg.starts_with("Ouverture dans ")
        || msg.starts_with("Herdr : ")
        || msg.starts_with("Machine « ")
        || msg.contains(" tourne déjà dans ")
        || msg.contains(" démarré dans ")
}

/// Même présentation que la v1 ; le code de sortie reste 0 (diagnostic).
pub fn doctor() -> Res<()> {
    let cfg = Config::load()?;
    for c in doctor::run(&cfg) {
        let status = if c.ok { "ok    " } else { "MANQUE" };
        println!("  {status} {:<12} {}", c.label, c.detail);
    }
    Ok(())
}

pub fn update(install: bool) -> Res<()> {
    let cfg = Config::load()?;
    let status = zrepo_core::update::check(&cfg, true)?;
    println!("Version installée : {}", status.installed);
    if let Some(warning) = &status.warning {
        eprintln!("{warning} Résultat du dernier contrôle conservé.");
    }
    if let Some(release) = status.available {
        println!(
            "Version disponible : {}\n{}\n\n{}",
            release.tag_name, release.html_url, release.body
        );
        if install {
            if status.stale {
                return Err(Fail::new(
                    EX_FAIL,
                    "Installation impossible avec un contrôle de version périmé. Réessayez plus tard.",
                ));
            }
            zrepo_core::update::install(&release)?;
            println!("Mise à jour installée. Relancez zrepo-rs.");
        } else {
            println!("Pour l'installer : zrepo-rs update --install");
        }
    } else {
        println!("Aucune mise à jour disponible.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Res<Command> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn commandes() {
        assert_eq!(
            p(&[]).unwrap(),
            Command::Launcher {
                refresh: false,
                target: None
            }
        );
        assert_eq!(
            p(&["--codex", "-r"]).unwrap(),
            Command::Launcher {
                refresh: true,
                target: Some(Target::Herdr)
            }
        );
        assert_eq!(p(&["setup"]).unwrap(), Command::Setup);
        assert_eq!(p(&["doctor"]).unwrap(), Command::Doctor);
        assert_eq!(p(&["update"]).unwrap(), Command::Update { install: false });
        assert_eq!(
            p(&["update", "--install"]).unwrap(),
            Command::Update { install: true }
        );
        assert_eq!(p(&["--doctor"]).unwrap(), Command::Doctor);
        assert_eq!(p(&["--version"]).unwrap(), Command::Version);
        assert_eq!(p(&["-h"]).unwrap(), Command::Help);
        assert_eq!(
            p(&["list", "--refresh"]).unwrap(),
            Command::List { refresh: true }
        );
        assert_eq!(p(&["--bogus"]).unwrap_err().code, EX_FAIL);
        assert_eq!(p(&["setup", "x"]).unwrap_err().code, EX_FAIL);
        assert_eq!(
            p(&["agent", "Alt+Space", "--open", "/Applications/zrepo-rs.app"]).unwrap(),
            Command::Agent {
                keys: "Alt+Space".into(),
                open: "/Applications/zrepo-rs.app".into()
            }
        );
        assert!(p(&["agent", "Alt+Space"]).is_err());
    }

    #[test]
    fn sortie_de_open() {
        assert!(is_final("Ouverture dans Zed : /p"));
        assert!(is_final("Ouverture dans VS Code : /p"));
        assert!(is_final("Herdr : nouvel espace « p » dans /p"));
        assert!(is_final("codex --yolo démarré dans w1:p1 (p-codex)"));
        assert!(!is_final("Clonage de o/r…"));
        assert!(!is_final("o/r cloné dans /p"));
        assert!(!is_final("Ouverture de Herdr…"));
        assert!(!is_final(
            "Note : /p est déjà pris par un autre projet, clonage vers /q"
        ));
    }

    #[test]
    fn sous_commande_open() {
        assert_eq!(
            p(&["open", "o/r"]).unwrap(),
            Command::Open {
                key: "o/r".into(),
                target: None
            }
        );
        assert_eq!(
            p(&["open", "--with", "codex", "/p"]).unwrap(),
            Command::Open {
                key: "/p".into(),
                target: Some(Target::Herdr)
            }
        );
        assert_eq!(
            p(&["open", "--with=terminal", "--", "/p"]).unwrap(),
            Command::Open {
                key: "/p".into(),
                target: Some(Target::Terminal)
            }
        );
        // Un éditeur : sa présence n'est vérifiée qu'à l'ouverture.
        assert_eq!(
            p(&["open", "a", "--with", "vscode"]).unwrap(),
            Command::Open {
                key: "a".into(),
                target: Some(Target::App("vscode".into()))
            }
        );
        assert!(p(&["open"]).is_err());
        assert!(p(&["open", "a", "b"]).is_err());
        assert!(p(&["open", "a", "--with"]).is_err());
        assert!(p(&["open", "a", "--with="]).is_err());
    }
}
