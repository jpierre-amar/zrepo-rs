//! `zrepo --doctor` : dependances, authentification, chemins.

use std::{fs, process::Command};

use crate::{
    config::Config,
    github, herdr,
    remote::{self, Settings},
    sys,
};

fn ok(what: &str, detail: impl std::fmt::Display) {
    println!("  ok     {what:<8} {detail}");
}

fn missing(what: &str, detail: &str) {
    println!("  MANQUE {what:<8} {detail}");
}

fn info(what: &str, detail: impl std::fmt::Display) {
    println!("  dir    {what:<8} {detail}");
}

fn line_count(path: &std::path::Path) -> Option<usize> {
    let t = fs::read_to_string(path).ok()?;
    let n = t.lines().filter(|l| !l.is_empty()).count();
    (n > 0).then_some(n)
}

pub fn run(cfg: &Config) {
    for c in ["gh", "git"] {
        match sys::which(c) {
            Some(p) => ok(c, p.display()),
            None => missing(c, ""),
        }
    }
    match sys::zed_bin().and_then(sys::which) {
        Some(p) => ok("zed", p.display()),
        None => missing("zed", "(ni zeditor ni zed)"),
    }
    if github::is_authenticated() {
        let login = sys::output_ok(Command::new("gh").args(["api", "user", "--jq", ".login"]))
            .map(|o| String::from_utf8_lossy(&o).trim().to_string())
            .unwrap_or_else(|| "connecté".into());
        ok("gh auth", login);
    } else {
        missing("gh auth", "-> lancez : gh auth login");
    }
    match sys::which("ssh") {
        Some(p) => ok("ssh", p.display()),
        None => missing("ssh", "(scan des serveurs impossible)"),
    }

    let zs = cfg.zed_settings.display();
    match remote::load(cfg) {
        Settings::Absent => info("zed", format!("{zs} (absent)")),
        Settings::Invalid(e) => info("zed", format!("{zs} (illisible : {e})")),
        Settings::Loaded(conns) => {
            let nproj = remote::settings_entries(&conns).len();
            info(
                "zed",
                format!("{zs} ({} serveurs, {nproj} projets)", conns.len()),
            );
        }
    }

    match sys::which("herdr") {
        Some(p) => {
            ok("herdr", p.display());
            if herdr::server_up() {
                ok(
                    "serveur",
                    format!(
                        "joignable ({} machines enregistrées)",
                        herdr::machine_count()
                    ),
                );
            } else {
                info("serveur", "arrêté — il démarrera à la première ouverture");
            }
        }
        None => missing(
            "herdr",
            &format!("(ouverture {} impossible)", cfg.herdr_agent_kind),
        ),
    }
    match sys::which(&cfg.herdr_agent_kind) {
        Some(p) => ok(
            &cfg.herdr_agent_kind,
            format!("{} {}", p.display(), cfg.herdr_agent_args.join(" ")),
        ),
        None => missing(&cfg.herdr_agent_kind, ""),
    }

    info("projets", cfg.projects_dir.display());
    let cf = cfg.cache_file.display();
    match line_count(&cfg.cache_file) {
        Some(n) => info("cache", format!("{cf} ({n} entrées)")),
        None => info("cache", format!("{cf} (vide)")),
    }
    let rf = cfg.remote_cache_file.display();
    match line_count(&cfg.remote_cache_file) {
        Some(n) => info("scan", format!("{rf} ({n} entrées)")),
        None => info("scan", format!("{rf} (vide — lancez zrepo --refresh)")),
    }
}
