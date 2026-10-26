//! Agent du raccourci clavier global (macOS et Windows).
//!
//! Une copie du binaire, lancée à l'ouverture de session, écoute la
//! combinaison choisie dans l'assistant et ouvre le lanceur, ou le ramène
//! devant s'il est déjà ouvert. C'est une copie, et non l'app elle-même :
//! - sous macOS, un processus du bundle zrepo-rs.app serait pris pour l'app
//!   par le Finder et le Dock ; la copie a son propre bundle, sans icône ;
//! - sous Windows, un exécutable en cours d'exécution ne peut pas être remplacé
//!   par une mise à jour.
//!
//! Sous Linux, Hyprland reçoit la combinaison dans sa configuration (`hypr`),
//! et les autres bureaux s'en chargent eux-mêmes : pas d'agent.

use std::path::Path;

#[cfg(not(any(target_os = "macos", windows)))]
use zrepo_core::{error::EX_FAIL, shortcut::Shortcut};

/// L'agent existe-t-il sur ce système ?
pub const SUPPORTED: bool = cfg!(any(target_os = "macos", windows));

#[cfg(any(target_os = "macos", windows))]
pub use imp::{install, installed, run, uninstall};

#[cfg(not(any(target_os = "macos", windows)))]
pub fn run(_keys: &str, _open: &Path) -> i32 {
    eprintln!("L'agent du raccourci n'existe que sous macOS et Windows.");
    EX_FAIL
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn install(_sc: &Shortcut) -> Result<(), String> {
    Err("Pas d'agent de raccourci sur ce système.".into())
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn uninstall() -> Result<(), String> {
    Ok(())
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn installed() -> bool {
    false
}

/// Échappement XML (plist).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// LaunchAgent : l'agent démarre à l'ouverture de session. Pas de redémarrage
/// automatique : un agent qui s'arrête (combinaison refusée) ne boucle pas.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn launch_agent_plist(label: &str, agent: &Path, keys: &str, open: &Path) -> String {
    let args = [
        agent.display().to_string(),
        "agent".into(),
        keys.into(),
        "--open".into(),
        open.display().to_string(),
    ];
    let args: String = args
        .iter()
        .map(|a| format!("    <string>{}</string>\n", xml(a)))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">
<plist version=\"1.0\">
<dict>
  <key>Label</key><string>{}</string>
  <key>ProgramArguments</key>
  <array>
{args}  </array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
",
        xml(label)
    )
}

/// Bundle .app qui contient `exe`, s'il y en a un.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn bundle_of(exe: &Path) -> Option<&Path> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app")
        .then_some(app)
}

#[cfg(any(target_os = "macos", windows))]
mod imp {
    use std::{
        path::{Path, PathBuf},
        sync::Mutex,
    };

    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
    use winit::{
        application::ApplicationHandler,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        window::WindowId,
    };
    use zrepo_core::{
        error::{EX_FAIL, EX_OK},
        shortcut::Shortcut,
    };

    struct Agent {
        open: PathBuf,
    }

    impl ApplicationHandler for Agent {
        fn resumed(&mut self, _: &ActiveEventLoop) {}

        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}

        fn user_event(&mut self, _: &ActiveEventLoop, (): ()) {
            show(&self.open);
        }
    }

    /// Écoute `keys` jusqu'à la fin de la session ; ouvre `open` à chaque appui.
    pub fn run(keys: &str, open: &Path) -> i32 {
        let Some(hotkey) = Shortcut::parse(keys).and_then(|s| s.to_string().parse::<HotKey>().ok())
        else {
            eprintln!("Combinaison illisible : {keys}");
            return EX_FAIL;
        };
        #[allow(unused_mut)]
        let mut builder = EventLoop::builder();
        #[cfg(target_os = "macos")]
        {
            // Ni icône dans le Dock ni menu : l'agent reste invisible.
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            builder.with_activation_policy(ActivationPolicy::Accessory);
        }
        let event_loop = match builder.build() {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Boucle d'événements impossible : {e}");
                return EX_FAIL;
            }
        };
        // Le gestionnaire doit naître sur le fil de la boucle d'événements.
        let manager = match GlobalHotKeyManager::new() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Raccourci global indisponible : {e}");
                return EX_FAIL;
            }
        };
        if let Err(e) = manager.register(hotkey) {
            eprintln!("Combinaison {keys} refusée (déjà utilisée ?) : {e}");
            return EX_FAIL;
        }
        #[cfg(windows)]
        let _ = std::fs::write(win::pid_file(), std::process::id().to_string());
        let proxy = Mutex::new(event_loop.create_proxy());
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            if e.state() == HotKeyState::Pressed
                && let Ok(p) = proxy.lock()
            {
                let _ = p.send_event(());
            }
        }));
        event_loop.set_control_flow(ControlFlow::Wait);
        let mut agent = Agent {
            open: open.to_path_buf(),
        };
        match event_loop.run_app(&mut agent) {
            Ok(()) => EX_OK,
            Err(e) => {
                eprintln!("{e}");
                EX_FAIL
            }
        }
    }

    /// macOS : `open` lance l'app, ou la ramène devant (et la sort du Dock si
    /// elle est réduite) si elle tourne déjà.
    #[cfg(target_os = "macos")]
    fn show(open: &Path) {
        if open.extension().is_some_and(|e| e == "app") {
            let _ = std::process::Command::new("/usr/bin/open")
                .arg(open)
                .spawn();
        } else {
            let _ = std::process::Command::new(open).spawn();
        }
    }

    /// Windows : une fenêtre « zrepo-rs » (le lanceur) est ramenée devant ;
    /// sinon on en lance une. L'agent vient de recevoir la touche : Windows
    /// l'autorise à passer une fenêtre au premier plan.
    #[cfg(windows)]
    fn show(open: &Path) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FindWindowW, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
        };
        let title: Vec<u16> = "zrepo-rs\0".encode_utf16().collect();
        // SAFETY: chaîne terminée par un zéro ; poignée vérifiée avant usage.
        unsafe {
            let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            if !hwnd.is_null() {
                if IsIconic(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                SetForegroundWindow(hwnd);
                return;
            }
        }
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new(open)
            .creation_flags(win::DETACHED_PROCESS | win::CREATE_NO_WINDOW)
            .spawn();
    }

    /// Binaire actuel, liens résolus.
    fn current_exe() -> Result<PathBuf, String> {
        std::env::current_exe()
            .and_then(|p| p.canonicalize())
            .map_err(|e| format!("Binaire actuel introuvable : {e}"))
    }

    /// Le temps que l'agent enregistre sa combinaison ou renonce.
    const START_CHECK: std::time::Duration = std::time::Duration::from_millis(1200);

    #[cfg(target_os = "macos")]
    pub use mac::{install, installed, uninstall};

    #[cfg(windows)]
    pub use win::{install, installed, uninstall};

    #[cfg(target_os = "macos")]
    mod mac {
        use std::{fs, path::PathBuf, process::Command};

        use zrepo_core::{config::Config, shortcut::Shortcut};

        use super::{START_CHECK, current_exe};
        use crate::agent::{bundle_of, launch_agent_plist, xml};

        const LABEL: &str = "io.github.jpierre-amar.zrepo-rs.agent";

        fn plist_path() -> Option<PathBuf> {
            std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(format!("Library/LaunchAgents/{LABEL}.plist")))
        }

        /// ~/Library/Application Support/zrepo-rs/zrepo-rs-agent.app
        fn agent_app() -> Option<PathBuf> {
            Config::path()
                .parent()
                .map(|d| d.join("zrepo-rs-agent.app"))
        }

        fn domain() -> Result<String, String> {
            let out = Command::new("/usr/bin/id")
                .arg("-u")
                .output()
                .map_err(|e| e.to_string())?;
            let uid = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if uid.is_empty() {
                return Err("Identifiant d'utilisateur introuvable.".into());
            }
            Ok(format!("gui/{uid}"))
        }

        fn launchctl(args: &[&str]) -> (bool, String) {
            match Command::new("/bin/launchctl").args(args).output() {
                Ok(o) => (
                    o.status.success(),
                    format!(
                        "{}{}",
                        String::from_utf8_lossy(&o.stdout),
                        String::from_utf8_lossy(&o.stderr)
                    ),
                ),
                Err(e) => (false, e.to_string()),
            }
        }

        fn stop(domain: &str) {
            let _ = launchctl(&["bootout", &format!("{domain}/{LABEL}")]);
        }

        fn info_plist() -> String {
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">
<plist version=\"1.0\">
<dict>
  <key>CFBundleExecutable</key><string>zrepo-rs-agent</string>
  <key>CFBundleIdentifier</key><string>{}</string>
  <key>CFBundleName</key><string>zrepo-rs (raccourci)</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{v}</string>
  <key>CFBundleVersion</key><string>{v}</string>
  <key>LSUIElement</key><true/>
</dict>
</plist>
",
                xml(LABEL),
                v = env!("CARGO_PKG_VERSION")
            )
        }

        pub fn install(sc: &Shortcut) -> Result<(), String> {
            let exe = current_exe()?;
            let open = bundle_of(&exe).map_or_else(|| exe.clone(), |a| a.to_path_buf());
            let app = agent_app().ok_or("Dossier de configuration introuvable.")?;
            let plist = plist_path().ok_or("Dossier personnel introuvable.")?;
            let domain = domain()?;
            let io = |what: &str, e: std::io::Error| format!("{what} : {e}");

            stop(&domain);
            let _ = fs::remove_dir_all(&app);
            let bin_dir = app.join("Contents/MacOS");
            fs::create_dir_all(&bin_dir).map_err(|e| io("Création de l'agent", e))?;
            let bin = bin_dir.join("zrepo-rs-agent");
            fs::copy(&exe, &bin).map_err(|e| io("Copie de l'agent", e))?;
            fs::write(app.join("Contents/Info.plist"), info_plist())
                .map_err(|e| io("Info.plist de l'agent", e))?;
            // Signature ad hoc, exigée sur Apple Silicon pour un bundle modifié.
            let _ = Command::new("/usr/bin/codesign")
                .args(["--force", "--sign", "-"])
                .arg(&app)
                .output();

            if let Some(d) = plist.parent() {
                fs::create_dir_all(d).map_err(|e| io("Dossier LaunchAgents", e))?;
            }
            let keys = sc.to_string();
            fs::write(&plist, launch_agent_plist(LABEL, &bin, &keys, &open))
                .map_err(|e| io("LaunchAgent", e))?;
            let (ok, out) = launchctl(&["bootstrap", &domain, &plist.to_string_lossy()]);
            if !ok {
                return Err(format!(
                    "launchctl n'a pas démarré l'agent : {}",
                    out.trim()
                ));
            }
            std::thread::sleep(START_CHECK);
            let (_, state) = launchctl(&["print", &format!("{domain}/{LABEL}")]);
            if state.contains("state = running") {
                Ok(())
            } else {
                Err(format!(
                    "L'agent de {} s'est arrêté au démarrage. Essayez une autre combinaison.",
                    sc.label()
                ))
            }
        }

        pub fn uninstall() -> Result<(), String> {
            stop(&domain()?);
            if let Some(p) = plist_path() {
                let _ = fs::remove_file(p);
            }
            if let Some(a) = agent_app() {
                let _ = fs::remove_dir_all(a);
            }
            Ok(())
        }

        pub fn installed() -> bool {
            plist_path().is_some_and(|p| p.is_file())
        }
    }

    #[cfg(windows)]
    mod win {
        use std::{
            fs,
            os::windows::process::CommandExt,
            path::PathBuf,
            process::{Command, Stdio},
        };

        use zrepo_core::shortcut::Shortcut;

        use super::{START_CHECK, current_exe};

        const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        const VALUE: &str = "zrepo-rs-agent";
        pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        pub const DETACHED_PROCESS: u32 = 0x0000_0008;

        /// %LOCALAPPDATA%\zrepo-rs\agent
        fn dir() -> PathBuf {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join(r"zrepo-rs\agent")
        }

        pub fn pid_file() -> PathBuf {
            dir().join("agent.pid")
        }

        fn quiet(cmd: &mut Command) -> bool {
            cmd.creation_flags(CREATE_NO_WINDOW)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        }

        fn stop() {
            if let Ok(pid) = fs::read_to_string(pid_file()) {
                let pid = pid.trim();
                if !pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()) {
                    quiet(Command::new("taskkill").args(["/PID", pid, "/F"]));
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
            }
            let _ = fs::remove_file(pid_file());
        }

        pub fn install(sc: &Shortcut) -> Result<(), String> {
            let exe = current_exe()?;
            let dir = dir();
            stop();
            fs::create_dir_all(&dir).map_err(|e| format!("Création de l'agent : {e}"))?;
            let agent = dir.join("zrepo-rs-agent.exe");
            fs::copy(&exe, &agent).map_err(|e| format!("Copie de l'agent : {e}"))?;
            let keys = sc.to_string();
            let line = format!(
                "\"{}\" agent {keys} --open \"{}\"",
                agent.display(),
                exe.display()
            );
            if !quiet(Command::new("reg").args([
                "add",
                RUN_KEY,
                "/v",
                VALUE,
                "/t",
                "REG_SZ",
                "/d",
                line.as_str(),
                "/f",
            ])) {
                return Err("Impossible d'inscrire l'agent au démarrage de session.".into());
            }
            let mut child = Command::new(&agent)
                .args(["agent", keys.as_str(), "--open"])
                .arg(&exe)
                .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("Démarrage de l'agent : {e}"))?;
            std::thread::sleep(START_CHECK);
            match child.try_wait() {
                Ok(None) => Ok(()),
                _ => Err(format!(
                    "{} n'a pas pu être enregistré : la combinaison est peut-être déjà \
                     prise par Windows ou une autre application. Essayez-en une autre.",
                    sc.label()
                )),
            }
        }

        pub fn uninstall() -> Result<(), String> {
            stop();
            quiet(Command::new("reg").args(["delete", RUN_KEY, "/v", VALUE, "/f"]));
            let _ = fs::remove_dir_all(dir());
            Ok(())
        }

        pub fn installed() -> bool {
            quiet(Command::new("reg").args(["query", RUN_KEY, "/v", VALUE]))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{bundle_of, launch_agent_plist};

    #[test]
    fn plist_du_launch_agent() {
        let p = launch_agent_plist(
            "io.example.agent",
            Path::new("/Users/a & b/agent"),
            "Alt+Space",
            Path::new("/Users/u/Applications/zrepo-rs.app"),
        );
        assert!(p.contains("<string>/Users/a &amp; b/agent</string>"));
        assert!(p.contains("<string>agent</string>\n    <string>Alt+Space</string>"));
        assert!(p.contains("<string>--open</string>"));
        assert!(p.contains("<key>RunAtLoad</key><true/>"));
        assert!(!p.contains("KeepAlive"));
    }

    #[test]
    fn bundle_du_binaire() {
        assert_eq!(
            bundle_of(Path::new(
                "/Users/u/Applications/zrepo-rs.app/Contents/MacOS/zrepo-rs"
            )),
            Some(Path::new("/Users/u/Applications/zrepo-rs.app"))
        );
        assert_eq!(bundle_of(Path::new("/Users/u/.local/bin/zrepo-rs")), None);
    }
}
