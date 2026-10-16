//! Assistant de configuration : dossier des projets, GitHub, cibles,
//! raccourci clavier, enregistrement.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Layout, Rect, RichText, ScrollArea, Sense, Stroke,
    StrokeKind, TextEdit, UiBuilder, pos2, vec2,
};
use zrepo_core::{
    apps::{self, EditorConfig},
    auth::{self, DeviceCode, Poll, TokenSource},
    config::{Config, RemoteServer},
    open::{self, Target},
    sources,
};

use super::theme::{self, Pal};
use crate::{detect, hypr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    Welcome,
    Folder,
    Github,
    Servers,
    Targets,
    Shortcut,
    Done,
}

const STEPS: [(Step, &str); 7] = [
    (Step::Welcome, "Bienvenue"),
    (Step::Folder, "Dossier"),
    (Step::Github, "GitHub"),
    (Step::Servers, "Serveurs"),
    (Step::Targets, "Cibles"),
    (Step::Shortcut, "Raccourci"),
    (Step::Done, "Terminé"),
];

impl Step {
    fn index(self) -> usize {
        STEPS.iter().position(|(s, _)| *s == self).unwrap_or(0)
    }
    fn from_index(i: usize) -> Step {
        STEPS[i.min(STEPS.len() - 1)].0
    }
}

pub enum Outcome {
    None,
    /// Configuration enregistrée ; `open` = lancer le lanceur.
    Finished {
        open: bool,
    },
}

enum Gh {
    Checking,
    Missing,
    Connected(String, TokenSource),
    Refused(String, TokenSource),
    Starting,
    Waiting(DeviceCode),
    Verifying,
}

enum GhMsg {
    Status(Option<(Result<String, String>, TokenSource)>, bool),
    Device(DeviceCode),
    Connected(String, TokenSource),
    Error(String),
}

pub struct Setup {
    pub cfg: Config,
    step: Step,
    embedded: bool,
    folder: String,
    folder_error: Option<String>,
    confirm_create: bool,
    pick_rx: Option<Receiver<Option<PathBuf>>>,
    gh: Gh,
    gh_error: Option<String>,
    gh_cli: Option<bool>,
    /// Génération de l'action GitHub en cours : les réponses d'une action
    /// annulée ou remplacée (device flow relancé...) sont ignorées.
    gh_gen: u64,
    gh_tx: Sender<(u64, GhMsg)>,
    gh_rx: Receiver<(u64, GhMsg)>,
    cancel: Option<Arc<AtomicBool>>,
    pat: String,
    server_host: String,
    server_label: String,
    server_paths: String,
    editing_server: Option<usize>,
    server_error: Option<String>,
    ssh_tx: Sender<(u64, String, sources::SshStatus)>,
    ssh_rx: Receiver<(u64, String, sources::SshStatus)>,
    ssh_gen: u64,
    discovered: Vec<(String, Option<sources::SshStatus>)>,
    zed: Option<String>,
    herdr: Option<String>,
    /// Détection coûteuse (PATH, dossiers d'installation) : recalculée
    /// seulement quand les réglages des cibles changent.
    terminal: Option<String>,
    /// Éditeurs connus détectés : (nom, chemin).
    detected: Vec<(String, String)>,
    targets: Vec<Target>,
    editor_name: String,
    editor_rx: Option<Receiver<Option<PathBuf>>>,
    exe: PathBuf,
    shortcut: Option<Result<hypr::Proposal, String>>,
    applied: Option<Result<Vec<PathBuf>, String>>,
    saved: Option<Result<PathBuf, String>>,
    logo: Option<egui::TextureHandle>,
}

impl Setup {
    pub fn new(ctx: &egui::Context, cfg: Config, embedded: bool) -> Self {
        let (gh_tx, gh_rx) = mpsc::channel();
        let (ssh_tx, ssh_rx) = mpsc::channel();
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.canonicalize().ok())
            .unwrap_or_else(|| PathBuf::from("zrepo-rs"));
        let mut s = Self {
            folder: cfg.projects_dir.display().to_string(),
            cfg,
            step: Step::Welcome,
            embedded,
            folder_error: None,
            confirm_create: false,
            pick_rx: None,
            gh: Gh::Checking,
            gh_error: None,
            gh_cli: None,
            gh_gen: 0,
            gh_tx,
            gh_rx,
            cancel: None,
            pat: String::new(),
            server_host: String::new(),
            server_label: String::new(),
            server_paths: String::new(),
            editing_server: None,
            server_error: None,
            ssh_tx,
            ssh_rx,
            ssh_gen: 0,
            discovered: Vec::new(),
            zed: detect::zed(),
            herdr: detect::herdr(),
            terminal: None,
            detected: Vec::new(),
            targets: Vec::new(),
            editor_name: String::new(),
            editor_rx: None,
            exe,
            shortcut: None,
            applied: None,
            saved: None,
            logo: None,
        };
        s.refresh_targets();
        s.check_github(ctx);
        // Débogage (captures d'écran) : ouvrir directement l'étape n (1 à 7).
        if let Some(n) = std::env::var("ZREPO_RS_DEBUG_SETUP_STEP")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
        {
            s.enter(ctx, Step::from_index(n.saturating_sub(1)));
        }
        s
    }

    /// La configuration a-t-elle été enregistrée (étape « Terminé ») ?
    pub fn saved(&self) -> bool {
        matches!(self.saved, Some(Ok(_)))
    }

    /// Ouvert depuis le lanceur (Ctrl-,) : Échap y revient sans enregistrer.
    pub fn embedded(&self) -> bool {
        self.embedded
    }

    pub fn show_servers(&mut self, ctx: &egui::Context) {
        self.enter(ctx, Step::Servers);
    }

    pub fn show_targets(&mut self, ctx: &egui::Context) {
        self.enter(ctx, Step::Targets);
    }

    fn enter(&mut self, ctx: &egui::Context, step: Step) {
        if step == Step::Shortcut && self.shortcut.is_none() && hypr::running() {
            self.shortcut = Some(hypr::propose(&self.exe));
        }
        if step == Step::Targets {
            self.refresh_targets();
        }
        if step == Step::Servers {
            self.discover_servers(ctx);
        }
        if step == Step::Done {
            self.save();
        }
        if step != Step::Github {
            self.stop_device();
        }
        if step == Step::Github
            && let Ok(code) = std::env::var("ZREPO_RS_DEBUG_DEVICE_CODE")
        {
            // Débogage : afficher l'écran du code sans contacter GitHub.
            self.gh = Gh::Waiting(DeviceCode {
                user_code: code,
                verification_uri: "https://github.com/login/device".into(),
                device_code: String::new(),
                interval: 5,
                expires_in: 900,
            });
        }
        self.step = step;
        ctx.request_repaint();
    }

    fn save(&mut self) {
        self.saved = Some(self.cfg.save().map(|()| Config::path()).map_err(|f| f.msg));
    }

    fn discover_servers(&mut self, ctx: &egui::Context) {
        self.ssh_gen += 1;
        let generation = self.ssh_gen;
        self.discovered = sources::ssh_hosts(&self.cfg)
            .into_iter()
            .map(|host| (host, None))
            .collect();
        for (host, _) in &self.discovered {
            let (host, tx, ctx) = (host.clone(), self.ssh_tx.clone(), ctx.clone());
            thread::spawn(move || {
                let status = sources::ssh_status(&host);
                let _ = tx.send((generation, host, status));
                ctx.request_repaint();
            });
        }
    }

    fn clear_server_form(&mut self) {
        self.server_host.clear();
        self.server_label.clear();
        self.server_paths.clear();
        self.editing_server = None;
        self.server_error = None;
    }

    fn save_server_form(&mut self) -> bool {
        let server =
            match server_from_form(&self.server_host, &self.server_label, &self.server_paths) {
                Ok(server) => server,
                Err(error) => {
                    self.server_error = Some(error);
                    return false;
                }
            };
        if self.cfg.remote.servers.iter().enumerate().any(|(i, s)| {
            Some(i) != self.editing_server && s.host.eq_ignore_ascii_case(&server.host)
        }) {
            self.server_error = Some("Ce serveur est déjà ajouté.".into());
            return false;
        }
        if let Some(i) = self.editing_server {
            self.cfg.remote.servers[i] = server;
        } else {
            self.cfg.remote.servers.push(server);
        }
        self.clear_server_form();
        true
    }

    /// Cibles disponibles d'après les réglages courants ; la cible par défaut
    /// est ramenée sur l'une d'elles.
    fn refresh_targets(&mut self) {
        self.terminal = apps::terminal(&self.cfg).map(|t| t.program.display().to_string());
        let mine = &self.cfg.editors;
        self.detected = apps::editors(&self.cfg)
            .into_iter()
            .filter(|a| !mine.iter().any(|e| e.name == a.name))
            .map(|a| (a.name, a.program.display().to_string()))
            .collect();
        self.targets = open::available_targets(&self.cfg);
        self.cfg.ui.default_target = open::pick_target(&self.cfg.ui.default_target, &self.targets);
    }

    /// Sélecteur de l'exécutable d'un éditeur, comme `pick_folder`.
    fn pick_editor(&mut self) {
        let dialog = || {
            let mut d = rfd::FileDialog::new().set_title("Exécutable de l'éditeur");
            if cfg!(windows) {
                d = d.add_filter("Programmes", &["exe", "cmd", "bat"]);
            }
            d
        };
        let (tx, rx) = mpsc::channel();
        if cfg!(target_os = "macos") {
            let _ = tx.send(dialog().pick_file());
        } else {
            let d = dialog();
            thread::spawn(move || {
                let _ = tx.send(d.pick_file());
            });
        }
        self.editor_rx = Some(rx);
    }

    fn add_editor(&mut self, program: PathBuf) {
        let typed = self.editor_name.trim();
        let name = if typed.is_empty() {
            program
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Éditeur".into())
        } else {
            typed.to_string()
        };
        self.cfg.editors.retain(|e| e.name != name);
        self.cfg.editors.push(EditorConfig {
            name: name.clone(),
            command: program.display().to_string(),
            args: Vec::new(),
        });
        self.editor_name.clear();
        self.refresh_targets();
        self.cfg.ui.default_target = Target::App(name);
    }

    // ---------- GitHub (threads) ----------

    /// Nouvelle action GitHub : les messages des précédentes deviennent périmés.
    fn next_gen(&mut self) -> u64 {
        self.gh_gen += 1;
        self.gh_gen
    }

    fn spawn_gh(&mut self, ctx: &egui::Context, job: impl FnOnce() -> GhMsg + Send + 'static) {
        let generation = self.next_gen();
        let (tx, ctx) = (self.gh_tx.clone(), ctx.clone());
        thread::spawn(move || {
            let _ = tx.send((generation, job()));
            ctx.request_repaint();
        });
    }

    fn check_github(&mut self, ctx: &egui::Context) {
        self.gh = Gh::Checking;
        self.spawn_gh(ctx, || {
            let current = auth::token().map(|(t, src)| (auth::whoami(&t).map_err(|f| f.msg), src));
            GhMsg::Status(current, auth::gh_cli_token().is_some())
        });
    }

    fn start_device(&mut self, ctx: &egui::Context, client_id: &'static str) {
        self.stop_device();
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        self.gh = Gh::Starting;
        self.gh_error = None;
        let generation = self.next_gen();
        let (tx, ctx) = (self.gh_tx.clone(), ctx.clone());
        thread::spawn(move || {
            let send = |m: GhMsg| {
                let _ = tx.send((generation, m));
                ctx.request_repaint();
            };
            let dc = match auth::device_start(client_id) {
                Ok(dc) => dc,
                Err(f) => return send(GhMsg::Error(f.msg)),
            };
            send(GhMsg::Device(dc.clone()));
            let start = Instant::now();
            let mut interval = Duration::from_secs(dc.interval.max(1));
            loop {
                // Attente découpée pour réagir vite à l'annulation.
                let until = Instant::now() + interval;
                while Instant::now() < until {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                if start.elapsed() > Duration::from_secs(dc.expires_in) {
                    return send(GhMsg::Error(
                        "Le code a expiré. Recommencez la connexion.".into(),
                    ));
                }
                match auth::device_poll(client_id, &dc) {
                    Ok(Poll::Pending) => {}
                    Ok(Poll::SlowDown) => interval += Duration::from_secs(5),
                    Ok(Poll::Done(token)) => {
                        if cancel.load(Ordering::Relaxed) {
                            return;
                        }
                        let r = auth::store_token(&token).and_then(|()| auth::whoami(&token));
                        return send(match r {
                            Ok(login) => GhMsg::Connected(login, TokenSource::Keyring),
                            Err(f) => GhMsg::Error(f.msg),
                        });
                    }
                    Err(f) => return send(GhMsg::Error(f.msg)),
                }
            }
        });
    }

    fn stop_device(&mut self) {
        if let Some(c) = self.cancel.take() {
            c.store(true, Ordering::Relaxed);
            self.next_gen();
        }
        if matches!(self.gh, Gh::Starting | Gh::Waiting(_)) {
            self.gh = Gh::Missing;
        }
    }

    fn use_pat(&mut self, ctx: &egui::Context) {
        let pat = self.pat.trim().to_string();
        if pat.is_empty() {
            self.gh_error = Some("Collez d'abord un jeton.".into());
            return;
        }
        self.stop_device();
        self.gh = Gh::Verifying;
        self.gh_error = None;
        self.spawn_gh(ctx, move || {
            match auth::whoami(&pat).and_then(|login| auth::store_token(&pat).map(|()| login)) {
                Ok(login) => GhMsg::Connected(login, TokenSource::Keyring),
                Err(f) => GhMsg::Error(f.msg),
            }
        });
    }

    /// Le jeton de gh est relu à chaque lancement (rien n'est copié) ; un
    /// jeton du trousseau passerait devant, il est donc retiré, mais seulement
    /// une fois le jeton de gh vérifié.
    fn use_gh(&mut self, ctx: &egui::Context) {
        self.stop_device();
        self.gh = Gh::Verifying;
        self.gh_error = None;
        self.spawn_gh(ctx, || {
            let Some(t) = auth::gh_cli_token() else {
                return GhMsg::Error("gh n'est pas connecté (gh auth login).".into());
            };
            match auth::whoami(&t).and_then(|login| auth::delete_token().map(|()| login)) {
                Ok(login) => GhMsg::Connected(login, TokenSource::GhCli),
                Err(f) => GhMsg::Error(f.msg),
            }
        });
    }

    fn logout(&mut self, ctx: &egui::Context) {
        if let Err(f) = auth::delete_token() {
            self.gh_error = Some(f.msg);
            return;
        }
        self.gh_error = None;
        self.check_github(ctx);
    }

    fn receive(&mut self) {
        while let Ok((generation, host, status)) = self.ssh_rx.try_recv() {
            if generation == self.ssh_gen
                && let Some((_, state)) = self.discovered.iter_mut().find(|(h, _)| *h == host)
            {
                *state = Some(status);
            }
        }
        while let Ok((generation, m)) = self.gh_rx.try_recv() {
            if let GhMsg::Status(_, gh_cli) = &m {
                self.gh_cli = Some(*gh_cli);
            }
            if generation != self.gh_gen {
                continue;
            }
            match m {
                GhMsg::Status(current, _) => {
                    if matches!(self.gh, Gh::Checking) {
                        self.gh = match current {
                            None => Gh::Missing,
                            Some((Ok(login), src)) => Gh::Connected(login, src),
                            Some((Err(e), src)) => Gh::Refused(e, src),
                        };
                    }
                }
                GhMsg::Device(dc) => {
                    if matches!(self.gh, Gh::Starting) {
                        self.gh = Gh::Waiting(dc);
                    }
                }
                GhMsg::Connected(login, src) => {
                    self.cancel = None;
                    self.pat.clear();
                    self.gh = Gh::Connected(login, src);
                    self.cfg.github.enabled = true;
                }
                GhMsg::Error(e) => {
                    self.cancel = None;
                    self.gh_error = Some(e);
                    if matches!(self.gh, Gh::Starting | Gh::Waiting(_) | Gh::Verifying) {
                        self.gh = Gh::Missing;
                    }
                }
            }
        }
        if let Some(rx) = &self.pick_rx
            && let Ok(r) = rx.try_recv()
        {
            self.pick_rx = None;
            if let Some(p) = r {
                self.folder = p.display().to_string();
                self.folder_error = None;
                self.confirm_create = false;
            }
        }
        if let Some(rx) = &self.editor_rx
            && let Ok(r) = rx.try_recv()
        {
            self.editor_rx = None;
            if let Some(p) = r {
                self.add_editor(p);
            }
        }
    }

    // ---------- Interface ----------

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Outcome {
        self.receive();
        let ctx = ui.ctx().clone();
        if matches!(
            self.gh,
            Gh::Checking | Gh::Starting | Gh::Waiting(_) | Gh::Verifying
        ) || self.pick_rx.is_some()
            || self.editor_rx.is_some()
        {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        let pal = Pal::of(ui);
        let area = ui.max_rect();
        let header = Rect::from_min_size(area.min, vec2(area.width(), 78.0));
        let footer = Rect::from_min_max(pos2(area.left(), area.bottom() - 64.0), area.max);
        let content = Rect::from_min_max(
            pos2(area.left() + 32.0, header.bottom() + 18.0),
            pos2(area.right() - 32.0, footer.top() - 8.0),
        );
        self.stepper(ui, header, &pal);
        ui.painter().hline(
            area.x_range(),
            header.bottom(),
            Stroke::new(1.0, pal.border),
        );
        ui.painter()
            .hline(area.x_range(), footer.top(), Stroke::new(1.0, pal.border));

        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(content)
                .layout(Layout::top_down(Align::Min)),
        );
        let mut outcome = Outcome::None;
        ScrollArea::vertical()
            .id_salt(("assistant", self.step.index()))
            .auto_shrink(false)
            .show(&mut child, |ui| {
                ui.set_width(content.width() - 12.0);
                match self.step {
                    Step::Welcome => self.welcome_ui(ui, &pal),
                    Step::Folder => self.folder_ui(ui, &pal),
                    Step::Github => self.github_ui(ui, &pal),
                    Step::Servers => self.servers_ui(ui, &pal),
                    Step::Targets => self.targets_ui(ui, &pal),
                    Step::Shortcut => self.shortcut_ui(ui, &pal),
                    Step::Done => self.done_ui(ui, &pal),
                }
            });
        let nav = self.footer_ui(ui, footer, &pal);
        if let Some(o) = nav {
            outcome = o;
        }
        outcome
    }

    fn stepper(&self, ui: &mut egui::Ui, rect: Rect, pal: &Pal) {
        let p = ui.painter();
        let n = STEPS.len();
        let inner = rect.shrink2(vec2(36.0, 0.0));
        let w = inner.width() / n as f32;
        let cy = rect.top() + 30.0;
        let cur = self.step.index();
        for (i, (_, label)) in STEPS.iter().enumerate() {
            let cx = inner.left() + w * (i as f32 + 0.5);
            if i + 1 < n {
                let color = if i < cur { pal.accent } else { pal.border };
                p.line_segment(
                    [pos2(cx + 16.0, cy), pos2(cx + w - 16.0, cy)],
                    Stroke::new(2.0, color),
                );
            }
            let (fill, fg, stroke) = if i < cur {
                (pal.accent, Color32::WHITE, Stroke::NONE)
            } else if i == cur {
                (pal.accent_soft, pal.hit, Stroke::new(2.0, pal.accent))
            } else {
                (pal.surface, pal.faint, Stroke::new(1.0, pal.border))
            };
            p.circle(pos2(cx, cy), 12.0, fill, stroke);
            let mark = if i < cur {
                "✔".to_string()
            } else {
                (i + 1).to_string()
            };
            p.text(
                pos2(cx, cy),
                egui::Align2::CENTER_CENTER,
                mark,
                FontId::proportional(12.5),
                fg,
            );
            let lc = if i == cur { pal.text } else { pal.faint };
            p.text(
                pos2(cx, cy + 25.0),
                egui::Align2::CENTER_CENTER,
                *label,
                FontId::proportional(12.0),
                lc,
            );
        }
    }

    fn footer_ui(&mut self, ui: &mut egui::Ui, rect: Rect, pal: &Pal) -> Option<Outcome> {
        let ctx = ui.ctx().clone();
        let mut out = None;
        let mut c = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(28.0, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        if self.step > Step::Welcome
            && self.step < Step::Done
            && theme::button(&mut c, "Précédent", false).clicked()
        {
            let prev = Step::from_index(self.step.index() - 1);
            self.enter(&ctx, prev);
        }
        if self.embedded && self.step < Step::Done {
            c.label(
                RichText::new("Échap : revenir au lanceur sans enregistrer")
                    .size(12.0)
                    .color(pal.faint),
            );
        }
        c.with_layout(Layout::right_to_left(Align::Center), |ui| match self.step {
            Step::Done => {
                if self.embedded {
                    if theme::button(ui, "Revenir au lanceur", true).clicked() {
                        out = Some(Outcome::Finished { open: true });
                    }
                } else {
                    if theme::button(ui, "Ouvrir zrepo-rs", true).clicked() {
                        out = Some(Outcome::Finished { open: true });
                    }
                    if theme::button(ui, "Fermer", false).clicked() {
                        out = Some(Outcome::Finished { open: false });
                    }
                }
            }
            step => {
                let label = match step {
                    Step::Welcome => "Commencer",
                    Step::Github if !matches!(self.gh, Gh::Connected(..)) => "Plus tard",
                    Step::Shortcut => "Enregistrer",
                    _ => "Suivant",
                };
                let primary = !(step == Step::Github && label == "Plus tard");
                if theme::button(ui, label, primary).clicked() && self.validate(step) {
                    let next = Step::from_index(step.index() + 1);
                    self.enter(&ctx, next);
                }
            }
        });
        out
    }

    /// Vérifie l'étape avant de passer à la suivante.
    fn validate(&mut self, step: Step) -> bool {
        if step == Step::Servers {
            let pending = self.editing_server.is_some()
                || !self.server_host.trim().is_empty()
                || !self.server_label.trim().is_empty()
                || !self.server_paths.trim().is_empty();
            return !pending || self.save_server_form();
        }
        if step != Step::Folder {
            return true;
        }
        let p = expand(self.folder.trim());
        if self.folder.trim().is_empty() || !p.is_absolute() {
            self.folder_error = Some("Indiquez un chemin complet (par exemple ~/Projects).".into());
            return false;
        }
        if p.exists() && !p.is_dir() {
            self.folder_error = Some("Ce chemin existe mais ce n'est pas un dossier.".into());
            return false;
        }
        if !p.exists() {
            self.confirm_create = true;
            return false;
        }
        self.cfg.projects_dir = p;
        self.folder_error = None;
        true
    }

    fn title(ui: &mut egui::Ui, pal: &Pal, title: &str, sub: &str) {
        ui.label(RichText::new(title).size(22.0).color(pal.text));
        ui.add_space(2.0);
        ui.label(RichText::new(sub).size(13.5).color(pal.weak));
        ui.add_space(14.0);
    }

    fn welcome_ui(&mut self, ui: &mut egui::Ui, pal: &Pal) {
        let logo = self.logo.get_or_insert_with(|| {
            let icon = super::icon();
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [icon.width as usize, icon.height as usize],
                &icon.rgba,
            );
            ui.ctx()
                .load_texture("zrepo-rs-logo", img, egui::TextureOptions::LINEAR)
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add(egui::Image::new((logo.id(), vec2(72.0, 72.0))));
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Bienvenue dans zrepo-rs")
                        .size(24.0)
                        .color(pal.text),
                );
                ui.label(
                    RichText::new("Tous vos projets à une touche de distance.")
                        .size(14.0)
                        .color(pal.weak),
                );
            });
        });
        ui.add_space(18.0);
        ui.label(
            RichText::new(
                "zrepo-rs réunit dans une seule fenêtre vos projets locaux, ceux de vos serveurs \
                 (déclarés dans Zed) et vos dépôts GitHub. Tapez quelques lettres, Entrée : \
                 le projet s'ouvre dans Zed ou dans Herdr, après un clonage si besoin.",
            )
            .size(14.0)
            .color(pal.text),
        );
        ui.add_space(14.0);
        ui.label(
            RichText::new("En quelques étapes :")
                .size(13.5)
                .color(pal.weak),
        );
        ui.add_space(4.0);
        ui.spacing_mut().interact_size.y = 24.0;
        ui.spacing_mut().item_spacing.y = 6.0;
        for (n, t) in [
            ("1", "le dossier où vivent vos projets ;"),
            ("2", "la connexion à GitHub (facultative) ;"),
            ("3", "l'éditeur ou l'agent à ouvrir par défaut ;"),
            ("4", "un raccourci clavier pour appeler zrepo-rs."),
        ] {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                ui.painter()
                    .circle_filled(r.center(), 10.0, pal.accent_soft);
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    n,
                    FontId::proportional(12.0),
                    pal.hit,
                );
                ui.label(RichText::new(t).size(14.0).color(pal.text));
            });
        }
        ui.add_space(12.0);
        ui.label(
            RichText::new(format!(
                "Tout se règle plus tard avec Ctrl-, dans la fenêtre, ou dans {}.",
                tilde_path(&Config::path())
            ))
            .size(12.0)
            .color(pal.faint),
        );
    }

    fn folder_ui(&mut self, ui: &mut egui::Ui, pal: &Pal) {
        Self::title(
            ui,
            pal,
            "Dossier des projets",
            "Les dépôts git qu'il contient apparaissent dans la liste ; les dépôts GitHub y sont clonés.",
        );
        ui.horizontal(|ui| {
            let w = ui.available_width() - 130.0;
            let r = ui.add(
                TextEdit::singleline(&mut self.folder)
                    .font(FontId::proportional(14.5))
                    .margin(vec2(10.0, 8.0))
                    .desired_width(w),
            );
            if r.changed() {
                self.folder_error = None;
                self.confirm_create = false;
            }
            let busy = self.pick_rx.is_some();
            if ui
                .add_enabled_ui(!busy, |ui| theme::button(ui, "Parcourir…", false))
                .inner
                .clicked()
            {
                self.pick_folder();
            }
        });
        ui.add_space(8.0);
        let p = expand(self.folder.trim());
        if let Some(e) = &self.folder_error {
            status(ui, pal.error, e);
        } else if self.folder.trim().is_empty() {
            status(ui, pal.faint, "Indiquez un dossier.");
        } else if p.is_dir() {
            status(ui, pal.ok, "Dossier existant.");
        } else if p.is_absolute() && !p.exists() {
            status(ui, pal.warn, "Ce dossier n'existe pas encore.");
        }
        if self.confirm_create {
            ui.add_space(10.0);
            card(ui, pal, |ui| {
                ui.label(
                    RichText::new(format!("Créer le dossier {} ?", p.display()))
                        .size(14.0)
                        .color(pal.text),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if theme::button(ui, "Créer le dossier", true).clicked() {
                        match std::fs::create_dir_all(&p) {
                            Ok(()) => {
                                self.confirm_create = false;
                                self.cfg.projects_dir = p.clone();
                                let ctx = ui.ctx().clone();
                                self.enter(&ctx, Step::Github);
                            }
                            Err(e) => {
                                self.confirm_create = false;
                                self.folder_error = Some(format!("Création impossible : {e}"));
                            }
                        }
                    }
                    if theme::button(ui, "Annuler", false).clicked() {
                        self.confirm_create = false;
                    }
                });
            });
        }
        ui.add_space(16.0);
        ui.label(
            RichText::new(format!(
                "Les sous-dossiers sont parcourus sur {} niveaux. Un dépôt GitHub déjà présent \
                 n'est jamais recloné ; un dossier occupé n'est jamais écrasé.",
                self.cfg.scan_depth
            ))
            .size(12.5)
            .color(pal.faint),
        );
    }

    /// Sélecteur natif. Hors macOS, dans un thread : la fenêtre continue de
    /// répondre au compositeur pendant que le dialogue est ouvert.
    fn pick_folder(&mut self) {
        let start = expand(self.folder.trim());
        let dialog = || {
            let mut d = rfd::FileDialog::new().set_title("Dossier des projets");
            if start.is_dir() {
                d = d.set_directory(&start);
            } else if let Some(parent) = start.parent().filter(|p| p.is_dir()) {
                d = d.set_directory(parent);
            }
            d
        };
        let (tx, rx) = mpsc::channel();
        if cfg!(target_os = "macos") {
            let _ = tx.send(dialog().pick_folder());
        } else {
            let d = dialog();
            thread::spawn(move || {
                let _ = tx.send(d.pick_folder());
            });
        }
        self.pick_rx = Some(rx);
    }

    fn github_ui(&mut self, ui: &mut egui::Ui, pal: &Pal) {
        let ctx = ui.ctx().clone();
        Self::title(
            ui,
            pal,
            "Connexion à GitHub",
            "Pour lister vos dépôts (privés compris, et ceux de vos organisations) et les cloner. \
             Le jeton est rangé dans le trousseau du système, jamais dans un fichier.",
        );
        // État actuel.
        let mut logout = false;
        card(ui, pal, |ui| match &self.gh {
            Gh::Checking => {
                ui.horizontal(|ui| {
                    theme::spinner(ui, 14.0, pal.weak);
                    ui.label(RichText::new("Vérification de la connexion…").color(pal.weak));
                });
            }
            Gh::Connected(login, src) => {
                ui.horizontal(|ui| {
                    dot(ui, pal.ok);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!("Connecté en tant que @{login}"))
                                .size(15.0)
                                .color(pal.text),
                        );
                        ui.label(source_note(*src, pal));
                    });
                    if *src == TokenSource::Keyring {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            logout = theme::button(ui, "Se déconnecter", false).clicked();
                        });
                    }
                });
            }
            Gh::Refused(msg, src) => {
                ui.horizontal(|ui| {
                    dot(ui, pal.error);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(msg).color(pal.text));
                        ui.label(source_note(*src, pal));
                    });
                    if *src == TokenSource::Keyring {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            logout = theme::button(ui, "Se déconnecter", false).clicked();
                        });
                    }
                });
            }
            Gh::Verifying => {
                ui.horizontal(|ui| {
                    theme::spinner(ui, 14.0, pal.weak);
                    ui.label(RichText::new("Vérification du jeton…").color(pal.weak));
                });
            }
            Gh::Missing | Gh::Starting | Gh::Waiting(_) => {
                ui.horizontal(|ui| {
                    dot(ui, pal.faint);
                    ui.label(RichText::new("Pas encore connecté.").color(pal.weak));
                });
            }
        });
        if logout {
            self.logout(&ctx);
        }
        if let Some(e) = &self.gh_error {
            ui.add_space(6.0);
            status(ui, pal.error, e);
        }
        ui.add_space(14.0);

        // Connexion par le navigateur.
        if let Some(client_id) = auth::CLIENT_ID {
            let mut action = None;
            match &self.gh {
                Gh::Waiting(dc) => {
                    card(ui, pal, |ui| {
                        ui.label(
                            RichText::new("Saisissez ce code sur GitHub :")
                                .size(13.5)
                                .color(pal.weak),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(&dc.user_code)
                                .font(FontId::monospace(34.0))
                                .color(pal.text),
                        );
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if theme::button(ui, "Copier le code et ouvrir github.com", true)
                                .clicked()
                            {
                                action = Some(1);
                            }
                            if theme::button(ui, "Annuler", false).clicked() {
                                action = Some(2);
                            }
                        });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            theme::spinner(ui, 12.0, pal.weak);
                            ui.label(
                                RichText::new(format!(
                                    "En attente de l'autorisation sur {}…",
                                    dc.verification_uri
                                ))
                                .size(12.5)
                                .color(pal.weak),
                            );
                        });
                    });
                    if action == Some(1) {
                        ctx.copy_text(dc.user_code.clone());
                        ctx.open_url(egui::OpenUrl::new_tab(&dc.verification_uri));
                    }
                }
                Gh::Starting => {
                    ui.horizontal(|ui| {
                        theme::spinner(ui, 14.0, pal.weak);
                        ui.label(RichText::new("Demande d'un code à GitHub…").color(pal.weak));
                    });
                }
                _ => {
                    let label = if matches!(self.gh, Gh::Connected(..)) {
                        "Se connecter avec un autre compte GitHub"
                    } else {
                        "Se connecter avec GitHub"
                    };
                    if theme::button(ui, label, !matches!(self.gh, Gh::Connected(..))).clicked() {
                        action = Some(0);
                    }
                    ui.label(
                        RichText::new("Un code s'affiche ; vous l'autorisez dans le navigateur.")
                            .size(12.5)
                            .color(pal.faint),
                    );
                }
            }
            match action {
                Some(0) => self.start_device(&ctx, client_id),
                Some(2) => self.stop_device(),
                _ => {}
            }
            ui.add_space(14.0);
            ui.label(RichText::new("Ou bien :").size(13.0).color(pal.weak));
            ui.add_space(6.0);
        }

        // Jeton de gh.
        if self.gh_cli == Some(true) {
            let using_gh = matches!(self.gh, Gh::Connected(_, TokenSource::GhCli));
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!using_gh, |ui| {
                    if theme::button(ui, "Utiliser le jeton de gh", false).clicked() {
                        self.use_gh(&ctx);
                    }
                });
                let note = if using_gh {
                    "déjà utilisé : zrepo-rs le relit à chaque lancement"
                } else {
                    "gh est connecté sur cette machine ; rien n'est copié"
                };
                ui.label(RichText::new(note).size(12.5).color(pal.faint));
            });
            ui.add_space(10.0);
        }

        // Jeton personnel.
        ui.label(
            RichText::new("Coller un jeton personnel")
                .size(13.5)
                .color(pal.text),
        );
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let w = ui.available_width() - 110.0;
            let r = ui.add(
                TextEdit::singleline(&mut self.pat)
                    .password(true)
                    .hint_text("ghp_… ou github_pat_…")
                    .margin(vec2(10.0, 8.0))
                    .desired_width(w),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let busy = matches!(self.gh, Gh::Verifying);
            let clicked = ui
                .add_enabled_ui(!busy && !self.pat.trim().is_empty(), |ui| {
                    theme::button(ui, "Valider", false)
                })
                .inner
                .clicked();
            if (clicked || enter) && !busy {
                self.use_pat(&ctx);
            }
        });
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Portées nécessaires : repo, read:org.")
                    .size(12.5)
                    .color(pal.faint),
            );
            ui.hyperlink_to(
                RichText::new("Créer un jeton sur github.com")
                    .size(12.5)
                    .color(pal.hit),
                "https://github.com/settings/tokens/new?scopes=repo,read:org&description=zrepo-rs",
            );
        });
    }

    fn servers_ui(&mut self, ui: &mut egui::Ui, pal: &Pal) {
        Self::title(
            ui,
            pal,
            "Vos serveurs SSH",
            "Les hôtes connus sont recherchés avant toute saisie.",
        );
        ui.checkbox(
            &mut self.cfg.remote.enabled,
            "Afficher les projets des serveurs",
        );
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Serveurs détectés")
                    .size(14.0)
                    .color(pal.text),
            );
            if ui.small_button("Relancer la recherche").clicked() {
                self.discover_servers(ui.ctx());
            }
        });
        ui.label(
            RichText::new(
                "Alias de ~/.ssh/config et connexions Zed ; test SSH sans mot de passe ni invite.",
            )
            .size(12.0)
            .color(pal.faint),
        );
        ui.add_space(6.0);
        if !sources::ssh_installed() {
            status(ui, pal.error, "Commande ssh introuvable sur ce poste.");
        } else if self.discovered.is_empty() {
            ui.label("Aucun serveur déclaré dans SSH ou Zed. Ajoutez-en un ci-dessous.");
        }
        for (host, availability) in self.discovered.clone() {
            let added = self
                .cfg
                .remote
                .servers
                .iter()
                .any(|s| s.host.eq_ignore_ascii_case(&host));
            ui.horizontal(|ui| {
                dot(
                    ui,
                    match availability {
                        Some(sources::SshStatus::Ready) => pal.ok,
                        Some(_) => pal.error,
                        None => pal.faint,
                    },
                );
                ui.label(RichText::new(&host).size(13.5).color(pal.text));
                ui.label(
                    RichText::new(match availability {
                        Some(sources::SshStatus::Ready) => "connexion SSH prête",
                        Some(sources::SshStatus::HostKeyChanged) => {
                            "clé d'hôte modifiée — vérifier"
                        }
                        Some(sources::SshStatus::Unavailable) => "connexion à vérifier",
                        None => "vérification…",
                    })
                    .size(12.0)
                    .color(pal.weak),
                );
                if ui
                    .add_enabled(
                        !added,
                        egui::Button::new(if added { "Ajouté" } else { "Ajouter" }),
                    )
                    .clicked()
                {
                    self.cfg.remote.servers.push(RemoteServer {
                        host: host.clone(),
                        label: host.clone(),
                        paths: Vec::new(),
                    });
                    self.cfg.remote.enabled = true;
                }
            });
        }
        ui.add_space(14.0);
        ui.label(RichText::new("Serveurs ajoutés").size(14.0).color(pal.text));
        let mut edit = None;
        let mut remove = None;
        for (i, server) in self.cfg.remote.servers.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} ({})", server.label, server.host))
                        .size(13.0)
                        .color(pal.text),
                );
                if ui.small_button("Modifier").clicked() {
                    edit = Some(i);
                }
                if ui.small_button("Supprimer").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.cfg.remote.servers.remove(i);
            self.clear_server_form();
        } else if let Some(i) = edit {
            let server = &self.cfg.remote.servers[i];
            self.server_host = server.host.clone();
            self.server_label = server.label.clone();
            self.server_paths = server.paths.join("\n");
            self.editing_server = Some(i);
            self.server_error = None;
        }
        ui.add_space(14.0);
        ui.label(
            RichText::new(if self.editing_server.is_some() {
                "Modifier un serveur"
            } else {
                "Ajouter un autre serveur"
            })
            .size(14.0)
            .color(pal.text),
        );
        ui.label("Hôte SSH (alias de ~/.ssh/config ou utilisateur@hôte)");
        ui.add(TextEdit::singleline(&mut self.server_host).desired_width(f32::INFINITY));
        ui.label("Nom affiché (facultatif)");
        ui.add(TextEdit::singleline(&mut self.server_label).desired_width(f32::INFINITY));
        ui.label("Chemins de projets sur le serveur (facultatif, un chemin absolu par ligne)");
        ui.add(
            TextEdit::multiline(&mut self.server_paths)
                .desired_width(f32::INFINITY)
                .desired_rows(2),
        );
        ui.label(
            RichText::new(
                "Sans chemin indiqué, zrepo-rs cherchera les dépôts Git lors de l'actualisation.",
            )
            .size(12.0)
            .color(pal.faint),
        );
        if let Some(error) = &self.server_error {
            status(ui, pal.error, error);
        }
        ui.horizontal(|ui| {
            if theme::button(
                ui,
                if self.editing_server.is_some() {
                    "Enregistrer"
                } else {
                    "Ajouter"
                },
                true,
            )
            .clicked()
            {
                self.save_server_form();
            }
            if self.editing_server.is_some() && ui.button("Annuler").clicked() {
                self.clear_server_form();
            }
        });
        ui.add_space(12.0);
        ui.label(
            RichText::new(format!(
                "Connexions Zed également lues dans {}",
                tilde_path(&self.cfg.zed_settings_path())
            ))
            .size(12.0)
            .color(pal.faint),
        );
    }

    fn targets_ui(&mut self, ui: &mut egui::Ui, pal: &Pal) {
        Self::title(
            ui,
            pal,
            "Où ouvrir les projets",
            "Entrée ouvre dans la cible par défaut ; Tab en change dans la fenêtre.",
        );
        card(ui, pal, |ui| {
            tool_row(
                ui,
                pal,
                "Zed",
                self.zed.as_deref(),
                "introuvable (facultatif) : zed.dev",
            );
            ui.add_space(6.0);
            let herdr_missing = if cfg!(windows) {
                "non disponible sous Windows"
            } else {
                "introuvable (facultatif)"
            };
            tool_row(ui, pal, "Herdr", self.herdr.as_deref(), herdr_missing);
            ui.add_space(6.0);
            tool_row(
                ui,
                pal,
                "Terminal",
                self.terminal.as_deref(),
                "introuvable : à indiquer dans config.toml, section [terminal]",
            );
            for (name, path) in &self.detected {
                ui.add_space(6.0);
                tool_row(ui, pal, name, Some(path.as_str()), "");
            }
        });
        ui.add_space(14.0);
        ui.spacing_mut().interact_size.y = 24.0;
        ui.spacing_mut().item_spacing.y = 8.0;
        self.my_editors_ui(ui, pal);
        ui.add_space(12.0);
        ui.label(RichText::new("Cible par défaut").size(13.5).color(pal.text));
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            for t in self.targets.clone() {
                let label = t.label(&self.cfg);
                ui.radio_value(&mut self.cfg.ui.default_target, t, label);
                ui.add_space(10.0);
            }
        });
        ui.add_space(12.0);
        ui.label(
            RichText::new("Sources et cibles")
                .size(13.5)
                .color(pal.text),
        );
        ui.add_space(2.0);
        let agent = self.cfg.agent_label();
        let herdr_toggled = ui
            .add_enabled_ui(!cfg!(windows), |ui| {
                ui.checkbox(
                    &mut self.cfg.herdr.enabled,
                    format!("Proposer Herdr + {agent} quand il est installé"),
                )
            })
            .inner
            .changed();
        if herdr_toggled {
            self.refresh_targets();
        }
        ui.checkbox(&mut self.cfg.github.enabled, "Afficher mes dépôts GitHub");
        ui.checkbox(
            &mut self.cfg.remote.enabled,
            "Afficher les projets de mes serveurs (configurés ici et dans Zed)",
        );
        ui.label(
            RichText::new(format!(
                "Lu dans {}",
                tilde_path(&self.cfg.zed_settings_path())
            ))
            .size(12.0)
            .color(pal.faint),
        );
    }

    /// Éditeurs ajoutés à la main : n'importe quel exécutable, qui reçoit le
    /// dossier du projet en dernier argument.
    fn my_editors_ui(&mut self, ui: &mut egui::Ui, pal: &Pal) {
        ui.label(RichText::new("Mes éditeurs").size(13.5).color(pal.text));
        ui.add_space(2.0);
        let mut remove = None;
        for (i, e) in self.cfg.editors.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&e.name).size(14.0).color(pal.text));
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(tilde_path(Path::new(&e.command)))
                            .size(12.5)
                            .color(pal.weak),
                    )
                    .truncate(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::button(ui, "Retirer", false).clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove {
            self.cfg.editors.remove(i);
            self.refresh_targets();
        }
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.editor_name)
                    .hint_text("Nom (facultatif)")
                    .desired_width(180.0),
            );
            let busy = self.editor_rx.is_some();
            if ui
                .add_enabled_ui(!busy, |ui| theme::button(ui, "Ajouter un éditeur…", false))
                .inner
                .clicked()
            {
                self.pick_editor();
            }
        });
        ui.label(
            RichText::new(
                "Choisissez l'exécutable de votre éditeur ou IDE : il recevra le dossier du \
                 projet. Options avancées : section [[editors]] de config.toml.",
            )
            .size(12.0)
            .color(pal.faint),
        );
    }

    fn shortcut_ui(&mut self, ui: &mut egui::Ui, pal: &Pal) {
        Self::title(
            ui,
            pal,
            "Raccourci clavier",
            "Pour appeler zrepo-rs d'une touche, depuis n'importe où.",
        );
        let exe = self.exe.display().to_string();
        if hypr::running() {
            self.hyprland_ui(ui, pal, &exe);
            return;
        }
        let steps: &[&str] = if cfg!(target_os = "macos") {
            &[
                "Ouvrez l'app Raccourcis, puis créez un raccourci.",
                "Ajoutez l'action « Ouvrir l'app » et choisissez zrepo-rs (dans ~/Applications).",
                "Dans les détails du raccourci : « Ajouter un raccourci clavier ».",
            ]
        } else if cfg!(windows) {
            &[
                "Menu Démarrer : clic droit sur zrepo-rs, « Ouvrir l'emplacement du fichier ».",
                "Clic droit sur le raccourci zrepo-rs, puis « Propriétés ».",
                "Champ « Touche de raccourci » : par exemple Ctrl + Alt + R.",
            ]
        } else {
            &[
                "GNOME : Paramètres → Clavier → Raccourcis personnalisés → « + ».",
                "KDE Plasma : Configuration du système → Raccourcis → Ajouter une application.",
                "Autre bureau : associez une touche à la commande ci-dessous.",
            ]
        };
        card(ui, pal, |ui| {
            for (i, s) in steps.iter().enumerate() {
                ui.label(
                    RichText::new(format!("{}. {s}", i + 1))
                        .size(13.5)
                        .color(pal.text),
                );
            }
        });
        ui.add_space(12.0);
        ui.label(
            RichText::new("Commande à lancer :")
                .size(13.0)
                .color(pal.weak),
        );
        ui.horizontal(|ui| {
            code_line(ui, pal, &exe);
            if theme::button(ui, "Copier", false).clicked() {
                ui.ctx().copy_text(exe.clone());
            }
        });
    }

    fn hyprland_ui(&mut self, ui: &mut egui::Ui, pal: &Pal, exe: &str) {
        let mut apply = false;
        match &self.shortcut {
            None => {}
            Some(Err(e)) => {
                status(ui, pal.error, e);
                ui.add_space(8.0);
                ui.label(RichText::new("Ajoutez à la main, par exemple :").color(pal.weak));
                code_line(
                    ui,
                    pal,
                    &format!("bindd = SUPER, R, Projets (zrepo-rs), exec, {exe}"),
                );
            }
            Some(Ok(p)) => {
                ui.horizontal(|ui| {
                    dot(ui, pal.ok);
                    ui.label(
                        RichText::new(
                            "Hyprland détecté : SUPER + R, fenêtre flottante et centrée.",
                        )
                        .color(pal.text),
                    );
                });
                ui.add_space(8.0);
                match &self.applied {
                    Some(Ok(baks)) => {
                        status(
                            ui,
                            pal.ok,
                            "Enregistré. Hyprland recharge sa configuration tout seul.",
                        );
                        for b in baks {
                            ui.label(
                                RichText::new(format!("Sauvegarde : {}", tilde_path(b)))
                                    .size(12.0)
                                    .color(pal.faint),
                            );
                        }
                    }
                    Some(Err(e)) => status(ui, pal.error, e),
                    None if p.changes.is_empty() => status(ui, pal.ok, "Tout est déjà en place."),
                    None => {
                        ui.horizontal(|ui| {
                            apply = theme::button(ui, "Appliquer", true).clicked();
                            ui.label(
                                RichText::new(
                                    "Rien n'est écrit sans ce clic. Copie .bak horodatée avant.",
                                )
                                .size(12.5)
                                .color(pal.faint),
                            );
                        });
                    }
                }
                for n in &p.notes {
                    status(ui, pal.ok, n);
                }
                ui.add_space(10.0);
                for c in &p.changes {
                    ui.label(
                        RichText::new(tilde_path(&c.file))
                            .size(12.0)
                            .monospace()
                            .color(pal.weak),
                    );
                    diff(ui, pal, &c.preview);
                    ui.add_space(8.0);
                }
                ui.label(
                    RichText::new(
                        "La fenêtre du lanceur dessine elle-même ses coins et son ombre : la règle \
                         retire bordure, ombre et flou du compositeur pour elle.",
                    )
                    .size(12.0)
                    .color(pal.faint),
                );
            }
        }
        if apply && let Some(Ok(p)) = &self.shortcut {
            self.applied = Some(hypr::apply(p));
        }
    }

    fn done_ui(&mut self, ui: &mut egui::Ui, pal: &Pal) {
        match &self.saved {
            Some(Ok(path)) => {
                Self::title(
                    ui,
                    pal,
                    "C'est prêt",
                    &format!("Configuration enregistrée dans {}", tilde_path(path)),
                );
            }
            Some(Err(e)) => {
                Self::title(ui, pal, "Enregistrement impossible", "");
                status(ui, pal.error, e);
                ui.add_space(10.0);
            }
            None => Self::title(ui, pal, "Récapitulatif", ""),
        }
        let github = match &self.gh {
            _ if !self.cfg.github.enabled => "désactivé".to_string(),
            Gh::Connected(login, _) => format!("@{login}"),
            _ => "non connecté (dépôts publics seulement pour le clonage)".to_string(),
        };
        let mut sources = vec!["locaux"];
        if self.cfg.remote.enabled {
            sources.push("serveurs");
        }
        if self.cfg.github.enabled {
            sources.push("GitHub");
        }
        card(ui, pal, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            let row = |ui: &mut egui::Ui, k: &str, v: &str| {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        vec2(150.0, 18.0),
                        Layout::left_to_right(Align::Min),
                        |ui| {
                            ui.set_min_width(150.0);
                            ui.label(RichText::new(k).size(13.0).color(pal.weak));
                        },
                    );
                    ui.add(egui::Label::new(RichText::new(v).size(13.5).color(pal.text)).wrap());
                });
            };
            row(
                ui,
                "Dossier des projets",
                &tilde_path(&self.cfg.projects_dir),
            );
            row(ui, "GitHub", &github);
            row(
                ui,
                "Cible par défaut",
                &self.cfg.ui.default_target.label(&self.cfg),
            );
            row(ui, "Sources", &sources.join(", "));
        });
        ui.add_space(14.0);
        ui.label(
            RichText::new(
                "Dans la fenêtre : Entrée ouvre, Tab change de cible, Ctrl-R actualise, \
                 Ctrl-, rouvre ces réglages.",
            )
            .size(12.5)
            .color(pal.faint),
        );
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        self.stop_device();
    }
}

fn expand(p: &str) -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    match (p.strip_prefix('~'), home) {
        (Some(rest), Some(h)) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            PathBuf::from(h).join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(p),
    }
}

fn tilde_path(p: &Path) -> String {
    let s = p.display().to_string();
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && s.starts_with(&h) => format!("~{}", &s[h.len()..]),
        _ => s,
    }
}

fn server_from_form(host: &str, label: &str, paths: &str) -> Result<RemoteServer, String> {
    let host = host.trim();
    if host.is_empty()
        || host.starts_with('-')
        || host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '/' || c == '\\')
    {
        return Err("Indiquez un hôte SSH ou un alias valide, sans espace ni chemin.".into());
    }
    let mut projects = Vec::new();
    for path in paths.lines().map(str::trim).filter(|p| !p.is_empty()) {
        if !path.starts_with('/') || path.chars().any(char::is_control) {
            return Err(format!(
                "Chemin distant invalide : {path}. Utilisez un chemin absolu."
            ));
        }
        if !projects.iter().any(|existing| existing == path) {
            projects.push(path.to_string());
        }
    }
    Ok(RemoteServer {
        host: host.into(),
        label: if label.trim().is_empty() {
            host.into()
        } else {
            label.trim().into()
        },
        paths: projects,
    })
}

fn card(ui: &mut egui::Ui, pal: &Pal, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(pal.surface)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn dot(ui: &mut egui::Ui, color: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 5.0, color);
}

fn status(ui: &mut egui::Ui, color: Color32, text: &str) {
    ui.horizontal_wrapped(|ui| {
        dot(ui, color);
        ui.label(RichText::new(text).size(13.0).color(color));
    });
}

fn source_note(src: TokenSource, pal: &Pal) -> RichText {
    let s = match src {
        TokenSource::Env => {
            "jeton : variable ZREPO_RS_GITHUB_TOKEN ou GITHUB_TOKEN (prioritaire)".to_string()
        }
        other => format!("jeton : {}", other.label()),
    };
    RichText::new(s).size(12.5).color(pal.weak)
}

fn tool_row(ui: &mut egui::Ui, pal: &Pal, name: &str, found: Option<&str>, missing: &str) {
    ui.horizontal(|ui| {
        dot(ui, if found.is_some() { pal.ok } else { pal.faint });
        ui.label(RichText::new(name).size(14.5).color(pal.text));
        ui.add_space(6.0);
        match found {
            Some(p) => ui.label(
                RichText::new(format!("détecté : {}", tilde_path(Path::new(p))))
                    .size(12.5)
                    .color(pal.weak),
            ),
            None => ui.label(RichText::new(missing).size(12.5).color(pal.weak)),
        };
    });
}

fn code_line(ui: &mut egui::Ui, pal: &Pal, text: &str) {
    egui::Frame::new()
        .fill(pal.surface)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(RichText::new(text).monospace().size(12.5).color(pal.text))
                    .selectable(true),
            );
        });
}

/// Aperçu façon diff : lignes retirées en rouge, ajoutées en vert.
fn diff(ui: &mut egui::Ui, pal: &Pal, lines: &[hypr::Line]) {
    egui::Frame::new()
        .fill(if pal.dark {
            Color32::from_rgb(0x12, 0x12, 0x17)
        } else {
            Color32::from_rgb(0xFA, 0xFA, 0xFC)
        })
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            for l in lines {
                let (prefix, text, color, bg) = match l {
                    hypr::Line::Removed(t) => ("- ", t, pal.error, pal.error.gamma_multiply(0.10)),
                    hypr::Line::Added(t) => ("+ ", t, pal.ok, pal.ok.gamma_multiply(0.10)),
                    hypr::Line::Note(t) => ("# ", t, pal.faint, Color32::TRANSPARENT),
                };
                // Fond posé sous le texte une fois sa hauteur connue.
                let slot = ui.painter().add(egui::Shape::Noop);
                let r = ui.add(
                    egui::Label::new(
                        RichText::new(format!("{prefix}{text}"))
                            .monospace()
                            .size(12.0)
                            .color(color),
                    )
                    .wrap(),
                );
                if bg != Color32::TRANSPARENT {
                    let full = Rect::from_min_max(
                        pos2(r.rect.left() - 4.0, r.rect.top()),
                        pos2(ui.max_rect().right() + 4.0, r.rect.bottom()),
                    );
                    ui.painter().set(
                        slot,
                        egui::epaint::RectShape::new(
                            full,
                            CornerRadius::same(3),
                            bg,
                            Stroke::NONE,
                            StrokeKind::Inside,
                        ),
                    );
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serveur_saisi_et_chemins_valides() {
        let server =
            server_from_form(" VPSHost ", "", "/home/jp/a\n/home/jp/a\n/home/jp/b").unwrap();
        assert_eq!(server.host, "VPSHost");
        assert_eq!(server.label, "VPSHost");
        assert_eq!(server.paths, ["/home/jp/a", "/home/jp/b"]);
        assert!(server_from_form("-oProxyCommand=bad", "", "").is_err());
        assert!(server_from_form("user@host", "", "relative/path").is_err());
    }
}
