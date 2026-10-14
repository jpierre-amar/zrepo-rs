//! zrepo — ouvrir un projet (local, distant ou GitHub) dans Zed ou dans Herdr.

mod config;
mod doctor;
mod entry;
mod error;
mod github;
mod herdr;
mod jsonc;
mod list;
mod local;
mod open;
mod remote;
mod sys;
mod tui;

use std::{
    io::{self, BufRead, IsTerminal},
    process::ExitCode,
};

use config::Config;
use error::{EX_CANCEL, EX_FAIL, EX_OK, Fail, Res};
use open::Target;

const USAGE: &str = "\
zrepo — ouvrir un projet dans Zed ou dans Herdr

  zrepo              sélecteur (projets locaux + distants + dépôts GitHub)
  zrepo --codex      idem, mais Entrée ouvre Herdr + codex --yolo
  zrepo --refresh    ignore le cache : réinterroge GitHub et rescanne
                     les serveurs déclarés dans les ssh_connections de Zed
  zrepo --doctor     vérifie dépendances et authentification
  zrepo --help       cette aide

Sous-commandes (scripts) :
  zrepo list [--refresh]                 liste TSV des projets
  zrepo open <clé> [--with zed|codex]    ouvre un projet (clone si besoin)
  zrepo doctor                           comme --doctor

Dans le sélecteur :
  ←  →  Tab   choisissent la cible : « Zed » ou « Herdr + codex --yolo »
  Entrée      ouvre le projet dans la cible affichée
  Ctrl-E      ouvre dans Zed, Ctrl-Y dans Herdr + codex --yolo
  Ctrl-R      rafraîchit (GitHub + scan des serveurs) sans quitter
  ↑ ↓ PgUp PgDn Home End   naviguent · Ctrl-U efface · Ctrl-W efface un mot
  Échap       annule

Le cache dure 10 min (ZREPO_CACHE_TTL), les projets vivent dans ~/Projects
(ZREPO_PROJECTS_DIR). Les projets distants sont lus dans les ssh_connections
de ~/.config/zed/settings.json (ZREPO_ZED_SETTINGS) et ouverts via ssh://,
sans clonage ; côté Herdr ils demandent une machine enregistrée
(`herdr machine add <hôte>`).";

fn main() -> ExitCode {
    let cfg = Config::from_env();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&cfg, &args) {
        Ok(()) => EX_OK,
        Err(f) => {
            if !f.msg.is_empty() {
                eprintln!("{}", f.msg);
            }
            f.code
        }
    };
    ExitCode::from(code.clamp(0, 255) as u8)
}

fn run(cfg: &Config, args: &[String]) -> Res<()> {
    match args.first().map(String::as_str) {
        Some("list") => cmd_list(cfg, &args[1..]),
        Some("open") => cmd_open(cfg, &args[1..]),
        Some("doctor") => {
            doctor::run(cfg);
            Ok(())
        }
        _ => cmd_ui(cfg, args),
    }
}

fn unknown(opt: &str) -> Fail {
    Fail::new(EX_FAIL, format!("Option inconnue : {opt}\n\n{USAGE}"))
}

fn cmd_list(cfg: &Config, args: &[String]) -> Res<()> {
    let mut refresh = false;
    for a in args {
        match a.as_str() {
            "--refresh" | "-r" => refresh = true,
            other => return Err(unknown(other)),
        }
    }
    let (entries, warnings) = list::collect(cfg, refresh)?;
    for w in warnings {
        eprintln!("{w}");
    }
    for e in entries {
        println!("{}", e.to_tsv());
    }
    Ok(())
}

fn cmd_open(cfg: &Config, args: &[String]) -> Res<()> {
    let usage = || {
        Fail::new(
            EX_FAIL,
            "Usage : zrepo open <chemin|owner/repo|ssh://...> [--with zed|codex]",
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
            format!("Cible inconnue : {with} (attendu : zed ou codex)"),
        )
    })?;
    open::open(cfg, key, target)
}

fn cmd_ui(cfg: &Config, args: &[String]) -> Res<()> {
    let mut refresh = false;
    let mut default = Target::Zed;
    for a in args {
        match a.as_str() {
            "--refresh" | "-r" => refresh = true,
            "--codex" | "-c" => default = Target::Codex,
            "--doctor" => {
                doctor::run(cfg);
                return Ok(());
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(unknown(other)),
        }
    }

    github::ensure_auth_interactive();

    let Some((key, target)) = tui::run(cfg, refresh, default)? else {
        return Err(Fail::new(EX_CANCEL, ""));
    };

    // Lance depuis SUPER + R, le terminal se ferme a la sortie : en cas
    // d'echec on garde le message a l'ecran jusqu'a ce qu'il soit lu.
    open::open(cfg, &key, target).map_err(|f| {
        eprintln!("{}", f.msg);
        if io::stdin().is_terminal() {
            eprintln!("\nAppuyez sur Entrée pour fermer.");
            let _ = io::stdin().lock().read_line(&mut String::new());
        }
        Fail::new(f.code, "")
    })
}
