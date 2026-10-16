//! Diagnostic : dependances, connexion GitHub, chemins. Rien n'est imprime
//! ici : l'appelant affiche les verifications comme il l'entend.

use std::{fs, path::Path};

use crate::{
    apps, auth,
    config::Config,
    sources::remote::{self, Settings},
    sys,
};

#[derive(Clone, Debug)]
pub struct Check {
    pub ok: bool,
    pub label: String,
    pub detail: String,
}

fn check(ok: bool, label: &str, detail: impl Into<String>) -> Check {
    Check {
        ok,
        label: label.into(),
        detail: detail.into(),
    }
}

fn line_count(path: &Path) -> Option<usize> {
    let t = fs::read_to_string(path).ok()?;
    let n = t.lines().filter(|l| !l.is_empty()).count();
    (n > 0).then_some(n)
}

fn github(cfg: &Config) -> Check {
    if !cfg.github.enabled {
        return check(true, "GitHub", "désactivé dans config.toml");
    }
    let Some((token, source)) = auth::token() else {
        return check(false, "GitHub", "non connecté — lancez : zrepo-rs setup");
    };
    match auth::whoami(&token) {
        Ok(login) => check(
            true,
            "GitHub",
            format!("@{login} (jeton : {})", source.label()),
        ),
        Err(f) => check(
            false,
            "GitHub",
            format!("{} (jeton : {})", f.msg, source.label()),
        ),
    }
}

#[cfg(not(windows))]
fn herdr(cfg: &Config, out: &mut Vec<Check>) {
    use crate::herdr;
    if !cfg.herdr.enabled {
        out.push(check(true, "herdr", "désactivé dans config.toml"));
        return;
    }
    // Herdr est facultatif : son absence masque seulement la cible.
    let Some(p) = sys::which("herdr") else {
        out.push(check(
            true,
            "herdr",
            "absent (facultatif : la cible Herdr est masquée)",
        ));
        return;
    };
    out.push(check(true, "herdr", p.display().to_string()));
    out.push(if herdr::server_up() {
        check(
            true,
            "serveur",
            format!(
                "joignable ({} machines enregistrées)",
                herdr::machine_count()
            ),
        )
    } else {
        check(
            true,
            "serveur",
            "arrêté — il démarrera à la première ouverture",
        )
    });
    let agent = &cfg.herdr.agent;
    out.push(match sys::which(agent) {
        Some(p) => check(
            true,
            agent,
            format!("{} {}", p.display(), cfg.herdr.args.join(" ")),
        ),
        None => check(false, agent, "introuvable dans le PATH"),
    });
}

pub fn run(cfg: &Config) -> Vec<Check> {
    let mut out = Vec::new();

    out.push(match sys::which("git") {
        Some(p) => match sys::git_version() {
            Some(v) if v < sys::GIT_MIN_FOR_TOKEN => check(
                false,
                "git",
                format!(
                    "{} (version {}.{} : 2.31 ou plus est requis pour cloner les dépôts privés)",
                    p.display(),
                    v.0,
                    v.1
                ),
            ),
            Some(v) => check(true, "git", format!("{} ({}.{})", p.display(), v.0, v.1)),
            None => check(true, "git", p.display().to_string()),
        },
        None => check(
            false,
            "git",
            "introuvable (clonage et identification des dépôts impossibles)",
        ),
    });
    out.push(match sys::zed_launcher() {
        Some(z) => {
            let mut d = z.program.display().to_string();
            for a in &z.prefix {
                d.push(' ');
                d.push_str(a);
            }
            check(true, "zed", d)
        }
        // Facultatif : d'autres cibles existent, mais les projets distants
        // ne s'ouvrent que dans Zed ou Herdr.
        None => check(true, "zed", "absent (facultatif)"),
    });
    out.push(match apps::terminal(cfg) {
        Some(t) => check(true, "terminal", t.program.display().to_string()),
        None => check(
            false,
            "terminal",
            "introuvable — indiquez-en un dans config.toml, section [terminal]",
        ),
    });
    let editors = apps::editors(cfg);
    out.push(if editors.is_empty() {
        check(
            true,
            "éditeurs",
            "aucun détecté (ajout possible : zrepo-rs setup)",
        )
    } else {
        let names: Vec<&str> = editors.iter().map(|a| a.name.as_str()).collect();
        check(true, "éditeurs", names.join(", "))
    });
    out.push(github(cfg));

    if cfg.remote.enabled {
        out.push(match sys::which("ssh") {
            Some(p) => check(true, "ssh", p.display().to_string()),
            None => check(false, "ssh", "introuvable (scan des serveurs impossible)"),
        });
        let zs = cfg.zed_settings_path();
        let zs = zs.display();
        out.push(match remote::load(cfg) {
            Settings::Absent => check(true, "settings Zed", format!("{zs} (absent)")),
            Settings::Invalid(e) => check(false, "settings Zed", format!("{zs} (illisible : {e})")),
            Settings::Loaded(conns) => {
                let nproj = remote::settings_entries(&conns).len();
                check(
                    true,
                    "settings Zed",
                    format!("{zs} ({} serveurs, {nproj} projets)", conns.len()),
                )
            }
        });
    } else {
        out.push(check(true, "serveurs", "désactivés dans config.toml"));
    }

    #[cfg(not(windows))]
    herdr(cfg, &mut out);

    let path = Config::path();
    out.push(if path.is_file() {
        check(true, "config", path.display().to_string())
    } else {
        check(
            true,
            "config",
            format!("{} (absent — défauts)", path.display()),
        )
    });
    let pd = &cfg.projects_dir;
    out.push(if pd.is_dir() {
        check(true, "projets", pd.display().to_string())
    } else {
        check(
            true,
            "projets",
            format!("{} (absent — créé au premier clonage)", pd.display()),
        )
    });
    let cf = cfg.cache_file.display();
    out.push(match line_count(&cfg.cache_file) {
        Some(n) => check(true, "cache", format!("{cf} ({n} entrées)")),
        None => check(true, "cache", format!("{cf} (vide)")),
    });
    let rf = cfg.remote_cache_file.display();
    out.push(match line_count(&cfg.remote_cache_file) {
        Some(n) => check(true, "scan", format!("{rf} ({n} entrées)")),
        None => check(
            true,
            "scan",
            format!("{rf} (vide — lancez zrepo-rs list --refresh)"),
        ),
    });
    out
}
