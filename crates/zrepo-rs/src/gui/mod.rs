//! Interface graphique (eframe/egui) : la fenêtre de lancement et l'assistant.
//!
//! Variables cachées, pour le débogage et les captures d'écran :
//! - `ZREPO_RS_DEBUG_SETUP_STEP=n` : l'assistant s'ouvre à l'étape n (1 à 6) ;
//! - `ZREPO_RS_DEBUG_KEEP_OPEN=1` : le lanceur ne se ferme pas en perdant le focus ;
//! - `ZREPO_RS_DEBUG_QUERY=texte` : recherche pré-remplie ;
//! - `ZREPO_RS_DEBUG_THEME=light|dark` : force le thème ;
//! - `ZREPO_RS_DEBUG_DEVICE_CODE=ABCD-1234` : écran du code GitHub sans réseau.

mod launcher;
mod search;
mod setup;
mod theme;

use std::{
    cell::Cell,
    process::{Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Color32, CornerRadius, Event, Key, Modifiers, Stroke, StrokeKind, ThemePreference,
    UiBuilder, ViewportBuilder, ViewportCommand,
};
use zrepo_core::{
    config::Config,
    error::{EX_CANCEL, EX_FAIL, EX_OK},
    open::Target,
};

use self::{launcher::Launcher, setup::Setup, theme::Pal};

pub enum Mode {
    Launcher {
        refresh: bool,
        target: Option<Target>,
    },
    Setup,
}

/// Carte visible du lanceur, et la marge transparente qui porte son ombre.
const CARD: [f32; 2] = [760.0, 540.0];
const MARGIN: f32 = 18.0;
const RADIUS: u8 = 14;
/// Délai avant de fermer sur perte du focus : le compositeur peut donner le
/// focus un peu après l'affichage.
const FOCUS_GRACE: Duration = Duration::from_millis(700);

pub fn icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../../../../packaging/zrepo-rs.png"))
        .unwrap_or_default()
}

/// Ouvre la fenêtre ; rend le code de sortie du processus.
pub fn run(mode: Mode) -> i32 {
    // Premier lancement : l'assistant d'abord, il relance le lanceur à la fin.
    let mode = match mode {
        Mode::Launcher { .. } if !Config::exists() => Mode::Setup,
        m => m,
    };
    let exit = Rc::new(Cell::new(EX_CANCEL));
    let frameless = matches!(mode, Mode::Launcher { .. });
    let base = ViewportBuilder::default()
        .with_app_id("zrepo-rs")
        .with_icon(icon());
    let viewport = if frameless {
        base.with_title("zrepo-rs")
            .with_inner_size([CARD[0] + 2.0 * MARGIN, CARD[1] + 2.0 * MARGIN])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_always_on_top()
    } else {
        base.with_title("zrepo-rs — configuration")
            .with_inner_size([720.0, 580.0])
            .with_min_inner_size([620.0, 520.0])
    };
    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        // Sans vsync : sous Wayland, eglSwapBuffers attendrait le compositeur,
        // qui ne rythme plus une fenêtre masquée (autre espace de travail,
        // écran en veille) ; l'interface se figerait (« ne répond pas »).
        // Les animations limitent elles-mêmes leur cadence (theme::spinner).
        glow_options: eframe::egui_glow::GlowConfiguration {
            vsync: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let e = exit.clone();
    let result = eframe::run_native(
        "zrepo-rs",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, mode, e)))),
    );
    match result {
        Ok(()) => exit.get(),
        Err(err) => {
            crate::attach_console();
            eprintln!("Impossible d'ouvrir la fenêtre de zrepo-rs : {err}");
            EX_FAIL
        }
    }
}

struct App {
    launcher: Option<Launcher>,
    setup: Option<Setup>,
    frameless: bool,
    exit: Rc<Cell<i32>>,
    keys: Vec<(Key, Modifiers)>,
    started: Instant,
    ever_focused: bool,
    keep_open: bool,
    theme_done: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, mode: Mode, exit: Rc<Cell<i32>>) -> Self {
        let ctx = &cc.egui_ctx;
        theme::install(ctx);
        let cfg = Config::load().map_err(|f| f.msg);
        let mut app = Self {
            launcher: None,
            setup: None,
            frameless: false,
            exit,
            keys: Vec::new(),
            started: Instant::now(),
            ever_focused: false,
            keep_open: std::env::var_os("ZREPO_RS_DEBUG_KEEP_OPEN").is_some(),
            theme_done: false,
        };
        match mode {
            Mode::Launcher { refresh, target } => {
                app.frameless = true;
                app.launcher = Some(Launcher::new(ctx, cfg, target, refresh));
            }
            Mode::Setup => {
                // Fichier illisible : l'assistant repart des valeurs par défaut.
                app.setup = Some(Setup::new(ctx, cfg.unwrap_or_default(), false));
            }
        }
        app
    }

    fn close(&self, ctx: &egui::Context, code: i32) {
        self.exit.set(code);
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    /// Sous Linux, winit ne connaît pas le thème du bureau (sous Wayland, il
    /// répond « clair » par défaut) : on lit la préférence GNOME/portail, que
    /// suivent aussi Omarchy et KDE. Ailleurs, egui suit le système.
    fn pick_theme(&mut self, ctx: &egui::Context) {
        if self.theme_done {
            return;
        }
        self.theme_done = true;
        let pref = match std::env::var("ZREPO_RS_DEBUG_THEME").as_deref() {
            Ok("light") => Some(ThemePreference::Light),
            Ok("dark") => Some(ThemePreference::Dark),
            _ if cfg!(target_os = "linux") => linux_color_scheme(),
            _ => None,
        };
        if let Some(p) = pref {
            ctx.set_theme(p);
        }
    }

    /// Relance zrepo-rs (le lanceur) dans un nouveau processus.
    fn spawn_launcher() {
        if let Ok(exe) = std::env::current_exe() {
            let _ = Command::new(exe)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }

    fn setup_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(setup) = &mut self.setup else { return };
        let escape = setup.embedded() && ctx.input(|i| i.key_pressed(Key::Escape));
        let outcome = setup.ui(ui);
        let embedded = setup.embedded();
        // Fermée par la croix du gestionnaire de fenêtres : le code dit si la
        // configuration a été enregistrée (0) ou non (130).
        if !embedded {
            self.exit.set(setup_exit_code(setup.saved()));
        }
        match outcome {
            setup::Outcome::Finished { open } if embedded => {
                let cfg = setup.cfg.clone();
                self.setup = None;
                if let Some(l) = &mut self.launcher {
                    l.reconfigure(&ctx, cfg);
                }
                if !open {
                    self.close(&ctx, EX_OK);
                }
            }
            setup::Outcome::Finished { open } => {
                // Sans config.toml (enregistrement raté), le lanceur rouvrirait l'assistant.
                if open && Config::exists() {
                    Self::spawn_launcher();
                }
                self.close(&ctx, EX_OK);
            }
            setup::Outcome::None if escape => self.setup = None,
            setup::Outcome::None => {}
        }
    }

    fn launcher_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(l) = &mut self.launcher else { return };
        let mut settings = false;
        for (k, m) in std::mem::take(&mut self.keys) {
            match l.on_key(&ctx, k, m) {
                launcher::Action::Close(code) => {
                    self.exit.set(code);
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
                launcher::Action::Settings => settings = true,
                launcher::Action::None => {}
            }
        }
        match l.ui(ui) {
            launcher::Action::Close(code) => {
                self.exit.set(code);
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            launcher::Action::Settings => settings = true,
            launcher::Action::None => {}
        }
        if settings {
            self.setup = Some(Setup::new(&ctx, l.cfg.clone(), true));
            return;
        }

        // Fermeture à la perte du focus, sauf pendant une ouverture ou une erreur.
        let focused = ctx.input(|i| i.viewport().focused);
        if focused == Some(true) {
            self.ever_focused = true;
        }
        let grace = self.started.elapsed() < FOCUS_GRACE;
        if focused == Some(false) && self.ever_focused && !grace && !l.busy() && !self.keep_open {
            self.close(&ctx, EX_CANCEL);
        }
    }
}

/// Code de sortie de `zrepo-rs setup` si la fenêtre est fermée maintenant.
fn setup_exit_code(saved: bool) -> i32 {
    if saved { EX_OK } else { EX_CANCEL }
}

/// `prefer-dark` / `prefer-light` / `default` (= clair) ; None si inconnu.
fn linux_color_scheme() -> Option<ThemePreference> {
    let out = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let s = String::from_utf8_lossy(&out.stdout);
    if s.contains("dark") {
        Some(ThemePreference::Dark)
    } else if s.contains("light") || s.contains("default") {
        Some(ThemePreference::Light)
    } else {
        None
    }
}

/// Raccourcis du lanceur, retirés des événements avant qu'egui ne les voie
/// (Tab déplacerait le focus, ↑ ↓ le curseur du champ de recherche...).
fn is_shortcut(key: Key, m: Modifiers) -> bool {
    let ctrl = m.ctrl || m.command;
    matches!(
        key,
        Key::Escape
            | Key::Enter
            | Key::Tab
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::PageUp
            | Key::PageDown
    ) || ctrl
        && matches!(
            key,
            Key::E | Key::Y | Key::R | Key::Comma | Key::Num1 | Key::Num2 | Key::Num3 | Key::Num4
        )
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.setup.is_some() || self.launcher.is_none() {
            return;
        }
        let keys = &mut self.keys;
        raw.events.retain(|e| match e {
            Event::Key {
                key,
                pressed,
                modifiers,
                ..
            } if is_shortcut(*key, *modifiers) => {
                if *pressed {
                    keys.push((*key, *modifiers));
                }
                false
            }
            _ => true,
        });
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        if self.frameless {
            [0.0; 4]
        } else {
            Pal::for_dark(visuals.dark_mode)
                .bg
                .to_normalized_gamma_f32()
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.pick_theme(&ctx);
        let pal = Pal::of(ui);
        let full = ui.max_rect();
        let area = if self.frameless {
            let card = full.shrink(MARGIN);
            let shadow = egui::Shadow {
                offset: [0, 6],
                blur: 18,
                spread: 0,
                color: Color32::from_black_alpha(if pal.dark { 150 } else { 60 }),
            };
            ui.painter()
                .add(shadow.as_shape(card, CornerRadius::same(RADIUS)));
            ui.painter().rect(
                card,
                CornerRadius::same(RADIUS),
                pal.bg,
                Stroke::new(1.0, pal.border),
                StrokeKind::Inside,
            );
            card
        } else {
            ui.painter().rect_filled(full, 0, pal.bg);
            full
        };
        let mut child = ui.new_child(UiBuilder::new().max_rect(area));
        child.set_clip_rect(area.expand(1.0));
        if self.setup.is_some() {
            self.setup_ui(&mut child);
        } else {
            self.launcher_ui(&mut child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fermeture_de_l_assistant() {
        assert_eq!(setup_exit_code(true), EX_OK);
        assert_eq!(setup_exit_code(false), EX_CANCEL);
    }

    #[test]
    fn raccourcis_interceptes() {
        let ctrl = Modifiers::CTRL;
        assert!(is_shortcut(Key::Tab, Modifiers::NONE));
        assert!(is_shortcut(Key::Comma, ctrl));
        assert!(!is_shortcut(Key::E, Modifiers::NONE));
        assert!(!is_shortcut(Key::A, ctrl));
    }
}
