# zrepo-rs : architecture et contrat entre les parties

zrepo-rs est un lanceur de projets multiplateforme (Linux, macOS, Windows). Il affiche
dans une **fenêtre graphique** flottante (style Raycast ou Spotlight) les projets
locaux, les projets distants (déclarés dans les `ssh_connections` de Zed) et les
dépôts GitHub du compte. Il ouvre ensuite le projet choisi dans **Zed**, ou dans
**Herdr + un agent** (codex --yolo, ou claude) quand Herdr est installé. Un dépôt GitHub
absent en local est cloné d'abord.

Langue de l'interface et des messages : **français** (avec accents).

## Arborescence

```
Cargo.toml                 workspace
crates/zrepo-core/         bibliothèque, sans aucune interface (pas de println! dans la logique)
crates/zrepo-rs/              binaire `zrepo-rs` : GUI (eframe/egui) + sous-commandes CLI
install/install.sh         installeur Linux et macOS (curl | sh)
install/install.ps1        installeur Windows (irm | iex)
packaging/                 zrepo-rs.desktop, icônes
.github/workflows/         ci.yml (check, clippy, tests sur les 3 OS), release.yml (binaires)
docs/
```

L'ancienne TUI (`tui.rs`, ratatui) est **supprimée**. Elle reste dans l'historique git
(commit c283473).

## Configuration : `config.toml`

Le fichier se trouve dans `directories::ProjectDirs::from("", "", "zrepo-rs").config_dir()`,
soit `~/.config/zrepo-rs` (Linux), `~/Library/Application Support/zrepo-rs` (macOS) et
`%APPDATA%\zrepo-rs\config` (Windows). Le cache va dans `cache_dir()` du même ProjectDirs.

```toml
projects_dir = "/home/jp/Projects"   # choisi dans l'assistant d'installation
scan_depth = 3
cache_ttl = 600                      # secondes

[github]
enabled = true                       # false = pas de dépôts GitHub dans la liste

[remote]
enabled = true
zed_settings = ""                    # vide = emplacement Zed par défaut de l'OS
scan_depth = 3
ssh_timeout = 8

[herdr]
enabled = true                       # n'apparaît que si `herdr` est dans le PATH
agent = "codex"                      # ou "claude" ; config::AGENTS
args = ["--yolo"]                    # claude : --dangerously-skip-permissions
wait = 20

[ui]
default_target = "zed"               # "zed" | "herdr"
```

Les variables d'environnement propres à la nouvelle application
(`ZREPO_RS_PROJECTS_DIR`, `ZREPO_RS_CACHE_TTL`, ...) restent prioritaires sur le
fichier. Les anciennes variables `ZREPO_*` ne sont volontairement pas lues afin
de conserver une séparation stricte avec `zrepo`. S'il n'y a pas de fichier, le
binaire lance l'assistant de configuration.

Emplacement par défaut de `settings.json` de Zed : `~/.config/zed/settings.json`
sous Linux et macOS, `%APPDATA%\Zed\settings.json` sous Windows.

## Accès GitHub (sans gh)

- **Connexion par le navigateur** (OAuth Device Flow), via `POST https://github.com/login/device/code`
  puis le polling de `https://github.com/login/oauth/access_token`, avec `scope=repo read:org`.
  Le client_id est compilé avec `option_env!("ZREPO_RS_GITHUB_CLIENT_ID")`. S'il est absent,
  l'assistant propose les deux options suivantes.
- **Jeton personnel** (PAT) collé dans l'assistant.
- **Jeton de gh** : `gh auth token`, si gh est installé et connecté.
- Le jeton est stocké dans le **trousseau du système** (crate `keyring`, service
  `zrepo-rs`, compte `github`), jamais dans config.toml.
  Ordre de lecture : variable `GITHUB_TOKEN` ou `ZREPO_RS_GITHUB_TOKEN`, puis le trousseau, puis `gh auth token`.
- API : `ureq` (rustls), `GET https://api.github.com/user/repos?per_page=100&sort=updated&affiliation=owner,collaborator,organization_member`,
  avec la pagination par l'en-tête `Link`.
- **Clonage** : `git clone https://github.com/owner/repo <dir>`. Le jeton passe par les variables
  d'environnement `GIT_CONFIG_COUNT`, `GIT_CONFIG_KEY_0` et `GIT_CONFIG_VALUE_0`
  (`http.https://github.com/.extraheader` = `AUTHORIZATION: basic base64("x-access-token:" + token)`).
  Il n'apparaît jamais sur la ligne de commande ni dans `.git/config`.

## API publique de `zrepo-core` (contrat)

```rust
pub mod error  { pub struct Fail { pub code: i32, pub msg: String }  pub type Res<T>; EX_* }
pub mod entry  { pub enum Kind { Local, Remote, Github }  pub struct Entry { kind, key, name, owner, visibility, description, localpath } }
pub mod config {
    pub struct Config { /* champs ci-dessus, + chemins cache résolus */ }
    impl Config {
        pub fn load() -> Res<Config>;          // fichier + env ; défauts si absent
        pub fn exists() -> bool;               // le fichier existe-t-il ?
        pub fn save(&self) -> Res<()>;
        pub fn path() -> PathBuf;
    }
}
pub mod auth {
    pub const CLIENT_ID: Option<&str>;
    pub enum TokenSource { Env, Keyring, GhCli }
    pub fn token() -> Option<(String, TokenSource)>;
    pub fn store_token(t: &str) -> Res<()>;
    pub fn delete_token() -> Res<()>;
    pub fn whoami(token: &str) -> Res<String>;            // login GitHub
    pub struct DeviceCode { pub user_code, pub verification_uri, pub device_code, pub interval: u64, pub expires_in: u64 }
    pub fn device_start(client_id: &str) -> Res<DeviceCode>;
    pub enum Poll { Pending, SlowDown, Done(String) }
    pub fn device_poll(client_id: &str, dc: &DeviceCode) -> Res<Poll>;   // un seul appel, non bloquant au-delà de la requête
    pub fn gh_cli_token() -> Option<String>;
}
pub mod sources {   // chaque source est indépendante, appelable depuis un thread
    pub fn locals(cfg: &Config) -> Vec<Entry>;
    pub fn remotes(cfg: &Config, refresh: bool) -> (Vec<Entry>, Vec<String> /*avertissements*/);
    pub fn github(cfg: &Config, refresh: bool) -> Res<Vec<Entry>>;
    pub fn combine(locals: &[Entry], remotes: &[Entry], github: &[Entry]) -> Vec<Entry>;
    pub fn collect(cfg: &Config, refresh: bool) -> Res<(Vec<Entry>, Vec<String>)>;
}
pub mod open {
    /// Écrite comme une chaîne dans config.toml : "zed", "herdr", "terminal", ou l'id d'un éditeur.
    pub enum Target { Zed, Herdr, Terminal, App(String) }
    pub fn available_targets(cfg: &Config) -> Vec<Target>;   // cibles installées ; jamais vide
    pub fn pick_target(want: &Target, targets: &[Target]) -> Target;   // repli si `want` est absent
    /// Résout (clone si besoin) puis ouvre. `progress` reçoit les messages d'étape.
    /// Terminal et éditeurs : vérifiés avant le clonage ; refusés pour un projet ssh://.
    pub fn open(cfg: &Config, key: &str, target: Target, progress: &dyn Fn(&str)) -> Res<()>;
}
pub mod apps {   // terminal et éditeurs
    pub struct EditorConfig { pub name, pub command, pub args }   // [[editors]] ; {path} = dossier
    pub struct TerminalConfig { pub command, pub args }           // [terminal] ; vide = détection
    pub fn editors(cfg: &Config) -> Vec<App>;     // déclarés d'abord, puis éditeurs connus détectés
    pub fn terminal(cfg: &Config) -> Option<App>;
    pub fn launch(app: &App, dir: &str, progress: &dyn Fn(&str)) -> Res<()>;
}
pub mod path {   // PATH du processus
    pub fn merge_paths(current: &OsStr, extra: impl IntoIterator<Item = PathBuf>) -> Option<(OsString, Vec<PathBuf>)>;
    #[cfg(target_os = "macos")]
    pub fn completed() -> Option<(OsString, Vec<PathBuf>)>;   // PATH du shell de connexion + dossiers usuels ; None si rien ne manque
    pub fn note_added(added: Vec<PathBuf>);                   // appelé par main après set_var
    pub fn added() -> Option<&'static [PathBuf]>;             // pour doctor
}
pub mod shortcut {   // combinaison globale, « Ctrl+Alt+R » ([ui] shortcut / shortcut_label)
    pub struct Shortcut { pub ctrl, pub alt, pub shift, pub super_key: bool, pub key: String }
    impl Shortcut {
        pub fn parse(s: &str) -> Option<Shortcut>;   // au moins un modificateur, sauf F1–F24
        pub fn suggested() -> Shortcut;               // ⌥ Espace, Ctrl+Alt+R, Super+R
        pub fn label(&self) -> String;
        pub fn hypr_lua(&self) -> String;             // « SUPER + ALT + R »
        pub fn hypr_conf(&self) -> (String, String);  // (« SUPER ALT », « R »)
    }
}
pub mod doctor { pub struct Check { pub ok: bool, pub label: String, pub detail: String }  pub fn run(cfg: &Config) -> Vec<Check>; }
```

## Spécificités de chaque OS (dans `zrepo-core`, derrière `cfg(target_os)`)

| | Linux | macOS | Windows |
|---|---|---|---|
| Binaire Zed | `zeditor` puis `zed` | `zed`, sinon `open -a Zed <chemin>` | `zed` dans le PATH, sinon `%LOCALAPPDATA%\Programs\Zed\bin\zed.exe` |
| Lancement détaché | `setsid -f` s'il existe, sinon spawn | spawn (ou `open`) | `CREATE_NO_WINDOW` et `DETACHED_PROCESS` |
| Herdr | oui. Focus Hyprland seulement si `hyprctl` est présent | oui, sans focus de fenêtre | désactivé |
| Terminal | `$TERMINAL`, puis ghostty, kitty, alacritty, foot, wezterm, gnome-terminal, konsole, xfce4-terminal, x-terminal-emulator, xterm | `open -a Terminal <chemin>` | `wt -d <chemin>`, sinon `pwsh`/`powershell -NoExit` dans une nouvelle console |
| Éditeurs connus | PATH | PATH, sinon `open -a <App>` si `/Applications/<App>.app` existe | PATH, sinon dossiers d'installation (`%LOCALAPPDATA%\Programs\…`) |
| `/proc` | oui | non | non |
| PATH au démarrage | inchangé | complété si lancé hors terminal (shell de connexion, délai 2 s, puis `~/.local/bin`, `~/.cargo/bin`, `/opt/homebrew/bin`, `/usr/local/bin`) | inchangé |
| Scan distant | `ssh` | `ssh` | `ssh.exe` (OpenSSH Windows) |

Aucun appel aux commandes `omarchy-*` n'est obligatoire. On les utilise si elles existent.

## Le binaire `zrepo-rs`

```
zrepo-rs                  fenêtre de lancement (assistant au premier lancement)
zrepo-rs setup            assistant de configuration (dossier, GitHub, cibles, raccourci)
zrepo-rs list [--refresh] TSV (compatible v1)
zrepo-rs open <clé> [--with <cible>]   (sans --with : cible par défaut ; codex = alias de herdr)
zrepo-rs doctor           diagnostic texte
zrepo-rs --version | --help
```
Sous Windows, le binaire est `windows_subsystem = "windows"`. On rattache la console
(`AttachConsole`) pour les sous-commandes texte.

### Fenêtre de lancement (eframe/egui)
- Fenêtre sans bordure, centrée, d'environ 760×520, coins arrondis, ombre.
  Elle reste ouverte quand elle perd le focus ; boutons Réduire et Fermer en haut à droite,
  Échap ferme aussi.
- Champ de recherche en haut (focus immédiat), recherche floue `nucleo-matcher` avec les lettres trouvées surlignées.
- Filtres en pastilles : Tous, Locaux, Serveurs, GitHub, avec un compteur pour chacun.
- Liste virtualisée. Chaque ligne a une icône par type, le nom, l'owner ou le serveur, et un badge de visibilité.
  La ligne sélectionnée est mise en évidence. Navigation au clavier (↑ ↓, PgUp et PgDn) et à la souris (survol, double-clic).
- Panneau de détails : chemin, origine, description, et « sera cloné dans … ».
- Barre du bas : choix de la cible (segments jusqu'à trois cibles, liste déroulante au-delà), aide clavier,
  état du chargement (spinner) et avertissements.
- Raccourcis : Entrée ouvre, Ctrl-E ouvre dans Zed, Ctrl-Y dans Herdr, Ctrl-T dans un terminal, Tab change la cible,
  Ctrl-R rafraîchit, Ctrl-, ouvre les réglages.
- Chargement asynchrone : les projets locaux s'affichent tout de suite, les serveurs et GitHub arrivent ensuite.
- Ouverture : le clonage est montré dans la fenêtre (progression), puis la fenêtre se ferme. En cas d'erreur,
  le message reste affiché dans la fenêtre.
- Thème sombre ou clair selon le système, avec une couleur d'accent.

### Assistant (premier lancement et `zrepo-rs setup`)
1. Bienvenue.
2. Dossier des projets : sélecteur natif (`rfd`), `~/Projects` par défaut.
3. GitHub : connexion par le navigateur (le code s'affiche en grand, avec un bouton « Copier et ouvrir github.com »
   et un polling en arrière-plan), ou un jeton collé, ou le jeton de gh, ou « Plus tard ». Affiche « Connecté en tant que @login ».
4. Cibles : Zed, Herdr, terminal et éditeurs connus détectés ou non ; « Mes éditeurs » (ajout d'un exécutable
   avec le sélecteur de fichiers, retrait) ; cible par défaut parmi les cibles disponibles.
5. Raccourci clavier : « Changer » puis la combinaison au clavier (Échap annule). Sous macOS et Windows,
   « Activer » installe l'agent (`agent.rs`) : une copie du binaire, lancée à l'ouverture de session
   (LaunchAgent / clé `Run`), qui écoute la combinaison (`global-hotkey`, boucle `winit`) et ouvre le
   lanceur ou le ramène devant (`open <app>` sous macOS, `FindWindow` + `SetForegroundWindow` sous
   Windows). C'est une copie : sous macOS, un processus du bundle serait pris pour l'app par le Finder ;
   sous Windows, l'exécutable en service ne pourrait pas être mis à jour. Sous Hyprland, la combinaison
   choisie est écrite (aperçu, puis confirmation) ; ailleurs sous Linux, instructions avec la combinaison.
6. Terminé : enregistre `config.toml` et propose d'ouvrir le lanceur.

## Distribution

- Dépôt GitHub `jpierre-amar/zrepo-rs`. Les releases sont déclenchées par un tag `v*`.
- Artefacts : `zrepo-rs-<target>.tar.gz` (Linux et macOS) et `zrepo-rs-<target>.zip` (Windows), chacun contenant
  le binaire `zrepo-rs` (ou `zrepo-rs.exe`) et `LICENSE`. Plus des fichiers `.sha256`.
  Cibles : `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`.
- `install.sh` : détecte l'OS et l'architecture, télécharge la dernière release (ou `ZREPO_RS_VERSION`), vérifie le sha256,
  et installe dans `~/.local/bin` (ou le dossier `ZREPO_RS_INSTALL_DIR`). Sous Linux il installe le `.desktop` et l'icône,
  sous macOS il crée `~/Applications/zrepo-rs.app` (un bundle minimal signé ad hoc) et fait de `~/.local/bin/zrepo-rs`
  un lien symbolique vers son binaire, pour que le trousseau ne voie qu'une application. Il lance ensuite `zrepo-rs setup`.
- `install.ps1` : installe dans `%LOCALAPPDATA%\Programs\zrepo-rs`, ajoute ce dossier au PATH utilisateur,
  crée un raccourci dans le menu Démarrer, puis lance `zrepo-rs setup`.
- Les deux installeurs demandent le dossier d'installation (avec le défaut ci-dessus), sauf avec `-y` ou `--yes`.
