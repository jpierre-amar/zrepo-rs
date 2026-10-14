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
zrepo-rs — ouvrir un projet dans Zed ou dans Herdr

  zrepo-rs                       fenêtre de lancement (assistant au premier lancement)
  zrepo-rs setup                 assistant de configuration (dossier, GitHub, cibles, raccourci)
  zrepo-rs list [--refresh]      liste TSV des projets (locaux, distants, GitHub)
  zrepo-rs open <clé> [--with zed|herdr]
                              ouvre un projet sans fenêtre (clone si besoin)
  zrepo-rs doctor                vérifie les dépendances et la connexion GitHub
  zrepo-rs --version | --help

Clés : chemin absolu (projet local), owner/repo (GitHub), ssh://hôte/chemin (distant).
`--with codex` reste accepté comme synonyme de `--with herdr`.
Options de la fenêtre : --refresh (réinterroge GitHub et rescanne les serveurs),
--codex (Herdr comme cible par défaut).

Dans la fenêtre :
  Entrée      ouvre dans la cible affichée      Tab     change de cible
  Ctrl-E      ouvre dans Zed                    Ctrl-Y  ouvre dans Herdr
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
        target: Target,
    },
    Doctor,
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
        Some("help") => Ok(Command::Help),
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
            "Usage : zrepo-rs open <chemin|owner/repo|ssh://...> [--with zed|herdr]",
        )
    };
    let mut key: Option<&str> = None;
    let mut with = "zed";
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--with" => with = it.next().map(String::as_str).ok_or_else(usage)?,
            s if s.starts_with("--with=") => with = &s["--with=".len()..],
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
    let target = Target::parse(with).ok_or_else(|| {
        Fail::new(
            EX_FAIL,
            format!("Cible inconnue : {with} (attendu : zed ou herdr)"),
        )
    })?;
    Ok(Command::Open {
        key: key.into(),
        target,
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
pub fn open(key: &str, target: Target) -> Res<()> {
    let cfg = Config::load()?;
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
    msg.starts_with("Ouverture dans Zed : ")
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
        assert_eq!(p(&["--doctor"]).unwrap(), Command::Doctor);
        assert_eq!(p(&["--version"]).unwrap(), Command::Version);
        assert_eq!(p(&["-h"]).unwrap(), Command::Help);
        assert_eq!(
            p(&["list", "--refresh"]).unwrap(),
            Command::List { refresh: true }
        );
        assert_eq!(p(&["--bogus"]).unwrap_err().code, EX_FAIL);
        assert_eq!(p(&["setup", "x"]).unwrap_err().code, EX_FAIL);
    }

    #[test]
    fn sortie_de_open() {
        assert!(is_final("Ouverture dans Zed : /p"));
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
                target: Target::Zed
            }
        );
        assert_eq!(
            p(&["open", "--with", "codex", "/p"]).unwrap(),
            Command::Open {
                key: "/p".into(),
                target: Target::Herdr
            }
        );
        assert_eq!(
            p(&["open", "--with=herdr", "--", "/p"]).unwrap(),
            Command::Open {
                key: "/p".into(),
                target: Target::Herdr
            }
        );
        assert!(p(&["open"]).is_err());
        assert!(p(&["open", "a", "b"]).is_err());
        assert!(p(&["open", "a", "--with", "vim"]).is_err());
        assert!(p(&["open", "a", "--with"]).is_err());
    }
}
