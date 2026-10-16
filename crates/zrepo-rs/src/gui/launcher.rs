//! Fenêtre de lancement : recherche, filtres, liste, détails, ouverture.

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Key, Layout, Modifiers, Rect, RichText, ScrollArea,
    Sense, Stroke, StrokeKind, TextEdit, UiBuilder, ViewportCommand, pos2, text::LayoutJob, vec2,
};
use nucleo_matcher::{Config as MatcherConfig, Matcher};
use zrepo_core::{
    config::Config,
    entry::{Entry, Kind},
    error::{EX_CANCEL, EX_OK, Res},
    open::{self, Target},
    sources,
};

use super::{
    search::{self, Filter, Match},
    theme::{self, Pal},
};

const ROW_H: f32 = 50.0;
const DETAILS_W: f32 = 272.0;

enum Msg {
    Locals(Vec<Entry>),
    Remotes(Vec<Entry>, Vec<String>),
    Github(Res<Vec<Entry>>),
}

enum OpenMsg {
    Progress(String),
    Done(Res<()>),
}

struct Opening {
    name: String,
    target: String,
    log: Vec<String>,
    rx: Receiver<OpenMsg>,
}

struct Failure {
    title: String,
    msg: String,
    code: i32,
}

pub enum Action {
    None,
    Close(i32),
    Settings,
    SettingsServers,
}

pub struct Launcher {
    pub cfg: Config,
    targets: Vec<Target>,
    target: Target,
    generation: u64,
    pending: u8,
    locals: Vec<Entry>,
    remotes: Vec<Entry>,
    github: Vec<Entry>,
    remote_warnings: Vec<String>,
    github_error: Option<String>,
    entries: Vec<Entry>,
    matcher: Matcher,
    query: String,
    filter: Filter,
    matches: Vec<Match>,
    view: Vec<usize>,
    counts: [usize; 4],
    sel: usize,
    scroll_to_sel: bool,
    view_h: f32,
    offset: f32,
    tx: Sender<(u64, Msg)>,
    rx: Receiver<(u64, Msg)>,
    opening: Option<Opening>,
    failure: Option<Failure>,
    /// Destination de clonage du dépôt sélectionné (clé, dossier).
    dest: Option<(String, Option<PathBuf>)>,
    notice: Option<(String, f64)>,
}

impl Launcher {
    pub fn new(
        ctx: &egui::Context,
        cfg: Result<Config, String>,
        target: Option<Target>,
        refresh: bool,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let (cfg, error) = match cfg {
            Ok(c) => (c, None),
            Err(e) => (Config::default(), Some(e)),
        };
        let mut l = Self {
            targets: Vec::new(),
            target: Target::Zed,
            cfg,
            generation: 0,
            pending: 0,
            locals: Vec::new(),
            remotes: Vec::new(),
            github: Vec::new(),
            remote_warnings: Vec::new(),
            github_error: None,
            entries: Vec::new(),
            matcher: Matcher::new(MatcherConfig::DEFAULT),
            query: std::env::var("ZREPO_RS_DEBUG_QUERY").unwrap_or_default(),
            filter: Filter::All,
            matches: Vec::new(),
            view: Vec::new(),
            counts: [0; 4],
            sel: 0,
            scroll_to_sel: false,
            view_h: 300.0,
            offset: 0.0,
            tx,
            rx,
            opening: None,
            failure: None,
            dest: None,
            notice: None,
        };
        l.apply_config(target);
        if let Some(e) = error {
            l.failure = Some(Failure {
                title: "Configuration illisible".into(),
                msg: format!(
                    "{e}\n\nCorrigez le fichier, ou refaites la configuration avec Ctrl-,."
                ),
                code: zrepo_core::error::EX_FAIL,
            });
        }
        l.load(ctx, refresh);
        l
    }

    /// Après l'assistant (Ctrl-,) : nouvelle configuration, nouvelle liste.
    pub fn reconfigure(&mut self, ctx: &egui::Context, cfg: Config) {
        let servers_changed = self.cfg.remote.servers != cfg.remote.servers;
        self.cfg = cfg;
        self.apply_config(None);
        self.failure = None;
        self.load(ctx, servers_changed);
    }

    fn apply_config(&mut self, target: Option<Target>) {
        self.targets = open::available_targets(&self.cfg);
        let want = target.unwrap_or_else(|| self.cfg.ui.default_target.clone());
        self.target = open::pick_target(&want, &self.targets);
    }

    /// Doit-on garder la fenêtre ouverte malgré la perte du focus ?
    pub fn busy(&self) -> bool {
        self.opening.is_some() || self.failure.is_some()
    }

    /// Trois sources en parallèle. Les résultats d'un chargement précédent
    /// (Ctrl-R pendant un chargement) sont ignorés grâce au numéro de génération.
    fn load(&mut self, ctx: &egui::Context, refresh: bool) {
        self.generation += 1;
        self.pending = 3;
        let generation = self.generation;
        let spawn = |job: Box<dyn FnOnce(&Config) -> Msg + Send>| {
            let (cfg, tx, ctx) = (self.cfg.clone(), self.tx.clone(), ctx.clone());
            thread::spawn(move || {
                let _ = tx.send((generation, job(&cfg)));
                ctx.request_repaint();
            });
        };
        spawn(Box::new(|cfg| Msg::Locals(sources::locals(cfg))));
        spawn(Box::new(move |cfg| {
            let (r, w) = sources::remotes(cfg, refresh);
            Msg::Remotes(r, w)
        }));
        spawn(Box::new(move |cfg| {
            Msg::Github(sources::github(cfg, refresh))
        }));
    }

    fn receive(&mut self) {
        let mut changed = false;
        while let Ok((generation, msg)) = self.rx.try_recv() {
            if generation != self.generation {
                continue;
            }
            self.pending = self.pending.saturating_sub(1);
            match msg {
                Msg::Locals(l) => self.locals = l,
                Msg::Remotes(r, w) => {
                    self.remotes = r;
                    self.remote_warnings = w;
                }
                Msg::Github(Ok(g)) => {
                    self.github = g;
                    self.github_error = None;
                }
                // L'ancienne liste GitHub reste affichée : mieux vaut périmé que vide.
                Msg::Github(Err(f)) => self.github_error = Some(f.msg),
            }
            changed = true;
        }
        if changed {
            self.entries = sources::combine(&self.locals, &self.remotes, &self.github);
            self.refilter(true);
        }
    }

    fn selected(&self) -> Option<&Entry> {
        let m = self.matches.get(*self.view.get(self.sel)?)?;
        self.entries.get(m.idx)
    }

    /// `keep` : nouvelles données, on garde le projet sélectionné. Sinon la
    /// recherche a changé et on repart du haut.
    fn refilter(&mut self, keep: bool) {
        let prev = keep
            .then(|| self.selected().map(|e| e.key.clone()))
            .flatten();
        self.matches = search::search(&mut self.matcher, &self.entries, &self.query);
        self.counts = search::counts(&self.entries, &self.matches);
        self.refilter_view();
        self.sel = prev
            .and_then(|k| {
                self.view
                    .iter()
                    .position(|&i| self.entries[self.matches[i].idx].key == k)
            })
            .unwrap_or(0);
        self.scroll_to_sel = true;
    }

    fn refilter_view(&mut self) {
        self.view = (0..self.matches.len())
            .filter(|&i| self.filter.accepts(self.entries[self.matches[i].idx].kind))
            .collect();
    }

    fn set_filter(&mut self, f: Filter) {
        self.filter = f;
        self.refilter_view();
        self.sel = 0;
        self.scroll_to_sel = true;
    }

    fn step(&mut self, delta: isize) {
        let len = self.view.len() as isize;
        if len == 0 {
            return;
        }
        self.sel = (self.sel as isize + delta).clamp(0, len - 1) as usize;
        self.scroll_to_sel = true;
    }

    fn page(&self) -> isize {
        ((self.view_h / ROW_H).floor() as isize - 1).max(1)
    }

    fn open(&mut self, ctx: &egui::Context, target: Target) {
        if !self.targets.contains(&target) {
            let why = match target {
                Target::Herdr if cfg!(windows) => "Herdr n'est pas disponible sous Windows.".into(),
                Target::Herdr if !self.cfg.herdr.enabled => {
                    "Herdr est désactivé dans la configuration (Ctrl-,).".into()
                }
                Target::Terminal => {
                    "Aucun terminal trouvé : indiquez-en un dans config.toml, section [terminal]."
                        .into()
                }
                t => format!("{} est introuvable.", t.label(&self.cfg)),
            };
            self.notify(ctx, &why);
            return;
        }
        let Some(e) = self.selected().cloned() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let cfg = self.cfg.clone();
        let c = ctx.clone();
        let key = e.key.clone();
        let label = target.label(&self.cfg);
        thread::spawn(move || {
            let progress = |m: &str| {
                let _ = tx.send(OpenMsg::Progress(m.to_string()));
                c.request_repaint();
            };
            let r = open::open(&cfg, &key, target, &progress);
            let _ = tx.send(OpenMsg::Done(r));
            c.request_repaint();
        });
        self.opening = Some(Opening {
            name: e.name.clone(),
            target: label,
            log: Vec::new(),
            rx,
        });
    }

    fn notify(&mut self, ctx: &egui::Context, msg: &str) {
        let until = ctx.input(|i| i.time) + 4.0;
        self.notice = Some((msg.to_string(), until));
        ctx.request_repaint_after(std::time::Duration::from_secs(4));
    }

    fn dest_for(&mut self, e: &Entry) -> Option<PathBuf> {
        if e.kind != Kind::Github {
            return None;
        }
        if self.dest.as_ref().is_none_or(|(k, _)| *k != e.key) {
            self.dest = Some((e.key.clone(), open::clone_destination(&self.cfg, &e.key)));
        }
        self.dest.as_ref().and_then(|(_, d)| d.clone())
    }

    /// Avance l'ouverture en cours ; Some(code) = fermer la fenêtre.
    fn poll_opening(&mut self) -> Option<i32> {
        let op = self.opening.as_mut()?;
        while let Ok(m) = op.rx.try_recv() {
            match m {
                OpenMsg::Progress(s) => op.log.push(s),
                OpenMsg::Done(Ok(())) => return Some(EX_OK),
                OpenMsg::Done(Err(f)) => {
                    let title = format!("Impossible d'ouvrir {}", op.name);
                    self.opening = None;
                    if f.msg.is_empty() {
                        return Some(f.code);
                    }
                    self.failure = Some(Failure {
                        title,
                        msg: f.msg,
                        code: f.code,
                    });
                    return None;
                }
            }
        }
        None
    }

    pub fn on_key(&mut self, ctx: &egui::Context, key: Key, m: Modifiers) -> Action {
        let ctrl = m.ctrl || m.command;
        if let Some(f) = &self.failure {
            return match key {
                Key::Escape => {
                    self.failure = None;
                    Action::None
                }
                Key::Enter => Action::Close(f.code),
                _ => Action::None,
            };
        }
        if self.opening.is_some() {
            return Action::None;
        }
        match key {
            Key::Escape => return Action::Close(EX_CANCEL),
            Key::Enter => self.open(ctx, self.target.clone()),
            Key::Tab if self.targets.len() > 1 => {
                let n = self.targets.len();
                let i = self
                    .targets
                    .iter()
                    .position(|t| *t == self.target)
                    .unwrap_or(0);
                let j = if m.shift {
                    (i + n - 1) % n
                } else {
                    (i + 1) % n
                };
                self.target = self.targets[j].clone();
            }
            Key::Tab => {
                let msg = format!(
                    "Une seule cible disponible : {}.",
                    self.target.label(&self.cfg)
                );
                self.notify(ctx, &msg);
            }
            Key::ArrowUp => self.step(-1),
            Key::ArrowDown => self.step(1),
            Key::PageUp => self.step(-self.page()),
            Key::PageDown => self.step(self.page()),
            Key::E if ctrl => self.open(ctx, Target::Zed),
            Key::Y if ctrl => self.open(ctx, Target::Herdr),
            Key::T if ctrl => self.open(ctx, Target::Terminal),
            Key::R if ctrl => {
                self.load(ctx, true);
                self.notify(ctx, "Actualisation : GitHub et scan des serveurs…");
            }
            Key::Comma if ctrl => return Action::Settings,
            Key::Num1 if ctrl => self.set_filter(Filter::All),
            Key::Num2 if ctrl => self.set_filter(Filter::Local),
            Key::Num3 if ctrl => self.set_filter(Filter::Remote),
            Key::Num4 if ctrl => self.set_filter(Filter::Github),
            _ => {}
        }
        Action::None
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Action {
        self.receive();
        if let Some(code) = self.poll_opening() {
            return Action::Close(code);
        }
        let ctx = ui.ctx().clone();
        if self.pending > 0 || self.opening.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let pal = Pal::of(ui);
        let card = ui.max_rect();
        let mut action = Action::None;

        let header = Rect::from_min_size(card.min, vec2(card.width(), 58.0));
        let pills = Rect::from_min_size(
            pos2(card.left(), header.bottom() + 12.0),
            vec2(card.width(), 28.0),
        );
        let footer = Rect::from_min_max(pos2(card.left(), card.bottom() - 46.0), card.max);
        let body = Rect::from_min_max(
            pos2(card.left(), pills.bottom() + 10.0),
            pos2(card.right(), footer.top()),
        );
        let list = Rect::from_min_max(body.min, pos2(body.right() - DETAILS_W, body.bottom()));
        let details = Rect::from_min_max(pos2(list.right(), body.top()), body.max);

        let overlay = self.busy();
        ui.add_enabled_ui(!overlay, |ui| {
            self.header_ui(ui, header, &pal, overlay);
            let p = ui.painter();
            p.hline(
                card.x_range(),
                header.bottom(),
                Stroke::new(1.0, pal.border),
            );
            if self.pills_ui(ui, pills) {
                action = Action::SettingsServers;
            }
            if self.list_ui(ui, list, &pal) {
                action = Action::SettingsServers;
            }
            self.details_ui(ui, details, &pal);
            ui.painter()
                .hline(card.x_range(), footer.top(), Stroke::new(1.0, pal.border));
            self.footer_ui(ui, footer, &pal);
        });
        if self.opening.is_some() {
            self.opening_ui(ui, card, &pal);
        } else if self.failure.is_some()
            && let Some(code) = self.failure_ui(ui, card, &pal)
        {
            action = Action::Close(code);
        }
        action
    }

    fn header_ui(&mut self, ui: &mut egui::Ui, rect: Rect, pal: &Pal, overlay: bool) {
        // Le lanceur n'a pas de bordure système : cette poignée permet de le déplacer.
        let handle = Rect::from_min_max(rect.min, pos2(rect.left() + 44.0, rect.bottom()));
        let drag = ui
            .interact(handle, ui.id().with("window_drag"), Sense::drag())
            .on_hover_cursor(egui::CursorIcon::Grab)
            .on_hover_text("Déplacer la fenêtre");
        if drag.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        for x in [20.0, 26.0] {
            for y in [-4.0, 0.0, 4.0] {
                ui.painter().circle_filled(
                    pos2(rect.left() + x, rect.center().y + y),
                    1.5,
                    pal.weak,
                );
            }
        }
        let count = if self.pending > 0 && self.entries.is_empty() {
            String::new()
        } else if self.query.trim().is_empty() {
            plural(self.entries.len(), "projet", "projets")
        } else {
            plural(self.matches.len(), "résultat", "résultats")
        };
        let cw = theme::text_width(ui, &count, FontId::proportional(12.5)).x;
        let right = rect.right() - 20.0;
        ui.painter().text(
            pos2(right, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            &count,
            FontId::proportional(12.5),
            pal.faint,
        );
        if self.pending > 0 {
            let s =
                Rect::from_center_size(pos2(right - cw - 16.0, rect.center().y), vec2(14.0, 14.0));
            theme::spinner_at(ui, s, pal.weak);
        }
        let edit_rect = Rect::from_min_max(
            pos2(rect.left() + 44.0, rect.top()),
            pos2(right - cw - 34.0, rect.bottom()),
        );
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(edit_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        let resp = child.add(
            TextEdit::singleline(&mut self.query)
                .id_salt("search")
                .frame(egui::Frame::NONE)
                .font(FontId::proportional(19.0))
                .hint_text(RichText::new("Rechercher un projet…").color(pal.faint))
                .desired_width(edit_rect.width()),
        );
        if !overlay && !resp.has_focus() {
            resp.request_focus();
        }
        if resp.changed() {
            self.refilter(false);
        }
    }

    fn pills_ui(&mut self, ui: &mut egui::Ui, rect: Rect) -> bool {
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(16.0, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 6.0;
        for (i, f) in Filter::ALL.into_iter().enumerate() {
            let hide_count = self.pending > 0 && self.counts[i] == 0;
            let count = (!hide_count).then_some(self.counts[i]);
            if theme::pill(&mut child, f.label(), count, self.filter == f)
                .on_hover_text(format!("Ctrl-{}", i + 1))
                .clicked()
            {
                self.set_filter(f);
            }
        }
        self.filter == Filter::Remote
            && child
                .small_button("Configurer les serveurs")
                .on_hover_text("Ajouter ou modifier les serveurs SSH")
                .clicked()
    }

    fn list_ui(&mut self, ui: &mut egui::Ui, rect: Rect, pal: &Pal) -> bool {
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect.shrink2(vec2(8.0, 0.0))));
        child.spacing_mut().item_spacing = vec2(0.0, 0.0);
        if self.view.is_empty() {
            let (title, sub) = if self.pending > 0 {
                ("Chargement des projets…".to_string(), String::new())
            } else if !self.query.trim().is_empty() {
                (
                    format!("Aucun projet ne correspond à « {} »", self.query.trim()),
                    "Essayez moins de lettres, ou un autre filtre.".to_string(),
                )
            } else if self.filter == Filter::Remote {
                (
                    "Aucun projet distant".to_string(),
                    "Recherchez vos hôtes SSH ou ajoutez un serveur dans les réglages.".to_string(),
                )
            } else if self.entries.is_empty() {
                (
                    "Aucun projet pour l'instant".to_string(),
                    format!(
                        "Les dépôts git de {} apparaîtront ici. Ctrl-, pour les réglages.",
                        self.cfg.projects_dir.display()
                    ),
                )
            } else {
                ("Rien dans cette catégorie".to_string(), String::new())
            };
            let c = rect.center() - vec2(0.0, 20.0);
            child.painter().text(
                c,
                egui::Align2::CENTER_CENTER,
                title,
                FontId::proportional(15.0),
                pal.weak,
            );
            if !sub.is_empty() {
                let g = child.painter().layout(
                    sub,
                    FontId::proportional(12.5),
                    pal.faint,
                    rect.width() - 60.0,
                );
                let pos = pos2(c.x - g.size().x / 2.0, c.y + 16.0);
                child.painter().galley(pos, g, pal.faint);
            }
            if self.pending == 0 && self.query.trim().is_empty() && self.filter == Filter::Remote {
                let button_rect = Rect::from_center_size(pos2(c.x, c.y + 72.0), vec2(190.0, 30.0));
                let mut button_ui = child.new_child(UiBuilder::new().max_rect(button_rect));
                return theme::button(&mut button_ui, "Configurer les serveurs", true).clicked();
            }
            return false;
        }
        let mut sa = ScrollArea::vertical()
            .id_salt("projets")
            .auto_shrink(false)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded);
        if self.scroll_to_sel {
            let top = self.sel as f32 * ROW_H;
            let mut off = self.offset;
            if top < off {
                off = top;
            } else if top + ROW_H > off + self.view_h {
                off = top + ROW_H - self.view_h;
            }
            sa = sa.vertical_scroll_offset(off.max(0.0));
            self.scroll_to_sel = false;
        }
        let mut open_now = false;
        let out = sa.show_rows(&mut child, ROW_H, self.view.len(), |ui, range| {
            for i in range {
                let (clicked, double) = self.row_ui(ui, i, pal);
                if clicked {
                    self.sel = i;
                }
                if double {
                    self.sel = i;
                    open_now = true;
                }
            }
        });
        self.offset = out.state.offset.y;
        self.view_h = out.inner_rect.height();
        if open_now {
            let ctx = ui.ctx().clone();
            self.open(&ctx, self.target.clone());
        }
        false
    }

    fn row_ui(&self, ui: &mut egui::Ui, i: usize, pal: &Pal) -> (bool, bool) {
        let m = &self.matches[self.view[i]];
        let e = &self.entries[m.idx];
        let (rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let p = ui.painter();
        let inner = rect.shrink2(vec2(4.0, 2.0));
        let selected = i == self.sel;
        let hover = ui
            .ctx()
            .animate_bool_with_time(resp.id, resp.hovered(), 0.12);
        if selected {
            p.rect_filled(inner, CornerRadius::same(10), pal.accent_soft);
            let bar = Rect::from_min_size(
                pos2(inner.left(), inner.top() + 12.0),
                vec2(3.0, inner.height() - 24.0),
            );
            p.rect_filled(bar, CornerRadius::same(2), pal.accent);
        } else if hover > 0.0 {
            let c = pal.hover.gamma_multiply(hover);
            p.rect_filled(inner, CornerRadius::same(10), c);
        }
        let icon = Rect::from_min_size(
            pos2(inner.left() + 12.0, inner.center().y - 15.0),
            vec2(30.0, 30.0),
        );
        theme::kind_icon(p, icon, e.kind, pal);

        // Badge à droite, puis le texte dans la place restante.
        let (label, fg, bg) = visibility_badge(e, pal);
        let b = theme::badge(
            p,
            pos2(inner.right() - 12.0, inner.center().y),
            label,
            fg,
            bg,
        );
        let x = icon.right() + 12.0;
        let max_w = b.left() - 12.0 - x;

        let mut job = LayoutJob::default();
        let base = egui::TextFormat {
            font_id: FontId::proportional(14.5),
            color: pal.text,
            ..Default::default()
        };
        let hit = egui::TextFormat {
            color: pal.hit,
            underline: Stroke::new(1.0, pal.hit),
            ..base.clone()
        };
        for (ci, ch) in e.name.chars().enumerate() {
            let f = if m.hits.binary_search(&ci).is_ok() {
                &hit
            } else {
                &base
            };
            job.append(ch.encode_utf8(&mut [0; 4]), 0.0, f.clone());
        }
        job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
        let g = p.layout_job(job);
        p.galley(pos2(x, inner.center().y - 17.0), g, pal.text);

        let sub = subtitle(e, &self.cfg.projects_dir);
        let mut job = LayoutJob::simple_singleline(sub, FontId::proportional(12.5), pal.weak);
        job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
        let g = p.layout_job(job);
        p.galley(pos2(x, inner.center().y + 3.0), g, pal.weak);
        (resp.clicked(), resp.double_clicked())
    }

    fn details_ui(&mut self, ui: &mut egui::Ui, rect: Rect, pal: &Pal) {
        let p = ui.painter();
        p.vline(rect.left(), rect.y_range(), Stroke::new(1.0, pal.border));
        let Some(e) = self.selected().cloned() else {
            return;
        };
        let dest = self.dest_for(&e);
        let inner = rect.shrink2(vec2(18.0, 14.0));
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
        );
        let ui = &mut child;
        ui.spacing_mut().item_spacing.y = 4.0;

        let (icon, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
        theme::kind_icon(ui.painter(), icon, e.kind, pal);
        ui.add_space(6.0);
        ui.add(egui::Label::new(RichText::new(&e.name).size(18.0).color(pal.text)).wrap());
        let kind = match e.kind {
            Kind::Local => "Projet local",
            Kind::Remote => "Projet distant (SSH)",
            Kind::Github if e.visibility == "private" => "Dépôt GitHub privé",
            Kind::Github => "Dépôt GitHub public",
        };
        ui.label(RichText::new(kind).size(12.5).color(pal.weak));
        ui.add_space(10.0);

        let field = |ui: &mut egui::Ui, label: &str, value: &str| {
            ui.label(
                RichText::new(label.to_uppercase())
                    .size(10.5)
                    .color(pal.faint),
            );
            ui.add(egui::Label::new(RichText::new(value).size(13.0).color(pal.text)).wrap());
            ui.add_space(8.0);
        };
        let scroll_h = inner.bottom() - ui.cursor().top() - 48.0;
        ScrollArea::vertical()
            .id_salt("details")
            .max_height(scroll_h.max(40.0))
            .auto_shrink([false, true])
            .show(ui, |ui| match e.kind {
                Kind::Local => {
                    field(ui, "Chemin", &tilde(&e.key));
                    if e.description.contains('/') {
                        field(ui, "Origine", &format!("github.com/{}", e.description));
                    } else {
                        field(ui, "Origine", "dépôt local, sans remote GitHub");
                    }
                }
                Kind::Remote => {
                    field(ui, "Serveur", &e.owner);
                    field(ui, "Chemin", &e.description);
                    field(ui, "Adresse", &e.key);
                }
                Kind::Github => {
                    field(ui, "Dépôt", &format!("github.com/{}", e.key));
                    if let Some(d) = e.description.strip_prefix("[archive] ") {
                        field(ui, "Statut", "archivé");
                        if !d.trim().is_empty() {
                            field(ui, "Description", d);
                        }
                    } else if !e.description.trim().is_empty() {
                        field(ui, "Description", &e.description);
                    }
                    match &dest {
                        Some(d) if d.exists() => {
                            field(ui, "Déjà présent dans", &tilde(&d.to_string_lossy()))
                        }
                        Some(d) => field(ui, "Sera cloné dans", &tilde(&d.to_string_lossy())),
                        None => field(
                            ui,
                            "Clonage",
                            "impossible : les dossiers cibles sont déjà pris (voir zrepo-rs open)",
                        ),
                    }
                }
            });

        let btn = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 36.0), inner.max);
        let label = format!("Ouvrir dans {}", self.target.label(&self.cfg));
        let mut c = ui.new_child(
            UiBuilder::new()
                .max_rect(btn)
                .layout(Layout::left_to_right(Align::Center)),
        );
        let b = egui::Button::new(RichText::new(label).size(13.5).color(Color32::WHITE))
            .fill(pal.accent)
            .corner_radius(CornerRadius::same(8))
            .min_size(vec2(btn.width(), 34.0))
            .truncate();
        if c.add(b).on_hover_text("Entrée").clicked() {
            let ctx = c.ctx().clone();
            self.open(&ctx, self.target.clone());
        }
    }

    fn footer_ui(&mut self, ui: &mut egui::Ui, rect: Rect, pal: &Pal) {
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(14.0, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let ui = &mut child;
        ui.spacing_mut().item_spacing.x = 6.0;

        // Sélecteur de cible : seulement les cibles disponibles. Au-delà de
        // trois, une liste déroulante tient dans la largeur de la fenêtre.
        if self.targets.len() > 3 {
            self.target_combo(ui);
        } else {
            self.target_segments(ui, pal);
        }
        ui.add_space(10.0);
        theme::keycap(ui, "Entrée", "ouvrir");
        if self.targets.len() > 1 {
            theme::keycap(ui, "Tab", "cible");
        }
        theme::keycap(ui, "Ctrl R", "actualiser");
        theme::keycap(ui, "Ctrl ,", "réglages");
        self.footer_status(ui, pal);
    }

    fn target_combo(&mut self, ui: &mut egui::Ui) {
        egui::ComboBox::from_id_salt("cible")
            .selected_text(self.target.label(&self.cfg))
            .width(170.0)
            .show_ui(ui, |ui| {
                for t in self.targets.clone() {
                    let label = t.label(&self.cfg);
                    ui.selectable_value(&mut self.target, t, label);
                }
            })
            .response
            .on_hover_text("Tab pour changer de cible");
    }

    fn target_segments(&mut self, ui: &mut egui::Ui, pal: &Pal) {
        let seg_h = 28.0;
        let labels: Vec<String> = self.targets.iter().map(|t| t.label(&self.cfg)).collect();
        let widths: Vec<f32> = labels
            .iter()
            .map(|l| theme::text_width(ui, l, FontId::proportional(13.0)).x + 22.0)
            .collect();
        let total: f32 = widths.iter().sum::<f32>() + 4.0;
        let (seg, _) = ui.allocate_exact_size(vec2(total, seg_h), Sense::hover());
        ui.painter().rect(
            seg,
            CornerRadius::same(8),
            pal.surface,
            Stroke::new(1.0, pal.border),
            StrokeKind::Inside,
        );
        let mut x = seg.left() + 2.0;
        for (i, t) in self.targets.clone().into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(x, seg.top() + 2.0), vec2(widths[i], seg_h - 4.0));
            x += widths[i];
            let resp = ui
                .interact(r, ui.id().with(("cible", i)), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Tab pour changer de cible");
            let on = t == self.target;
            if on {
                ui.painter()
                    .rect_filled(r, CornerRadius::same(6), pal.accent);
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(r, CornerRadius::same(6), pal.hover);
            }
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                &labels[i],
                FontId::proportional(13.0),
                if on { Color32::WHITE } else { pal.weak },
            );
            if resp.clicked() {
                self.target = t;
            }
        }
    }

    /// À droite : message temporaire, chargement ou avertissements.
    fn footer_status(&mut self, ui: &mut egui::Ui, pal: &Pal) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let now = ui.input(|i| i.time);
            let mut warnings: Vec<String> = self.remote_warnings.clone();
            if let Some(g) = &self.github_error {
                warnings.push(format!("GitHub : {g}"));
            }
            if let Some((msg, until)) = &self.notice
                && now < *until
            {
                ui.add(egui::Label::new(RichText::new(msg).size(12.0).color(pal.weak)).truncate());
            } else if self.pending > 0 {
                ui.label(RichText::new("Chargement…").size(12.0).color(pal.weak));
                theme::spinner(ui, 13.0, pal.weak);
            } else if !warnings.is_empty() {
                let text = if warnings.len() == 1 {
                    "1 avertissement".to_string()
                } else {
                    format!("{} avertissements", warnings.len())
                };
                let r = ui
                    .add(
                        egui::Label::new(
                            RichText::new(format!("⚠ {text}"))
                                .size(12.0)
                                .color(pal.warn),
                        )
                        .sense(Sense::hover()),
                    )
                    .on_hover_ui(|ui| {
                        ui.set_max_width(380.0);
                        for w in &warnings {
                            ui.label(RichText::new(w).size(12.5));
                        }
                    });
                r.on_hover_cursor(egui::CursorIcon::Help);
            }
        });
    }

    fn overlay_box(ui: &mut egui::Ui, card: Rect, pal: &Pal, height: f32) -> Rect {
        let dim = if pal.dark {
            Color32::from_black_alpha(150)
        } else {
            Color32::from_white_alpha(170)
        };
        ui.painter().rect_filled(card, CornerRadius::same(14), dim);
        let r = Rect::from_center_size(card.center(), vec2(480.0, height));
        let shadow = egui::Shadow {
            offset: [0, 6],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(if pal.dark { 120 } else { 40 }),
        };
        ui.painter().add(shadow.as_shape(r, CornerRadius::same(14)));
        ui.painter().rect(
            r,
            CornerRadius::same(14),
            pal.bg,
            Stroke::new(1.0, pal.border),
            StrokeKind::Inside,
        );
        r
    }

    fn opening_ui(&mut self, ui: &mut egui::Ui, card: Rect, pal: &Pal) {
        let Some(op) = &self.opening else { return };
        let shown = op.log.len().clamp(1, 4) as f32;
        let r = Self::overlay_box(ui, card, pal, 112.0 + shown * 20.0);
        let mut c = ui.new_child(
            UiBuilder::new()
                .max_rect(r.shrink(24.0))
                .layout(Layout::top_down(Align::Min)),
        );
        c.horizontal(|ui| {
            theme::spinner(ui, 18.0, pal.accent);
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!("Ouverture de {}", op.name))
                    .size(17.0)
                    .color(pal.text),
            );
        });
        c.label(
            RichText::new(format!("dans {}", op.target))
                .size(12.5)
                .color(pal.weak),
        );
        c.add_space(10.0);
        if op.log.is_empty() {
            c.label(RichText::new("Préparation…").size(12.5).color(pal.faint));
        }
        let n = op.log.len();
        for (i, line) in op.log.iter().enumerate().skip(n.saturating_sub(4)) {
            let color = if i + 1 == n { pal.text } else { pal.faint };
            c.add(egui::Label::new(RichText::new(line).size(12.5).color(color)).truncate());
        }
    }

    fn failure_ui(&mut self, ui: &mut egui::Ui, card: Rect, pal: &Pal) -> Option<i32> {
        let f = self.failure.as_ref()?;
        let lines = f.msg.lines().count().clamp(1, 8) as f32;
        let r = Self::overlay_box(ui, card, pal, 150.0 + lines * 19.0);
        let mut c = ui.new_child(
            UiBuilder::new()
                .max_rect(r.shrink(24.0))
                .layout(Layout::top_down(Align::Min)),
        );
        c.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 11.0, pal.error);
            ui.painter().text(
                dot.center(),
                egui::Align2::CENTER_CENTER,
                "!",
                FontId::proportional(15.0),
                Color32::WHITE,
            );
            ui.label(RichText::new(&f.title).size(17.0).color(pal.text));
        });
        c.add_space(8.0);
        let msg = f.msg.clone();
        let code = f.code;
        ScrollArea::vertical()
            .max_height(lines * 19.0 + 8.0)
            .auto_shrink([false, true])
            .show(&mut c, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(msg).size(13.0).color(pal.weak))
                        .wrap()
                        .selectable(true),
                );
            });
        let btns = Rect::from_min_max(
            pos2(r.left() + 24.0, r.bottom() - 58.0),
            pos2(r.right() - 24.0, r.bottom() - 20.0),
        );
        let mut b = ui.new_child(
            UiBuilder::new()
                .max_rect(btns)
                .layout(Layout::right_to_left(Align::Center)),
        );
        let mut close = None;
        if theme::button(&mut b, "Fermer", true)
            .on_hover_text("Entrée")
            .clicked()
        {
            close = Some(code);
        }
        if theme::button(&mut b, "Retour à la liste", false)
            .on_hover_text("Échap")
            .clicked()
        {
            self.failure = None;
        }
        close
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n > 1 { many } else { one })
}

/// ~/Projects/x plutôt que /home/moi/Projects/x.
fn tilde(path: &str) -> String {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    match path.strip_prefix(&home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with(['/', '\\'])) => {
            format!("~{rest}")
        }
        _ => path.to_string(),
    }
}

fn subtitle(e: &Entry, projects: &Path) -> String {
    match e.kind {
        // Le chemin relatif seulement s'il dit quelque chose de plus que le nom.
        Kind::Local => {
            let rel = Path::new(&e.key)
                .strip_prefix(projects)
                .ok()
                .map(|r| r.display().to_string())
                .filter(|r| r != &e.name);
            match (e.description.contains('/'), rel) {
                (true, Some(r)) => format!("{} · {r}", e.description),
                (true, None) => e.description.clone(),
                (false, Some(r)) => format!("{r} · sans remote GitHub"),
                (false, None) => "dépôt local, sans remote GitHub".to_string(),
            }
        }
        // description = hôte:chemin ; owner = nom du serveur dans Zed.
        Kind::Remote => match e.description.split_once(':') {
            Some((_, path)) => format!("{} · {path}", e.owner),
            None => e.description.clone(),
        },
        Kind::Github if e.description.trim().is_empty() => e.owner.clone(),
        Kind::Github => format!("{} · {}", e.owner, e.description),
    }
}

fn visibility_badge(e: &Entry, pal: &Pal) -> (&'static str, Color32, Color32) {
    let soft = |c: Color32| c.gamma_multiply(if pal.dark { 0.22 } else { 0.14 });
    let text = |c: Color32| {
        if pal.dark {
            theme::lerp(c, Color32::WHITE, 0.35)
        } else {
            theme::lerp(c, Color32::BLACK, 0.3)
        }
    };
    match e.kind {
        Kind::Local => {
            let c = pal.kind(Kind::Local);
            ("local", text(c), soft(c))
        }
        Kind::Remote => {
            let c = pal.kind(Kind::Remote);
            ("ssh", text(c), soft(c))
        }
        Kind::Github if e.description.starts_with("[archive]") => {
            ("archivé", pal.weak, soft(pal.weak))
        }
        Kind::Github if e.visibility == "private" => ("privé", text(pal.accent), soft(pal.accent)),
        Kind::Github => ("public", text(pal.ok), soft(pal.ok)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chemin_avec_tilde() {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return;
        }
        assert_eq!(tilde(&format!("{home}/Projects/x")), "~/Projects/x");
        assert_eq!(tilde(&format!("{home}x/y")), format!("{home}x/y"));
        assert_eq!(tilde("/srv/x"), "/srv/x");
    }
}
