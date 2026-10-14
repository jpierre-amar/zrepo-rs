//! Selecteur plein ecran (ratatui) avec recherche floue (nucleo).
//!
//! La liste s'affiche des que les projets locaux sont lus ; les serveurs et
//! GitHub arrivent ensuite en arriere-plan, sans bloquer la saisie.

use std::{
    io,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

use nucleo_matcher::{
    Config as MatchConfig, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    config::Config,
    entry::{Entry, Kind},
    error::{EX_FAIL, Fail, Res},
    github, list, local,
    open::Target,
    remote,
};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const NAME_W: usize = 26;
const OWNER_W: usize = 18;
const VIS_W: usize = 8;
const DETAILS_MIN_WIDTH: u16 = 110;

enum Msg {
    Locals(Vec<Entry>),
    Remotes(Vec<Entry>, Vec<String>),
    Github(Res<Vec<Entry>>),
}

/// Ligne retenue par le filtre : index dans `entries` et caracteres du nom
/// qui correspondent a la recherche.
struct Match {
    idx: usize,
    hits: Vec<usize>,
}

struct App {
    cfg: Config,
    generation: u64,
    pending: u8,
    locals: Vec<Entry>,
    remotes: Vec<Entry>,
    github: Vec<Entry>,
    remote_warnings: Vec<String>,
    github_error: Option<String>,
    entries: Vec<Entry>,
    query: String,
    matches: Vec<Match>,
    state: ListState,
    target: Target,
    matcher: Matcher,
    started: Instant,
    page: usize,
}

enum Action {
    Continue,
    Cancel,
    Open(Target),
}

/// Lance le selecteur. `None` = annule par l'utilisateur.
pub fn run(cfg: &Config, refresh: bool, default: Target) -> Res<Option<(String, Target)>> {
    let (tx, rx) = mpsc::channel();
    let mut app = App::new(cfg.clone(), default);
    app.load(refresh, &tx);

    let mut terminal = ratatui::init();
    let result = app.event_loop(&mut terminal, &tx, &rx);
    ratatui::restore();
    result.map_err(|e| Fail::new(EX_FAIL, format!("Erreur du terminal : {e}")))
}

impl App {
    fn new(cfg: Config, target: Target) -> Self {
        Self {
            cfg,
            generation: 0,
            pending: 0,
            locals: Vec::new(),
            remotes: Vec::new(),
            github: Vec::new(),
            remote_warnings: Vec::new(),
            github_error: None,
            entries: Vec::new(),
            query: String::new(),
            matches: Vec::new(),
            state: ListState::default(),
            target,
            matcher: Matcher::new(MatchConfig::DEFAULT),
            started: Instant::now(),
            page: 10,
        }
    }

    /// Trois sources en parallele. Les resultats d'un chargement precedent
    /// (Ctrl-R pendant un chargement) sont ignores grace au numero de generation.
    fn load(&mut self, refresh: bool, tx: &Sender<(u64, Msg)>) {
        self.generation += 1;
        self.pending = 3;
        self.started = Instant::now();
        let generation = self.generation;

        let spawn = |job: Box<dyn FnOnce(&Config) -> Msg + Send>| {
            let (cfg, tx) = (self.cfg.clone(), tx.clone());
            thread::spawn(move || {
                let _ = tx.send((generation, job(&cfg)));
            });
        };
        spawn(Box::new(|cfg| Msg::Locals(local::entries(cfg))));
        spawn(Box::new(move |cfg| {
            let (r, w) = remote::entries(cfg, refresh);
            Msg::Remotes(r, w)
        }));
        spawn(Box::new(move |cfg| {
            Msg::Github(github::entries(cfg, refresh))
        }));
    }

    fn receive(&mut self, generation: u64, msg: Msg) {
        if generation != self.generation {
            return;
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
            // L'ancienne liste GitHub reste affichee : mieux vaut perime que vide.
            Msg::Github(Err(f)) => self.github_error = Some(f.msg),
        }
        self.entries = list::combine(&self.locals, &self.remotes, &self.github);
        self.refilter(true);
    }

    fn selected(&self) -> Option<&Entry> {
        let m = self.matches.get(self.state.selected()?)?;
        self.entries.get(m.idx)
    }

    /// `keep` : nouvelles donnees, on garde le projet selectionne. Sinon la
    /// recherche a change et on repart du haut, comme fzf.
    fn refilter(&mut self, keep: bool) {
        let prev = keep
            .then(|| self.selected().map(|e| e.key.clone()))
            .flatten();

        self.matches = if self.query.trim().is_empty() {
            (0..self.entries.len())
                .map(|idx| Match {
                    idx,
                    hits: Vec::new(),
                })
                .collect()
        } else {
            let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
            let (mut buf, mut indices) = (Vec::new(), Vec::new());
            let mut scored: Vec<(u32, Match)> = Vec::new();
            for (idx, e) in self.entries.iter().enumerate() {
                // Le nom en tete : les indices qui tombent dedans sont surlignes.
                let hay = format!(
                    "{}  {}  {}  {}  {}",
                    e.name,
                    e.owner,
                    e.visibility,
                    e.description,
                    e.kind.as_str()
                );
                indices.clear();
                let Some(score) = pattern.indices(
                    Utf32Str::new(&hay, &mut buf),
                    &mut self.matcher,
                    &mut indices,
                ) else {
                    continue;
                };
                let n = e.name.chars().count();
                let mut hits: Vec<usize> = indices
                    .iter()
                    .map(|&i| i as usize)
                    .filter(|&i| i < n)
                    .collect();
                hits.sort_unstable();
                hits.dedup();
                scored.push((score, Match { idx, hits }));
            }
            // A score egal, l'ordre d'origine : locaux, distants, GitHub.
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.idx.cmp(&b.1.idx)));
            scored.into_iter().map(|(_, m)| m).collect()
        };

        let pos = prev
            .and_then(|k| {
                self.matches
                    .iter()
                    .position(|m| self.entries[m.idx].key == k)
            })
            .unwrap_or(0);
        self.state.select((!self.matches.is_empty()).then_some(pos));
        if !keep {
            *self.state.offset_mut() = 0;
        }
    }

    fn step(&mut self, delta: isize, wrap: bool) {
        let len = self.matches.len() as isize;
        if len == 0 {
            return;
        }
        let cur = self.state.selected().unwrap_or(0) as isize;
        let next = if wrap {
            (cur + delta).rem_euclid(len)
        } else {
            (cur + delta).clamp(0, len - 1)
        };
        self.state.select(Some(next as usize));
    }

    fn event_loop(
        &mut self,
        terminal: &mut DefaultTerminal,
        tx: &Sender<(u64, Msg)>,
        rx: &Receiver<(u64, Msg)>,
    ) -> io::Result<Option<(String, Target)>> {
        loop {
            while let Ok((generation, msg)) = rx.try_recv() {
                self.receive(generation, msg);
            }
            terminal.draw(|f| self.draw(f))?;

            if !event::poll(Duration::from_millis(80))? {
                continue;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match self.on_key(key, tx) {
                Action::Continue => {}
                Action::Cancel => return Ok(None),
                Action::Open(target) => {
                    if let Some(e) = self.selected() {
                        return Ok(Some((e.key.clone(), target)));
                    }
                }
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, tx: &Sender<(u64, Msg)>) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return Action::Cancel,
            KeyCode::Char('c' | 'q') if ctrl => return Action::Cancel,
            KeyCode::Enter => return Action::Open(self.target),
            KeyCode::Char('e') if ctrl => return Action::Open(Target::Zed),
            KeyCode::Char('y') if ctrl => return Action::Open(Target::Codex),
            KeyCode::Char('r') if ctrl => self.load(true, tx),
            KeyCode::Left => self.target = Target::Zed,
            KeyCode::Right => self.target = Target::Codex,
            KeyCode::Tab | KeyCode::BackTab => self.target = self.target.other(),
            KeyCode::Up => self.step(-1, true),
            KeyCode::Down => self.step(1, true),
            KeyCode::Char('p' | 'k') if ctrl => self.step(-1, true),
            KeyCode::Char('n' | 'j') if ctrl => self.step(1, true),
            KeyCode::PageUp => self.step(-(self.page as isize), false),
            KeyCode::PageDown => self.step(self.page as isize, false),
            KeyCode::Home => self.step(isize::MIN / 2, false),
            KeyCode::End => self.step(isize::MAX / 2, false),
            KeyCode::Backspace => {
                self.query.pop();
                self.refilter(false);
            }
            KeyCode::Char('u') if ctrl => {
                self.query.clear();
                self.refilter(false);
            }
            KeyCode::Char('w') if ctrl => {
                let trimmed = self.query.trim_end();
                let cut = trimmed.rfind(char::is_whitespace).map_or(0, |i| i + 1);
                self.query.truncate(cut);
                self.refilter(false);
            }
            KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                self.query.push(c);
                self.refilter(false);
            }
            _ => {}
        }
        Action::Continue
    }

    // --- rendu ---------------------------------------------------------------

    fn draw(&mut self, f: &mut Frame) {
        let outer = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Line::from(" Projets ").bold())
            .border_style(Style::new().fg(Color::DarkGray));
        let inner = outer.inner(f.area());
        f.render_widget(outer, f.area());

        let [header, target, search, sep, body, status] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(inner);

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(format!(" {} projets", self.entries.len())).bold(),
                Span::raw(" · ←/→ cible · Entrée ouvre · Ctrl-E Zed · Ctrl-Y Herdr · Ctrl-R rafraîchit · Échap quitte")
                    .dim(),
            ])),
            header,
        );
        self.draw_target(f, target);
        self.draw_search(f, search);
        f.render_widget(
            Block::new()
                .borders(Borders::TOP)
                .border_style(Style::new().fg(Color::DarkGray)),
            sep,
        );

        if body.width >= DETAILS_MIN_WIDTH {
            let [list_area, details] =
                Layout::horizontal([Constraint::Min(1), Constraint::Length(44)]).areas(body);
            self.draw_list(f, list_area);
            self.draw_details(f, details);
        } else {
            self.draw_list(f, body);
        }
        self.draw_status(f, status);
    }

    fn draw_target(&self, f: &mut Frame, area: Rect) {
        let on = Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED);
        let off = Style::new().add_modifier(Modifier::DIM);
        let (z, c) = match self.target {
            Target::Zed => (on, off),
            Target::Codex => (off, on),
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(" Ouvrir dans  "),
                Span::styled(" Zed ", z),
                Span::raw("   "),
                Span::styled(format!(" Herdr + {} ", self.cfg.agent_label()), c),
            ])),
            area,
        );
    }

    fn draw_search(&self, f: &mut Frame, area: Rect) {
        let line = Line::from(vec![
            Span::raw(" 🔎 "),
            Span::raw("Rechercher > ").fg(Color::Cyan),
            Span::raw(self.query.as_str()),
        ]);
        let x = area.x + (line.width() as u16).min(area.width.saturating_sub(1));
        f.render_widget(Paragraph::new(line), area);
        f.set_cursor_position(Position::new(x, area.y));
    }

    fn draw_list(&mut self, f: &mut Frame, area: Rect) {
        self.page = (area.height as usize).saturating_sub(1).max(1);

        if self.matches.is_empty() {
            let msg = if self.pending > 0 {
                "Chargement…".to_string()
            } else if self.entries.is_empty() {
                let mut m = "Aucun projet trouvé (ni dans ~/Projects, ni sur GitHub).".to_string();
                if let Some(e) = &self.github_error {
                    m.push_str("\n\n");
                    m.push_str(e);
                }
                m
            } else {
                "Aucune correspondance.".to_string()
            };
            f.render_widget(
                Paragraph::new(msg).dim().wrap(Wrap { trim: false }),
                area.inner(ratatui::layout::Margin::new(2, 1)),
            );
            return;
        }

        let items: Vec<ListItem> = self
            .matches
            .iter()
            .map(|m| ListItem::new(row(&self.entries[m.idx], &m.hits)))
            .collect();
        let list = List::new(items).highlight_symbol("▸ ").highlight_style(
            Style::new()
                .bg(Color::Indexed(236))
                .add_modifier(Modifier::BOLD),
        );
        f.render_stateful_widget(list, area, &mut self.state);
    }

    fn draw_details(&self, f: &mut Frame, area: Rect) {
        let block = Block::new()
            .borders(Borders::LEFT)
            .border_style(Style::new().fg(Color::DarkGray))
            .padding(ratatui::widgets::Padding::horizontal(1));
        let Some(e) = self.selected() else {
            f.render_widget(block, area);
            return;
        };
        let (tag, color) = tag(e.kind);
        let field = |label: &'static str, value: String| {
            vec![
                Line::from(Span::raw(label).dim()),
                Line::from(value),
                Line::raw(""),
            ]
        };

        let mut lines = vec![
            Line::from(Span::raw(e.name.clone()).bold()),
            Line::from(Span::styled(tag, Style::new().fg(color))),
            Line::raw(""),
        ];
        match e.kind {
            Kind::Local => {
                lines.extend(field("Dossier", e.localpath.clone()));
                lines.extend(field("Dépôt d'origine", e.description.clone()));
            }
            Kind::Remote => {
                lines.extend(field("Serveur", e.owner.clone()));
                lines.extend(field("Emplacement", e.description.clone()));
                lines.extend(field("Ouverture", "via ssh, sans clonage".into()));
            }
            Kind::Github => {
                lines.extend(field("Dépôt", e.key.clone()));
                lines.extend(field("Visibilité", e.visibility.clone()));
                if !e.description.is_empty() {
                    lines.extend(field("Description", e.description.clone()));
                }
                lines.extend(field(
                    "À l'ouverture",
                    format!(
                        "sera cloné dans {}",
                        self.cfg.projects_dir.join(&e.name).display()
                    ),
                ));
            }
        }
        f.render_widget(
            Paragraph::new(lines)
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn draw_status(&self, f: &mut Frame, area: Rect) {
        let count = format!("{}/{} ", self.matches.len(), self.entries.len());
        let [left, right] =
            Layout::horizontal([Constraint::Min(1), Constraint::Length(count.len() as u16)])
                .areas(area);

        let line = if self.pending > 0 {
            let frame = (self.started.elapsed().as_millis() / 80) as usize % SPINNER.len();
            Line::from(vec![
                Span::raw(format!(" {} ", SPINNER[frame])).fg(Color::Cyan),
                Span::raw("Chargement des serveurs et de GitHub…").dim(),
            ])
        } else {
            let mut warnings: Vec<&str> = self.remote_warnings.iter().map(String::as_str).collect();
            if let Some(e) = &self.github_error {
                warnings.push(e.lines().next().unwrap_or(e));
            }
            if warnings.is_empty() {
                Line::raw("")
            } else {
                Line::from(Span::raw(format!(" ⚠ {}", warnings.join(" · "))).fg(Color::Yellow))
            }
        };
        f.render_widget(Paragraph::new(line), left);
        f.render_widget(Paragraph::new(count).dim(), right);
    }
}

fn tag(kind: Kind) -> (&'static str, Color) {
    match kind {
        Kind::Local => ("● LOCAL ", Color::Green),
        Kind::Remote => ("◆ REMOTE", Color::Magenta),
        Kind::Github => ("☁ GITHUB", Color::Blue),
    }
}

/// Tronque avec « … » ou complete par des espaces, en caracteres.
fn fit(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n > width {
        s.chars()
            .take(width - 1)
            .chain(std::iter::once('…'))
            .collect()
    } else {
        format!("{s}{}", " ".repeat(width - n))
    }
}

fn row(e: &Entry, hits: &[usize]) -> Line<'static> {
    let (tag, color) = tag(e.kind);
    let mut spans = vec![Span::styled(tag, Style::new().fg(color)), Span::raw("  ")];

    let hit = Style::new()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    for (i, c) in fit(&e.name, NAME_W).chars().enumerate() {
        let style = if hits.binary_search(&i).is_ok() {
            hit
        } else {
            Style::new()
        };
        spans.push(Span::styled(c.to_string(), style));
    }

    let vis_color = match e.visibility.as_str() {
        "private" => Color::Yellow,
        "public" => Color::Cyan,
        _ => Color::DarkGray,
    };
    spans.extend([
        Span::raw(" "),
        Span::raw(fit(&e.owner, OWNER_W)).dim(),
        Span::raw(" "),
        Span::styled(fit(&e.visibility, VIS_W), Style::new().fg(vis_color)),
        Span::raw(" "),
        Span::raw(e.description.clone()).dim(),
    ]);
    Line::from(spans)
}
