//! Cibles d'ouverture autres que Zed et Herdr : un terminal dans le dossier du
//! projet, les editeurs detectes (VS Code, Cursor...) et ceux declares dans
//! config.toml (`[[editors]]`).

use std::{
    env,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    error::{EX_FAIL, Fail, Res},
    sys,
};

/// Editeur declare dans config.toml. `{path}` dans `args` est remplace par le
/// dossier du projet ; sans `{path}`, le dossier est ajoute a la fin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// Terminal impose. `command` vide = detection automatique. Le terminal est
/// lance dans le dossier du projet ; `{path}` dans `args` y est aussi remplace.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub command: String,
    pub args: Vec<String>,
}

/// Programme pret a lancer pour une cible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub id: String,
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Ajouter le dossier a la fin quand `args` ne contient pas `{path}`.
    pub append_path: bool,
    /// Windows : programme console a ouvrir dans sa propre fenetre.
    pub console: bool,
}

impl App {
    pub fn args_for(&self, dir: &str) -> Vec<String> {
        let mut out: Vec<String> = self.args.iter().map(|a| a.replace("{path}", dir)).collect();
        if self.append_path && !self.args.iter().any(|a| a.contains("{path}")) {
            out.push(dir.to_string());
        }
        out
    }
}

/// Editeur connu, cherche dans le PATH puis aux emplacements d'installation
/// habituels.
struct Preset {
    id: &'static str,
    name: &'static str,
    bins: &'static [&'static str],
    /// Windows : (variable d'environnement, chemin relatif).
    windows: &'static [(&'static str, &'static str)],
    /// macOS : nom de l'application pour `open -a`.
    mac_app: &'static str,
}

const PRESETS: &[Preset] = &[
    Preset {
        id: "vscode",
        name: "VS Code",
        bins: &["code"],
        windows: &[
            ("LOCALAPPDATA", r"Programs\Microsoft VS Code\bin\code.cmd"),
            ("ProgramFiles", r"Microsoft VS Code\bin\code.cmd"),
        ],
        mac_app: "Visual Studio Code",
    },
    Preset {
        id: "cursor",
        name: "Cursor",
        bins: &["cursor"],
        windows: &[(
            "LOCALAPPDATA",
            r"Programs\cursor\resources\app\bin\cursor.cmd",
        )],
        mac_app: "Cursor",
    },
    Preset {
        id: "windsurf",
        name: "Windsurf",
        bins: &["windsurf"],
        windows: &[("LOCALAPPDATA", r"Programs\Windsurf\bin\windsurf.cmd")],
        mac_app: "Windsurf",
    },
    Preset {
        id: "sublime",
        name: "Sublime Text",
        bins: &["subl"],
        windows: &[("ProgramFiles", r"Sublime Text\subl.exe")],
        mac_app: "Sublime Text",
    },
    Preset {
        id: "idea",
        name: "IntelliJ IDEA",
        bins: &["idea"],
        windows: &[],
        mac_app: "IntelliJ IDEA",
    },
    Preset {
        id: "pycharm",
        name: "PyCharm",
        bins: &["pycharm"],
        windows: &[],
        mac_app: "PyCharm",
    },
    Preset {
        id: "webstorm",
        name: "WebStorm",
        bins: &["webstorm"],
        windows: &[],
        mac_app: "WebStorm",
    },
    Preset {
        id: "phpstorm",
        name: "PhpStorm",
        bins: &["phpstorm"],
        windows: &[],
        mac_app: "PhpStorm",
    },
    Preset {
        id: "rustrover",
        name: "RustRover",
        bins: &["rustrover"],
        windows: &[],
        mac_app: "RustRover",
    },
];

/// Nom affiche d'un editeur connu ou declare ; None si l'id est inconnu.
pub fn editor_name(cfg: &Config, id: &str) -> Option<String> {
    cfg.editors
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(id))
        .map(|e| e.name.clone())
        .or_else(|| {
            PRESETS
                .iter()
                .find(|p| p.id.eq_ignore_ascii_case(id))
                .map(|p| p.name.to_string())
        })
}

fn mac_app_exists(app: &str) -> bool {
    let name = format!("{app}.app");
    std::iter::once(PathBuf::from("/Applications").join(&name))
        .chain(sys::home_dir().map(|h| h.join("Applications").join(&name)))
        .any(|p| p.is_dir())
}

fn detect_preset(p: &Preset) -> Option<App> {
    let app = |program: PathBuf, args: Vec<String>| App {
        id: p.id.into(),
        name: p.name.into(),
        program,
        args,
        append_path: true,
        console: false,
    };
    if let Some(bin) = p.bins.iter().find_map(|b| sys::which(b)) {
        return Some(app(bin, Vec::new()));
    }
    if cfg!(windows) {
        return p
            .windows
            .iter()
            .filter_map(|(var, rel)| Some(PathBuf::from(env::var_os(var)?).join(rel)))
            .find(|f| f.is_file())
            .map(|f| app(f, Vec::new()));
    }
    if cfg!(target_os = "macos") && mac_app_exists(p.mac_app) {
        return Some(app(
            PathBuf::from("open"),
            vec!["-a".into(), p.mac_app.into()],
        ));
    }
    None
}

/// Chemin tel quel s'il contient un separateur, sinon recherche dans le PATH.
fn resolve_command(command: &str) -> Option<PathBuf> {
    let c = command.trim();
    if c.is_empty() {
        return None;
    }
    let p = Path::new(c);
    if p.components().count() > 1 || p.is_absolute() {
        p.is_file().then(|| p.to_path_buf())
    } else {
        sys::which(c)
    }
}

/// Editeurs disponibles : ceux de config.toml d'abord, puis les editeurs
/// connus detectes (sauf s'ils portent le nom d'un editeur declare).
pub fn editors(cfg: &Config) -> Vec<App> {
    let mut out: Vec<App> = cfg
        .editors
        .iter()
        .filter(|e| !e.name.trim().is_empty())
        .filter_map(|e| {
            Some(App {
                id: e.name.clone(),
                name: e.name.clone(),
                program: resolve_command(&e.command)?,
                args: e.args.clone(),
                append_path: true,
                console: false,
            })
        })
        .collect();
    for p in PRESETS {
        let taken = out
            .iter()
            .any(|a| a.id.eq_ignore_ascii_case(p.id) || a.name.eq_ignore_ascii_case(p.name));
        if !taken && let Some(a) = detect_preset(p) {
            out.push(a);
        }
    }
    out
}

pub fn find_editor(cfg: &Config, id: &str) -> Option<App> {
    editors(cfg)
        .into_iter()
        .find(|a| a.id.eq_ignore_ascii_case(id))
}

fn terminal_app(program: PathBuf, args: &[&str], console: bool) -> App {
    App {
        id: "terminal".into(),
        name: "Terminal".into(),
        program,
        args: args.iter().map(|s| s.to_string()).collect(),
        append_path: false,
        console,
    }
}

#[cfg(windows)]
fn default_terminal() -> Option<App> {
    // Windows Terminal ; sinon PowerShell dans sa propre console.
    if let Some(wt) = sys::which("wt") {
        return Some(terminal_app(wt, &["-d", "{path}"], false));
    }
    ["pwsh", "powershell"]
        .into_iter()
        .find_map(sys::which)
        .map(|p| terminal_app(p, &["-NoExit"], true))
}

#[cfg(target_os = "macos")]
fn default_terminal() -> Option<App> {
    Some(terminal_app(
        PathBuf::from("open"),
        &["-a", "Terminal", "{path}"],
        false,
    ))
}

/// $TERMINAL d'abord, puis les terminaux courants avec leur option de dossier
/// de depart (le dossier courant suffit aux autres).
#[cfg(not(any(windows, target_os = "macos")))]
fn default_terminal() -> Option<App> {
    if let Some(t) = env::var("TERMINAL").ok().and_then(|t| resolve_command(&t)) {
        return Some(terminal_app(t, &[], false));
    }
    const KNOWN: &[(&str, &[&str])] = &[
        ("ghostty", &["--working-directory={path}"]),
        ("kitty", &["--directory", "{path}"]),
        ("alacritty", &["--working-directory", "{path}"]),
        ("foot", &["--working-directory={path}"]),
        ("wezterm", &["start", "--cwd", "{path}"]),
        ("gnome-terminal", &["--working-directory={path}"]),
        ("konsole", &["--workdir", "{path}"]),
        ("xfce4-terminal", &["--working-directory={path}"]),
        ("x-terminal-emulator", &[]),
        ("xterm", &[]),
    ];
    KNOWN
        .iter()
        .find_map(|(bin, args)| sys::which(bin).map(|p| terminal_app(p, args, false)))
}

/// Terminal de config.toml s'il est renseigne, sinon celui du systeme.
pub fn terminal(cfg: &Config) -> Option<App> {
    let t = &cfg.terminal;
    if t.command.trim().is_empty() {
        return default_terminal();
    }
    let program = resolve_command(&t.command)?;
    let args: Vec<&str> = t.args.iter().map(String::as_str).collect();
    Some(terminal_app(program, &args, false))
}

/// Lance l'application sur le dossier `dir` (deja present sur le disque).
pub fn launch(app: &App, dir: &str, progress: &dyn Fn(&str)) -> Res<()> {
    progress(&format!("Ouverture dans {} : {dir}", app.name));
    let args = app.args_for(dir);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    #[cfg(windows)]
    let ok = if app.console {
        sys::spawn_console(&app.program, &args, Path::new(dir))
    } else {
        sys::spawn_detached_in(&app.program, &args, Some(Path::new(dir)))
    };
    #[cfg(not(windows))]
    let ok = sys::spawn_detached_in(&app.program, &args, Some(Path::new(dir)));
    if ok {
        Ok(())
    } else {
        Err(Fail::new(
            EX_FAIL,
            format!(
                "Impossible de lancer {}.\nLe projet est prêt ici : {dir}",
                app.program.display()
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(args: &[&str], append_path: bool) -> App {
        App {
            id: "x".into(),
            name: "X".into(),
            program: PathBuf::from("x"),
            args: args.iter().map(|s| s.to_string()).collect(),
            append_path,
            console: false,
        }
    }

    #[test]
    fn arguments_et_dossier() {
        assert_eq!(app(&[], true).args_for("/p"), vec!["/p"]);
        assert_eq!(app(&["-n"], true).args_for("/p"), vec!["-n", "/p"]);
        assert_eq!(
            app(&["--goto", "{path}"], true).args_for("/p q"),
            vec!["--goto", "/p q"]
        );
        assert_eq!(
            app(&["--working-directory={path}"], false).args_for("/p"),
            vec!["--working-directory=/p"]
        );
        assert!(app(&[], false).args_for("/p").is_empty());
    }

    #[test]
    fn editeur_declare_prioritaire() {
        let git = sys::which("git").unwrap();
        let cfg = Config {
            editors: vec![
                EditorConfig {
                    name: "Mon IDE".into(),
                    command: git.display().to_string(),
                    args: vec!["{path}".into()],
                },
                EditorConfig {
                    name: "Absent".into(),
                    command: "zrepo-rs-editeur-qui-n-existe-pas".into(),
                    args: Vec::new(),
                },
            ],
            ..Config::default()
        };
        let list = editors(&cfg);
        assert_eq!(list[0].name, "Mon IDE");
        assert_eq!(list[0].program, git);
        assert!(list.iter().all(|a| a.name != "Absent"));
        assert!(find_editor(&cfg, "mon ide").is_some());
        assert_eq!(editor_name(&cfg, "vscode").as_deref(), Some("VS Code"));
        assert_eq!(editor_name(&cfg, "MON IDE").as_deref(), Some("Mon IDE"));
        assert_eq!(editor_name(&cfg, "vim"), None);
    }

    #[test]
    fn terminal_impose() {
        let git = sys::which("git").unwrap();
        let mut cfg = Config::default();
        cfg.terminal.command = git.display().to_string();
        cfg.terminal.args = vec!["-C".into(), "{path}".into()];
        let t = terminal(&cfg).unwrap();
        assert_eq!(t.program, git);
        assert_eq!(t.args_for("/p"), vec!["-C", "/p"]);
        cfg.terminal.command = "zrepo-rs-terminal-qui-n-existe-pas".into();
        assert!(terminal(&cfg).is_none());
    }
}
