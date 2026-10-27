//! Reglages : config.toml (dossier de configuration de l'OS), puis les
//! variables d'environnement `ZREPO_RS_*`, prioritaires sur le fichier.
//!
//! Tout champ absent du fichier prend sa valeur par defaut : un fichier
//! partiel ou ancien reste lisible.

use std::{
    env, fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::{
    apps::{EditorConfig, TerminalConfig},
    error::{EX_FAIL, Fail, Res},
    open::Target,
    sys,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub projects_dir: PathBuf,
    pub scan_depth: usize,
    pub cache_ttl: u64, // secondes
    pub github: GithubConfig,
    pub remote: RemoteConfig,
    pub updates: UpdateConfig,
    pub herdr: HerdrConfig,
    pub ui: UiConfig,
    pub terminal: TerminalConfig,
    /// Editeurs ajoutes a la main (`[[editors]]`), en plus de ceux detectes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub editors: Vec<EditorConfig>,

    /// Cache des depots GitHub (resolu, jamais ecrit dans le fichier).
    #[serde(skip)]
    pub cache_file: PathBuf,
    /// Cache du dernier scan des serveurs.
    #[serde(skip)]
    pub remote_cache_file: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GithubConfig {
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    pub enabled: bool,
    pub cache_ttl: u64,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cache_ttl: 86400,
        }
    }
}

/// Serveurs ajoutes dans zrepo-rs, en plus des ssh_connections de Zed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteServer {
    pub host: String,
    #[serde(default)]
    pub label: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

/// Serveurs et projets distants, declares ici ou dans Zed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<RemoteServer>,
    /// Vide = emplacement par defaut de l'OS (voir `zed_settings_path`).
    pub zed_settings: String,
    pub scan_depth: usize,
    pub ssh_timeout: u64, // secondes, par hote
}

/// Agents proposés dans l'assistant : (type Herdr, nom affiché, option qui
/// autorise toutes les actions sans confirmation).
pub const AGENTS: [(&str, &str, &str); 2] = [
    ("codex", "Codex", "--yolo"),
    ("claude", "Claude", "--dangerously-skip-permissions"),
];

fn bypass_flag(agent: &str) -> Option<&'static str> {
    AGENTS.iter().find(|a| a.0 == agent).map(|a| a.2)
}

fn is_bypass_flag(arg: &str) -> bool {
    AGENTS.iter().any(|a| a.2 == arg)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct HerdrConfig {
    pub enabled: bool,
    pub agent: String,
    pub args: Vec<String>,
    pub wait: u64, // secondes
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub default_target: Target,
    /// Raccourci clavier global, « Ctrl+Alt+R » ; vide = aucun (voir `shortcut`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub shortcut: String,
    /// Libellé affiché du raccourci, s'il diffère de la forme texte.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub shortcut_label: String,
}

impl Default for GithubConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            servers: Vec::new(),
            zed_settings: String::new(),
            scan_depth: 3,
            ssh_timeout: 8,
        }
    }
}

impl Default for HerdrConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            agent: "codex".into(),
            args: vec!["--yolo".into()],
            wait: 20,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            default_target: Target::Zed,
            shortcut: String::new(),
            shortcut_label: String::new(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let cache = cache_dir();
        Self {
            projects_dir: sys::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Projects"),
            scan_depth: 3,
            cache_ttl: 600,
            github: GithubConfig::default(),
            remote: RemoteConfig::default(),
            updates: UpdateConfig::default(),
            herdr: HerdrConfig::default(),
            ui: UiConfig::default(),
            terminal: TerminalConfig::default(),
            editors: Vec::new(),
            cache_file: cache.join("repos.tsv"),
            remote_cache_file: cache.join("remotes.tsv"),
        }
    }
}

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", "zrepo-rs")
}

/// Sans repertoire personnel, on se rabat sur le dossier temporaire plutot
/// que d'echouer : la liste reste utilisable, sans cache durable.
fn cache_dir() -> PathBuf {
    project_dirs().map_or_else(
        || env::temp_dir().join("zrepo-rs"),
        |d| d.cache_dir().to_path_buf(),
    )
}

/// Une variable vide vaut une variable absente, comme `${VAR:-defaut}`.
fn var(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.is_empty())
}

fn num<T: FromStr>(name: &str, current: T) -> T {
    var(name).and_then(|v| v.parse().ok()).unwrap_or(current)
}

/// `~/x` dans le fichier : commode quand on l'edite a la main.
fn expand_home(p: &Path) -> PathBuf {
    match (p.strip_prefix("~"), sys::home_dir()) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => p.to_path_buf(),
    }
}

/// settings.json de Zed : ~/.config/zed sous Linux et macOS (Zed n'utilise
/// pas Application Support), %APPDATA%\Zed sous Windows.
#[cfg(windows)]
fn default_zed_settings() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.config_dir().join("Zed").join("settings.json"))
        .unwrap_or_else(|| PathBuf::from(r"Zed\settings.json"))
}

#[cfg(target_os = "macos")]
fn default_zed_settings() -> PathBuf {
    sys::home_dir()
        .unwrap_or_default()
        .join(".config/zed/settings.json")
}

/// Sous Linux, XDG_CONFIG_HOME est respecte (BaseDirs le lit).
#[cfg(not(any(windows, target_os = "macos")))]
fn default_zed_settings() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.config_dir().join("zed/settings.json"))
        .unwrap_or_else(|| PathBuf::from(".config/zed/settings.json"))
}

impl Config {
    /// Chemin de config.toml.
    pub fn path() -> PathBuf {
        project_dirs().map_or_else(
            || env::temp_dir().join("zrepo-rs").join("config.toml"),
            |d| d.config_dir().join("config.toml"),
        )
    }

    pub fn exists() -> bool {
        Self::path().is_file()
    }

    /// Fichier (ou defauts s'il est absent), puis variables d'environnement.
    pub fn load() -> Res<Config> {
        let path = Self::path();
        let mut cfg = match fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text)
                .map_err(|e| Fail::new(EX_FAIL, format!("{} illisible :\n{e}", path.display())))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => {
                return Err(Fail::new(
                    EX_FAIL,
                    format!("Impossible de lire {} : {e}", path.display()),
                ));
            }
        };
        cfg.apply_env();
        Ok(cfg)
    }

    fn parse(text: &str) -> Result<Config, toml::de::Error> {
        let mut cfg: Config = toml::from_str(text)?;
        cfg.projects_dir = expand_home(&cfg.projects_dir);
        Ok(cfg)
    }

    /// Memes noms et memes defauts que la version Bash.
    fn apply_env(&mut self) {
        if let Some(d) = var("ZREPO_RS_PROJECTS_DIR") {
            self.projects_dir = PathBuf::from(d);
        }
        self.cache_ttl = num("ZREPO_RS_CACHE_TTL", self.cache_ttl);
        self.scan_depth = num("ZREPO_RS_SCAN_DEPTH", self.scan_depth);
        if let Some(z) = var("ZREPO_RS_ZED_SETTINGS") {
            self.remote.zed_settings = z;
        }
        self.remote.scan_depth = num("ZREPO_RS_REMOTE_SCAN_DEPTH", self.remote.scan_depth);
        self.remote.ssh_timeout = num("ZREPO_RS_REMOTE_SSH_TIMEOUT", self.remote.ssh_timeout);
        if let Some(a) = var("ZREPO_RS_HERDR_AGENT") {
            self.herdr.agent = a;
        }
        if let Some(a) = var("ZREPO_RS_HERDR_AGENT_ARGS") {
            self.herdr.args = a.split_whitespace().map(String::from).collect();
        }
        self.herdr.wait = num("ZREPO_RS_HERDR_WAIT", self.herdr.wait);
    }

    /// Ecriture atomique. Les surcharges d'environnement actives au moment de
    /// l'appel sont ecrites avec le reste : `save` enregistre l'etat courant.
    pub fn save(&self) -> Res<()> {
        let path = Self::path();
        let text = toml::to_string_pretty(self)
            .map_err(|e| Fail::new(EX_FAIL, format!("Configuration non sérialisable : {e}")))?;
        sys::write_atomic(&path, &text).map_err(|e| {
            Fail::new(
                EX_FAIL,
                format!("Impossible d'écrire {} : {e}", path.display()),
            )
        })
    }

    /// settings.json de Zed effectivement lu.
    pub fn zed_settings_path(&self) -> PathBuf {
        if self.remote.zed_settings.trim().is_empty() {
            default_zed_settings()
        } else {
            expand_home(Path::new(self.remote.zed_settings.trim()))
        }
    }

    /// Nom affiché de l'agent : « Codex », « Claude », ou le type tel quel.
    pub fn agent_name(&self) -> String {
        AGENTS
            .iter()
            .find(|a| a.0 == self.herdr.agent)
            .map_or_else(|| self.herdr.agent.clone(), |a| a.1.to_string())
    }

    /// L'agent démarre-t-il sans demander de confirmation (--yolo...) ?
    pub fn agent_bypass(&self) -> bool {
        self.herdr.args.iter().any(|a| is_bypass_flag(a))
    }

    /// Change d'agent : l'option sans confirmation suit (celle du nouvel
    /// agent remplace celle de l'ancien), les autres arguments restent.
    pub fn set_agent(&mut self, agent: &str) {
        let bypass = self.agent_bypass();
        self.herdr.agent = agent.to_string();
        self.set_agent_bypass(bypass);
    }

    pub fn set_agent_bypass(&mut self, on: bool) {
        self.herdr.args.retain(|a| !is_bypass_flag(a));
        if on && let Some(f) = bypass_flag(&self.herdr.agent) {
            self.herdr.args.insert(0, f.to_string());
        }
    }

    /// « codex --yolo » : l'agent tel qu'il sera lance.
    pub fn agent_label(&self) -> String {
        std::iter::once(self.herdr.agent.as_str())
            .chain(self.herdr.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fichier_partiel_complete_par_les_defauts() {
        let cfg = Config::parse(
            r#"
projects_dir = "/srv/projets"
[herdr]
agent = "claude"
[ui]
default_target = "herdr"
"#,
        )
        .unwrap();
        assert_eq!(cfg.projects_dir, PathBuf::from("/srv/projets"));
        assert_eq!(cfg.scan_depth, 3);
        assert_eq!(cfg.cache_ttl, 600);
        assert!(cfg.github.enabled);
        assert!(cfg.remote.enabled);
        assert_eq!(cfg.remote.ssh_timeout, 8);
        assert_eq!(cfg.herdr.agent, "claude");
        assert_eq!(cfg.herdr.args, vec!["--yolo".to_string()]);
        assert_eq!(cfg.ui.default_target, Target::Herdr);
        assert!(cfg.cache_file.ends_with("repos.tsv"));
        assert!(cfg.remote_cache_file.ends_with("remotes.tsv"));
    }

    #[test]
    fn fichier_vide_et_aller_retour() {
        let cfg = Config::parse("").unwrap();
        let text = toml::to_string_pretty(&cfg).unwrap();
        assert!(!text.contains("cache_file"));
        let back = Config::parse(&text).unwrap();
        assert_eq!(back.projects_dir, cfg.projects_dir);
        assert_eq!(back.herdr.args, cfg.herdr.args);
        assert_eq!(back.ui.default_target, Target::Zed);
        assert!(!text.contains("editors"));
    }

    /// Une cible inconnue n'est pas une erreur : ce peut etre un editeur
    /// declare plus loin ou desinstalle depuis. Le lanceur se rabat alors
    /// sur une cible disponible.
    #[test]
    fn champ_inconnu_et_cible_libre_toleres() {
        assert!(
            Config::parse("futur = 1\n[github]\nenabled = false\n")
                .is_ok_and(|c| !c.github.enabled)
        );
        let cfg = Config::parse("[ui]\ndefault_target = \"vim\"\n").unwrap();
        assert_eq!(cfg.ui.default_target, Target::App("vim".into()));
    }

    #[test]
    fn editeurs_et_terminal() {
        let cfg = Config::parse(
            r#"
[ui]
default_target = "terminal"
[terminal]
command = "kitty"
args = ["--directory", "{path}"]
[[editors]]
name = "Mon IDE"
command = "C:/outils/ide.exe"
args = ["--open", "{path}"]
[[editors]]
name = "Vim"
command = "gvim"
"#,
        )
        .unwrap();
        assert_eq!(cfg.ui.default_target, Target::Terminal);
        assert_eq!(cfg.terminal.command, "kitty");
        assert_eq!(cfg.editors.len(), 2);
        assert_eq!(cfg.editors[0].args, vec!["--open", "{path}"]);
        assert!(cfg.editors[1].args.is_empty());
        let back = Config::parse(&toml::to_string_pretty(&cfg).unwrap()).unwrap();
        assert_eq!(back.editors, cfg.editors);
        assert_eq!(back.terminal, cfg.terminal);
    }

    #[test]
    fn zed_settings_vide_vaut_defaut() {
        let mut cfg = Config::default();
        assert!(cfg.zed_settings_path().ends_with("settings.json"));
        cfg.remote.zed_settings = "/tmp/z.json".into();
        assert_eq!(cfg.zed_settings_path(), PathBuf::from("/tmp/z.json"));
    }

    #[test]
    fn serveurs_manuels_survivent_a_la_serialisation() {
        let cfg = Config::parse(
            "[[remote.servers]]\nhost = 'VPSHost'\nlabel = 'Production'\npaths = ['/home/jp/app']\n",
        )
        .unwrap();
        assert_eq!(cfg.remote.servers[0].host, "VPSHost");
        assert_eq!(cfg.remote.servers[0].paths, ["/home/jp/app"]);
        let back = Config::parse(&toml::to_string_pretty(&cfg).unwrap()).unwrap();
        assert_eq!(back.remote.servers, cfg.remote.servers);
    }

    #[test]
    fn choix_de_l_agent() {
        let mut cfg = Config::default();
        assert_eq!(cfg.agent_label(), "codex --yolo");
        assert!(cfg.agent_bypass());
        cfg.herdr.args.push("--search".into());
        cfg.set_agent("claude");
        assert_eq!(
            cfg.agent_label(),
            "claude --dangerously-skip-permissions --search"
        );
        assert_eq!(cfg.agent_name(), "Claude");
        cfg.set_agent_bypass(false);
        assert_eq!(cfg.agent_label(), "claude --search");
        cfg.set_agent("codex");
        assert_eq!(cfg.agent_label(), "codex --search");
        cfg.herdr.agent = "gemini".into();
        assert_eq!(cfg.agent_name(), "gemini");
        // Agent sans option connue : rien n'est ajouté.
        cfg.set_agent_bypass(true);
        assert_eq!(cfg.agent_label(), "gemini --search");
    }

    #[test]
    fn raccourci_absent_ou_enregistre() {
        let cfg = Config::default();
        assert!(cfg.ui.shortcut.is_empty());
        assert!(!toml::to_string_pretty(&cfg).unwrap().contains("shortcut"));
        let mut cfg = cfg;
        cfg.ui.shortcut = "Alt+Space".into();
        cfg.ui.shortcut_label = "⌥ Espace".into();
        let back = Config::parse(&toml::to_string_pretty(&cfg).unwrap()).unwrap();
        assert_eq!(back.ui.shortcut, "Alt+Space");
        assert_eq!(back.ui.shortcut_label, "⌥ Espace");
    }
}
