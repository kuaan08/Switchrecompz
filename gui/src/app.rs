//! The main window: Recompile, Library and About tabs.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, CornerRadius, Id, Label, Layout, Margin, RichText, ScrollArea, Sense, Stroke, TextEdit,
    TextStyle, TextWrapMode, Ui, Vec2,
};

use crate::game::{self, GameInfo};
use crate::ini::{self, GameIni, Kind, Opt};
use crate::job::{Job, JobConfig, JobEvent, LineKind, Phase, Progress};
use crate::library::{self, CodeCompiler, Game};
use crate::payload;
use crate::settings::{Compiler, Settings};
use crate::theme;
use crate::toolchain::{self, Toolchain, Versions};

const MAX_LOG_LINES: usize = 50_000;
const FORM_WIDTH: f32 = 420.0;
const DISCORD: &str = "https://discord.gg/gdRaXmBTY";
const SETTINGS_FILE: &str = "SwitchRecomp.json";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Recompile,
    Library,
    About,
}

struct LogLine {
    text: String,
    kind: LineKind,
}

enum Outcome {
    Success(PathBuf),
    Failed(Option<i32>),
    Cancelled,
}

enum Confirm {
    Quit,
    CleanBuild(Game),
}

struct IniEditor {
    game: Game,
    ini: GameIni,
    changed: bool,
}

type FileFilter = (&'static str, &'static [&'static str]);

/// The single-file build unpacking its tools on first start.
struct Unpack {
    done: Arc<AtomicU64>,
    rx: mpsc::Receiver<Result<PathBuf, String>>,
    error: Option<String>,
}

impl Unpack {
    fn start(ctx: &egui::Context) -> Self {
        let done = Arc::new(AtomicU64::new(0));
        let (tx, rx) = mpsc::channel();
        let (counter, ctx) = (done.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(payload::unpack_cached(&counter).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        Self { done, rx, error: None }
    }

    fn fraction(&self) -> f32 {
        self.done.load(Ordering::Relaxed) as f32 / payload::packed_len().max(1) as f32
    }
}

/// What the UI asked for. Run after drawing, when nothing is borrowed and the
/// native dialogs can be parented to the window.
enum Action {
    PickInputFolder,
    PickImport(u8),
    PickInputNro,
    PickOutDir,
    PickIniFile(&'static str, FileFilter),
    Start,
    Cancel,
    Play(PathBuf),
    Reveal(PathBuf),
    Open(PathBuf),
    EditSettings(Game),
    AskCleanBuild(Game),
    CleanBuild(Game),
    Rescan,
    Quit,
}

pub struct App {
    tc: Toolchain,
    /// Where games and settings go: the release folder, or next to the single-file exe.
    data: Option<PathBuf>,
    unpack: Option<Unpack>,
    versions: Versions,
    versions_rx: Option<mpsc::Receiver<Versions>>,
    settings: Settings,
    saved: Settings,
    last_save: Instant,
    tab: Tab,
    inspected: Option<(String, Result<GameInfo, String>, Instant)>,
    job: Option<Job>,
    progress: Progress,
    elapsed: Duration,
    log: Vec<LogLine>,
    outcome: Option<Outcome>,
    games: Vec<Game>,
    editor: Option<IniEditor>,
    confirm: Option<Confirm>,
    notice: Option<(String, bool, Instant)>,
    actions: Vec<Action>,
    quitting: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let (tc, data, unpack) = if payload::embedded() {
            let data = Some(portable_dir());
            if payload::ready() {
                (Toolchain::at(payload::home()), data, None)
            } else {
                (Toolchain::default(), data, Some(Unpack::start(&cc.egui_ctx)))
            }
        } else {
            let tc = Toolchain::detect();
            let data = tc.home.clone();
            (tc, data, None)
        };
        let settings = data.as_ref().map(|d| Settings::load(&d.join(SETTINGS_FILE))).unwrap_or_default();

        let mut app = Self {
            tc,
            data,
            unpack,
            versions: Versions::default(),
            versions_rx: None,
            saved: settings.clone(),
            settings,
            last_save: Instant::now(),
            tab: Tab::Recompile,
            inspected: None,
            job: None,
            progress: Progress::default(),
            elapsed: Duration::ZERO,
            log: Vec::new(),
            outcome: None,
            games: Vec::new(),
            editor: None,
            confirm: None,
            notice: None,
            actions: Vec::new(),
            quitting: false,
        };
        if app.unpack.is_none() {
            app.tools_ready(&cc.egui_ctx);
        }
        app.rescan();
        app
    }

    /// Probes the tool versions and falls back to whichever compiler exists.
    fn tools_ready(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let (probe, ctx) = (self.tc.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(probe.probe_versions());
            ctx.request_repaint();
        });
        self.versions_rx = Some(rx);
        let tc = &self.tc;
        match self.settings.compiler {
            Compiler::Gcc if tc.gcc.is_none() && tc.tcc.is_some() => self.settings.compiler = Compiler::Tcc,
            Compiler::Tcc if tc.tcc.is_none() && tc.gcc.is_some() => self.settings.compiler = Compiler::Gcc,
            _ => {}
        }
        // a missing compiler is no reason to rewrite the saved choice
        self.saved.compiler = self.settings.compiler;
    }

    fn poll_unpack(&mut self, ctx: &egui::Context) {
        let Some(u) = &mut self.unpack else { return };
        match u.rx.try_recv() {
            Ok(Ok(home)) => {
                self.unpack = None;
                self.tc = Toolchain::at(Some(home));
                self.tools_ready(ctx);
            }
            Ok(Err(e)) => u.error = Some(e),
            Err(_) if u.error.is_none() => ctx.request_repaint_after(Duration::from_millis(100)),
            Err(_) => {}
        }
    }

    // ------------------------------------------------------------------ state

    fn settings_path(&self) -> Option<PathBuf> {
        self.data.as_ref().map(|d| d.join(SETTINGS_FILE))
    }

    fn games_dir(&self) -> Option<PathBuf> {
        self.data.as_ref().map(|d| d.join("games"))
    }

    fn save_settings(&mut self, force: bool) {
        if self.settings == self.saved || (!force && self.last_save.elapsed() < Duration::from_secs(1)) {
            return;
        }
        if let Some(path) = self.settings_path() {
            let _ = self.settings.save(&path);
        }
        self.saved = self.settings.clone();
        self.last_save = Instant::now();
    }

    fn out_dir(&self) -> Option<PathBuf> {
        let custom = self.settings.out_dir.trim();
        if custom.is_empty() { self.games_dir() } else { Some(PathBuf::from(custom)) }
    }

    fn jobs(&self) -> usize {
        if self.settings.jobs == 0 { ncpu() } else { self.settings.jobs.min(ncpu()) }
    }

    fn compiler_path(&self) -> Option<PathBuf> {
        match self.settings.compiler {
            Compiler::Tcc => self.tc.tcc.clone(),
            Compiler::Gcc => self.tc.gcc.clone(),
        }
    }

    fn info(&self) -> Option<&Result<GameInfo, String>> {
        self.inspected.as_ref().map(|(_, r, _)| r)
    }

    /// Re-reads the input when it changes, and every few seconds in case the
    /// folder is still being extracted.
    fn refresh_inspection(&mut self) {
        let input = self.settings.input.trim().to_string();
        let fresh = self
            .inspected
            .as_ref()
            .is_some_and(|(k, _, at)| *k == input && (self.job.is_some() || at.elapsed() < Duration::from_secs(3)));
        if !fresh {
            let result = game::inspect(Path::new(&input));
            self.inspected = Some((input, result, Instant::now()));
        }
    }

    /// Output name switchrecomp.py will use.
    fn title(&self) -> String {
        let name = self.settings.name.trim();
        if !name.is_empty() {
            return game::sanitize_title(name);
        }
        match self.info() {
            Some(Ok(info)) => info.default_title.clone(),
            _ => "Game".into(),
        }
    }

    fn planned_exe(&self) -> Option<PathBuf> {
        Some(self.out_dir()?.join(format!("{}{}", self.title(), std::env::consts::EXE_SUFFIX)))
    }

    /// Why Recompile can't run right now.
    fn blocker(&self) -> Option<String> {
        if self.tc.home.is_none() {
            return Some("switchrecomp.py was not found next to the app.".into());
        }
        if self.tc.python.is_none() {
            return Some(format!("Python 3 was not found. {}", toolchain::PYTHON_HINT));
        }
        if self.compiler_path().is_none() {
            return Some(match self.settings.compiler {
                Compiler::Tcc => "TCC was not found (tcc folder).".into(),
                Compiler::Gcc => format!("GCC was not found. {}", toolchain::GCC_HINT),
            });
        }
        match self.info() {
            Some(Ok(_)) => None,
            Some(Err(e)) => Some(e.clone()),
            None => Some("No game selected.".into()),
        }
    }

    fn set_input(&mut self, p: PathBuf) {
        self.settings.input = p.display().to_string();
        // a custom name belonged to the previous game
        self.settings.name.clear();
        self.inspected = None;
    }

    fn notify(&mut self, msg: impl Into<String>, error: bool) {
        self.notice = Some((msg.into(), error, Instant::now()));
    }

    fn rescan(&mut self) {
        self.games = self.out_dir().map(|d| library::scan(&d)).unwrap_or_default();
    }

    fn push_log(&mut self, text: String) {
        if self.log.len() >= MAX_LOG_LINES {
            self.log.drain(..MAX_LOG_LINES / 10);
        }
        let kind = crate::job::classify(&text);
        self.log.push(LogLine { text, kind });
    }

    // ------------------------------------------------------------------ job

    fn start(&mut self, ctx: &egui::Context) {
        if self.job.is_some() || self.blocker().is_some() {
            return;
        }
        let (Some(home), Some(script), Some(python), Some(cc), Some(out_dir)) =
            (self.tc.home.clone(), self.tc.script(), self.tc.python.clone(), self.compiler_path(), self.out_dir())
        else {
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            self.notify(format!("Cannot create {}: {e}", out_dir.display()), true);
            return;
        }
        let input = self.settings.input.trim().to_string();
        let cfg = JobConfig {
            python,
            home,
            script,
            input: PathBuf::from(&input),
            hactool: if self.settings.hactool.trim().is_empty() {
                self.tc.hactool().map(|p| p.display().to_string()).unwrap_or_default()
            } else {
                self.settings.hactool.clone()
            },
            prod_keys: self.settings.prod_keys.clone(),
            title_keys: self.settings.title_keys.clone(),
            out_dir,
            name: Some(self.title()),
            cc,
            gcc: self.settings.compiler == Compiler::Gcc,
            opt_level: self.settings.opt_level,
            native: self.settings.native,
            jobs: self.jobs(),
            interp_only: self.settings.interp_only,
        };
        self.log.clear();
        self.push_log(format!("> {}", cfg.display()));
        self.progress = Progress::default();
        self.elapsed = Duration::ZERO;
        match Job::start(&cfg, ctx) {
            Ok(job) => {
                self.job = Some(job);
                self.outcome = None;
                self.settings.remember_input(&input);
            }
            Err(e) => {
                self.push_log(format!("Could not start Python: {e}"));
                self.progress.phase = Phase::Failed;
                self.outcome = Some(Outcome::Failed(None));
            }
        }
    }

    fn poll_job(&mut self) {
        let Some(job) = &self.job else { return };
        let events = job.events();
        self.elapsed = job.started.elapsed();
        let cancelled = job.cancelled;
        let mut exit = None;
        for ev in events {
            match ev {
                JobEvent::Line(line) => {
                    self.progress.feed(&line);
                    self.push_log(line);
                }
                JobEvent::Exited(code) => exit = Some(code),
            }
        }
        let Some(code) = exit else { return };
        self.job = None;
        self.rescan();
        let (phase, outcome) = if cancelled {
            self.push_log("Cancelled.".into());
            (Phase::Cancelled, Outcome::Cancelled)
        } else if code == Some(0) {
            let exe = self.progress.exe.clone().or_else(|| self.planned_exe()).unwrap_or_default();
            (Phase::Done, Outcome::Success(exe))
        } else {
            (Phase::Failed, Outcome::Failed(code))
        };
        self.progress.phase = phase;
        self.outcome = Some(outcome);
    }

    fn run_actions(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        for action in std::mem::take(&mut self.actions) {
            match action {
                Action::PickImport(which) => {
                    if let Some(p) = dialog(frame, None)
                        .set_title("Select your local key file or extractor override")
                        .pick_file()
                    {
                        let value = p.display().to_string();
                        match which {
                            0 => self.settings.hactool = value,
                            1 => self.settings.prod_keys = value,
                            _ => self.settings.title_keys = value,
                        }
                    }
                }
                Action::PickInputFolder => {
                    let start = Path::new(self.settings.input.trim()).parent().map(Path::to_path_buf);
                    if let Some(p) = dialog(frame, start.as_deref())
                        .set_title("Choose the game folder (with exefs and romfs)")
                        .pick_folder()
                    {
                        self.set_input(p);
                    }
                }
                Action::PickInputNro => {
                    let start = Path::new(self.settings.input.trim()).parent().map(Path::to_path_buf);
                    if let Some(p) = dialog(frame, start.as_deref())
                        .set_title("Choose a Switch game file")
                        .add_filter("Switch games", &["nro", "nsp", "xci", "nca"])
                        .pick_file()
                    {
                        self.set_input(p);
                    }
                }
                Action::PickOutDir => {
                    if let Some(p) = dialog(frame, self.out_dir().as_deref()).set_title("Output folder").pick_folder() {
                        self.settings.out_dir = p.display().to_string();
                        self.rescan();
                    }
                }
                Action::PickIniFile(key, (name, exts)) => {
                    if let Some(p) = dialog(frame, None).add_filter(name, exts).pick_file()
                        && let Some(ed) = &mut self.editor
                    {
                        ed.ini.set(key, &p.display().to_string());
                        ed.changed = true;
                    }
                }
                Action::Start => self.start(ctx),
                Action::Cancel => {
                    if let Some(job) = &mut self.job {
                        job.cancel();
                    }
                }
                Action::Play(exe) => {
                    if let Err(e) = library::launch(&exe) {
                        self.notify(format!("Could not start {}: {e}", exe.display()), true);
                    }
                }
                Action::Reveal(p) => {
                    let _ = library::reveal(&p);
                }
                Action::Open(p) => {
                    if let Err(e) = library::open(&p) {
                        self.notify(format!("Could not open {}: {e}", p.display()), true);
                    }
                }
                Action::EditSettings(game) => {
                    let ini = GameIni::load(&game.ini());
                    self.editor = Some(IniEditor { game, ini, changed: false });
                }
                Action::AskCleanBuild(game) => self.confirm = Some(Confirm::CleanBuild(game)),
                Action::CleanBuild(game) => {
                    match std::fs::remove_dir_all(game.build_dir()) {
                        Ok(()) => self.notify(format!("Deleted the build cache of {}.", game.name), false),
                        Err(e) => self.notify(format!("Could not delete the build cache: {e}"), true),
                    }
                    self.rescan();
                }
                Action::Rescan => self.rescan(),
                Action::Quit => {
                    self.quitting = true;
                    if let Some(job) = &mut self.job {
                        job.cancel();
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.job.is_some() && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Confirm::Quit);
        }
    }

    fn handle_drop(&mut self, ctx: &egui::Context) {
        let dropped = ctx
            .input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).find(|p| !p.as_os_str().is_empty()));
        if let Some(p) = dropped {
            if self.job.is_none() {
                self.set_input(p);
                self.tab = Tab::Recompile;
            } else {
                self.notify("A recompile is running: wait for it to finish first.", true);
            }
        }
    }

    // ------------------------------------------------------------------ chrome

    fn top_bar(&mut self, ui: &mut Ui) {
        egui::Panel::top("top_bar")
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(Margin::symmetric(16, 10)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    logo(ui, 26.0);
                    ui.label(RichText::new("SwitchRecomp").size(19.0).strong().color(Color32::WHITE));
                    ui.add_space(18.0);
                    let library = if self.games.is_empty() {
                        "Library".to_string()
                    } else {
                        format!("Library  {}", self.games.len())
                    };
                    for (tab, label) in
                        [(Tab::Recompile, "Recompile"), (Tab::Library, library.as_str()), (Tab::About, "About")]
                    {
                        if tab_button(ui, self.tab == tab, label).clicked() {
                            if tab == Tab::Library {
                                self.actions.push(Action::Rescan);
                            }
                            self.tab = tab;
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let v = &self.versions;
                        let missing_gcc = format!("Not found. {}", toolchain::GCC_HINT);
                        chip(ui, "GCC", v.gcc.as_deref(), self.tc.gcc.as_deref(), &missing_gcc);
                        chip(ui, "TCC", v.tcc.as_deref(), self.tc.tcc.as_deref(), "Not found (tcc folder).");
                        let py = self.tc.python.as_ref().map(|p| p.program.as_path());
                        chip(ui, "Python", v.python.as_deref(), py, &format!("Not found. {}", toolchain::PYTHON_HINT));
                    });
                });
            });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let (text, color) = self.status_text();
        egui::Panel::bottom("status_bar")
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(Margin::symmetric(16, 6)))
            .show(ui, |ui| {
                // credit first, so a long path is cut short instead of running into it
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("SwitchRecomp by MZ Labs").small().color(theme::TEXT_WEAK));
                    ui.add_space(16.0);
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(Label::new(RichText::new(text).small().color(color)).truncate());
                    });
                });
            });
    }

    fn status_text(&self) -> (String, Color32) {
        if let Some((msg, error, at)) = &self.notice
            && at.elapsed() < Duration::from_secs(6)
        {
            return (msg.clone(), if *error { theme::ERROR } else { theme::SUCCESS });
        }
        if self.job.is_some() {
            return (format!("{}  ·  {}", self.progress.describe(), fmt_elapsed(self.elapsed)), theme::TEXT);
        }
        match self.out_dir() {
            Some(d) => (format!("Games folder: {}", d.display()), theme::TEXT_WEAK),
            None => (String::new(), theme::TEXT_WEAK),
        }
    }

    fn drop_overlay(&self, ctx: &egui::Context) {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        let rect = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("drop_overlay")));
        painter.rect_filled(rect, 0, Color32::from_black_alpha(200));
        painter.rect_stroke(rect.shrink(18.0), 14, Stroke::new(2.0, theme::ACCENT), egui::StrokeKind::Inside);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Drop a game folder, .nsp, .xci, .nca or .nro",
            egui::FontId::proportional(22.0),
            Color32::WHITE,
        );
    }

    fn unpack_screen(&mut self, ui: &mut Ui) {
        let Some(u) = &self.unpack else { return };
        let (fraction, error) = (u.fraction(), u.error.clone());
        let mut retry = false;
        egui::CentralPanel::default().frame(page_frame()).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.25).max(16.0));
                logo(ui, 56.0);
                ui.add_space(12.0);
                ui.label(RichText::new("Getting SwitchRecomp ready").size(22.0).strong().color(Color32::WHITE));
                ui.add_space(4.0);
                match &error {
                    None => {
                        ui.label(
                            RichText::new("Unpacking Python, TCC and GCC. This only happens on the first start.")
                                .color(theme::TEXT_WEAK),
                        );
                        ui.add_space(16.0);
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .desired_width(440.0)
                                .desired_height(10.0)
                                .fill(theme::ACCENT)
                                .corner_radius(5),
                        );
                        ui.label(RichText::new(format!("{:.0}%", fraction * 100.0)).small().color(theme::TEXT_WEAK));
                    }
                    Some(e) => {
                        ui.label(RichText::new(format!("Unpacking failed: {e}")).color(theme::ERROR));
                        ui.label(
                            RichText::new("Make sure the disk has about 500 MB free, then try again.")
                                .small()
                                .color(theme::TEXT_WEAK),
                        );
                        ui.add_space(8.0);
                        retry = ui.add(primary_button("Try again")).clicked();
                    }
                }
                if let Some(dir) = payload::home() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(dir.display().to_string()).small().color(theme::TEXT_WEAK));
                }
            });
        });
        if retry {
            self.unpack = Some(Unpack::start(ui.ctx()));
        }
    }

    fn missing_home(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().frame(page_frame()).show(ui, |ui| {
            card(ui, theme::ERROR, |ui| {
                ui.label(RichText::new("SwitchRecomp files not found").size(18.0).strong().color(theme::ERROR));
                ui.label(
                    "switchrecomp.py was not found next to this program. Keep it in the SwitchRecomp \
                     folder, together with switchrecomp.py and the folders that came with it.",
                );
            });
        });
    }

    // ------------------------------------------------------------------ recompile tab

    fn recompile_tab(&mut self, ui: &mut Ui) {
        egui::Panel::left("form")
            .resizable(false)
            .exact_size(FORM_WIDTH)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(Margin::same(18)))
            .show(ui, |ui| {
                egui::Panel::bottom("form_actions")
                    .show_separator_line(false)
                    .frame(egui::Frame::new().inner_margin(Margin { top: 12, ..Default::default() }))
                    .show(ui, |ui| self.action_button(ui));
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.add_enabled_ui(self.job.is_none(), |ui| self.form(ui));
                });
            });
        egui::CentralPanel::default().frame(page_frame()).show(ui, |ui| {
            self.run_card(ui);
            ui.add_space(14.0);
            self.log_view(ui);
        });
    }

    fn form(&mut self, ui: &mut Ui) {
        section(ui, "Game");
        ui.add(
            TextEdit::singleline(&mut self.settings.input)
                .hint_text("Folder, .nsp, .xci, .nca or .nro")
                .desired_width(f32::INFINITY),
        );
        let mut picked_recent = None;
        ui.horizontal(|ui| {
            if ui.button("🗁  Folder…").clicked() {
                self.actions.push(Action::PickInputFolder);
            }
            if ui.button("🗋  Open NSP / XCI / NCA / NRO…").clicked() {
                self.actions.push(Action::PickInputNro);
            }
            if ui.button("🔑  Select prod.keys…").clicked() {
                self.actions.push(Action::PickImport(1));
            }
            if !self.settings.recent.is_empty() {
                ui.menu_button("Recent  ⏷", |ui| {
                    for r in &self.settings.recent {
                        if ui.button(r).clicked() {
                            picked_recent = Some(r.clone());
                            ui.close();
                        }
                    }
                });
            }
        });
        if let Some(r) = picked_recent {
            self.set_input(PathBuf::from(r));
            self.refresh_inspection();
        }
        ui.add_space(4.0);
        self.input_status(ui);
        ui.label("hactool is included in release builds. Select your own prod.keys before importing a package.");
        if !self.settings.prod_keys.trim().is_empty() {
            ui.label(format!("prod.keys: {}", self.settings.prod_keys));
        }
        ui.collapsing("Advanced import options / title.keys", |ui| {
            ui.label(
                "Override the bundled hactool only if needed. Keys stay local. Updates/DLC need a merged base dump.",
            );
            for (which, label) in [
                (0, "hactool override (blank uses included tool)"),
                (1, "prod.keys"),
                (2, "title.keys (if required)"),
            ] {
                ui.label(label);
                ui.horizontal(|ui| {
                    let value = match which {
                        0 => &mut self.settings.hactool,
                        1 => &mut self.settings.prod_keys,
                        _ => &mut self.settings.title_keys,
                    };
                    ui.add(TextEdit::singleline(value).desired_width((ui.available_width() - 42.0).max(60.0)));
                    if ui.button("…").clicked() {
                        self.actions.push(Action::PickImport(which));
                    }
                });
            }
        });

        ui.add_space(14.0);
        section(ui, "Output");
        ui.label("Name");
        let hint = self.title();
        ui.add(TextEdit::singleline(&mut self.settings.name).hint_text(hint).desired_width(f32::INFINITY));
        ui.label("Folder");
        ui.horizontal(|ui| {
            let default = self.games_dir().map(|d| d.display().to_string()).unwrap_or_default();
            ui.add(
                TextEdit::singleline(&mut self.settings.out_dir)
                    .hint_text(default)
                    .desired_width(ui.available_width() - 44.0),
            );
            if ui.button("…").on_hover_text("Choose the output folder").clicked() {
                self.actions.push(Action::PickOutDir);
            }
        });
        if let Some(exe) = self.planned_exe() {
            ui.label(RichText::new(format!("Creates {}", exe.display())).small().color(theme::TEXT_WEAK));
        }

        ui.add_space(14.0);
        section(ui, "Compiler");
        let (tcc, gcc) = (self.tc.tcc.is_some(), self.tc.gcc.is_some());
        let selected = self.settings.compiler;
        let versions = self.versions.clone();
        ui.columns(2, |cols| {
            let lines = if tcc {
                ("Builds in seconds", "The game runs slower")
            } else {
                ("Not found", "tcc folder is missing")
            };
            if compiler_tile(&mut cols[0], selected == Compiler::Tcc, tcc, "TCC", lines, versions.tcc.as_deref())
                .on_hover_text("Tiny C Compiler, bundled. Good for a quick first test.")
                .clicked()
            {
                self.settings.compiler = Compiler::Tcc;
            }
            let lines = if gcc {
                ("Takes minutes", "The game runs much faster")
            } else {
                ("Not found", if cfg!(windows) { "Add it to the gcc folder" } else { "Install gcc" })
            };
            if compiler_tile(&mut cols[1], selected == Compiler::Gcc, gcc, "GCC", lines, versions.gcc.as_deref())
                .on_hover_text(if cfg!(windows) {
                    "MinGW-w64 GCC from the gcc folder or PATH. Recommended to actually play: \
                     the game runs several times faster than with TCC."
                } else {
                    "GCC from your system. Recommended to actually play: \
                     the game runs several times faster than with TCC."
                })
                .clicked()
            {
                self.settings.compiler = Compiler::Gcc;
            }
        });
        ui.add_space(4.0);
        if self.settings.compiler == Compiler::Gcc {
            ui.horizontal(|ui| {
                ui.label("Optimization");
                egui::ComboBox::from_id_salt("opt_level")
                    .selected_text(format!("-O{}", self.settings.opt_level))
                    .show_ui(ui, |ui| {
                        for (level, text) in
                            [(0, "-O0   fastest build, slow game"), (1, "-O1"), (2, "-O2   recommended"), (3, "-O3")]
                        {
                            ui.selectable_value(&mut self.settings.opt_level, level, text);
                        }
                    });
            });
            ui.checkbox(&mut self.settings.native, "Optimize for this processor")
                .on_hover_text("Adds -march=native. A little faster, but the game may not start on other PCs.");
        }
        ui.horizontal(|ui| {
            ui.label("Parallel jobs");
            let mut jobs = self.jobs();
            if ui.add(egui::Slider::new(&mut jobs, 1..=ncpu())).changed() {
                self.settings.jobs = jobs;
            }
        });
        egui::CollapsingHeader::new("Advanced").id_salt("advanced").show(ui, |ui| {
            ui.checkbox(&mut self.settings.interp_only, "Interpreter only (diagnostics)").on_hover_text(
                "Skips the translation: all game code runs in the slow fallback interpreter. \
                 Only useful to track down translation bugs.",
            );
        });
    }

    fn input_status(&mut self, ui: &mut Ui) {
        let empty = self.settings.input.trim().is_empty();
        match self.info() {
            None => {}
            Some(Err(_)) if empty => {
                ui.label(
                    RichText::new(
                        "Pick the folder you extracted with hactool (it holds exefs and romfs), or drop it here.",
                    )
                    .small()
                    .color(theme::TEXT_WEAK),
                );
            }
            Some(Err(msg)) => {
                let msg = msg.clone();
                card(ui, theme::ERROR, |ui| {
                    ui.label(RichText::new(format!("✖  {msg}")).color(theme::ERROR));
                });
            }
            Some(Ok(info)) => {
                let info = info.clone();
                card(ui, theme::BORDER, |ui| {
                    let title = match (&info.name, &info.kind) {
                        (Some(n), _) => n.clone(),
                        (None, game::Kind::Nro) => "Homebrew".into(),
                        (None, game::Kind::Folder { .. } | game::Kind::Package) => info.default_title.clone(),
                    };
                    ui.label(RichText::new(format!("✔  {title}")).strong().color(theme::SUCCESS));
                    let code: u64 = info.modules.iter().map(|m| m.1).sum();
                    let mut facts = vec![];
                    if let Some(id) = info.title_id {
                        facts.push(format!("Title ID {id:016X}"));
                    }
                    match &info.kind {
                        game::Kind::Package => facts.push("Package awaiting extraction and content validation".into()),
                        game::Kind::Nro => facts.push(format!("homebrew .nro, {}", library::human_size(code))),
                        game::Kind::Folder { romfs } => {
                            let n = info.modules.len();
                            facts.push(format!(
                                "{n} module{}, {}",
                                if n == 1 { "" } else { "s" },
                                library::human_size(code)
                            ));
                            if romfs.is_some() {
                                facts.push("romfs found".into());
                            }
                        }
                    }
                    ui.label(RichText::new(facts.join("  ·  ")).small().color(theme::TEXT_WEAK));
                    for w in &info.warnings {
                        ui.label(RichText::new(format!("⚠  {w}")).small().color(theme::WARNING));
                    }
                });
            }
        }
    }

    fn action_button(&mut self, ui: &mut Ui) {
        let size = Vec2::new(ui.available_width(), 44.0);
        if let Some(job) = &self.job {
            let label = if job.cancelled { "Stopping…" } else { "⏹  Cancel" };
            let button = egui::Button::new(RichText::new(label).size(16.0).color(Color32::WHITE))
                .fill(Color32::from_rgb(0x8a, 0x2c, 0x33))
                .corner_radius(9)
                .min_size(size);
            if ui.add_enabled(!job.cancelled, button).clicked() {
                self.actions.push(Action::Cancel);
            }
            return;
        }
        let blocker = self.blocker();
        let button = egui::Button::new(RichText::new("▶  Recompile").size(16.0).strong().color(Color32::WHITE))
            .fill(theme::ACCENT)
            .corner_radius(9)
            .min_size(size);
        if ui.add_enabled(blocker.is_none(), button).clicked() {
            self.actions.push(Action::Start);
        }
        if let Some(why) = blocker.filter(|_| !self.settings.input.trim().is_empty()) {
            ui.label(RichText::new(why).small().color(theme::TEXT_WEAK));
        }
    }

    fn run_card(&mut self, ui: &mut Ui) {
        if self.job.is_some() {
            card(ui, theme::BORDER, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(self.progress.describe()).strong().size(16.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(fmt_elapsed(self.elapsed)).monospace().color(theme::TEXT_WEAK));
                    });
                });
                ui.add(
                    egui::ProgressBar::new(self.progress.fraction())
                        .desired_height(8.0)
                        .fill(theme::ACCENT)
                        .corner_radius(4),
                );
            });
            return;
        }
        match &self.outcome {
            Some(Outcome::Success(exe)) => {
                let exe = exe.clone();
                let name = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let game = self.games.iter().find(|g| g.exe == exe).cloned();
                card(ui, theme::SUCCESS, |ui| {
                    ui.label(RichText::new(format!("✔  {name} is ready")).size(18.0).strong().color(theme::SUCCESS));
                    ui.label(
                        RichText::new(format!("Recompiled in {}  ·  {}", fmt_elapsed(self.elapsed), exe.display()))
                            .small()
                            .color(theme::TEXT_WEAK),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.add(primary_button("▶  Play")).clicked() {
                            self.actions.push(Action::Play(exe.clone()));
                        }
                        if let Some(game) = game
                            && ui.button("⚙  Game settings").clicked()
                        {
                            self.actions.push(Action::EditSettings(game));
                        }
                        if ui.button("🗁  Show in folder").clicked() {
                            self.actions.push(Action::Reveal(exe.clone()));
                        }
                    });
                });
            }
            Some(Outcome::Failed(code)) => {
                let code = code.map(|c| format!(" (exit code {c})")).unwrap_or_default();
                card(ui, theme::ERROR, |ui| {
                    ui.label(
                        RichText::new(format!("✖  Recompile failed{code}")).size(18.0).strong().color(theme::ERROR),
                    );
                    ui.label(
                        RichText::new(
                            "The output below shows what went wrong. Check that the game folder is complete \
                             and that the compiler folder was fully unzipped.",
                        )
                        .small()
                        .color(theme::TEXT_WEAK),
                    );
                });
            }
            Some(Outcome::Cancelled) => {
                card(ui, theme::BORDER, |ui| {
                    ui.label(RichText::new("Recompile cancelled").size(16.0).strong());
                    ui.label(
                        RichText::new("Files compiled so far are kept: the next run continues from there.")
                            .small()
                            .color(theme::TEXT_WEAK),
                    );
                });
            }
            None => welcome(ui),
        }
    }

    fn log_view(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Output").strong());
            if !self.log.is_empty() {
                ui.label(RichText::new(format!("{} lines", self.log.len())).small().color(theme::TEXT_WEAK));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled(!self.log.is_empty() && self.job.is_none(), egui::Button::new("Clear")).clicked() {
                    self.log.clear();
                }
                if ui.add_enabled(!self.log.is_empty(), egui::Button::new("📋  Copy")).clicked() {
                    let text = self.log.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
                    ui.ctx().copy_text(text);
                    self.notify("Output copied to the clipboard.", false);
                }
            });
        });
        egui::Frame::new()
            .fill(theme::LOG_BG)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(10)
            .inner_margin(Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                if self.log.is_empty() {
                    ui.label(RichText::new("The recompiler output appears here.").monospace().color(theme::TEXT_WEAK));
                    return;
                }
                ui.spacing_mut().item_spacing.y = 2.0;
                let row = ui.text_style_height(&TextStyle::Monospace);
                ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show_rows(
                    ui,
                    row,
                    self.log.len(),
                    |ui, range| {
                        for line in &self.log[range] {
                            let text = RichText::new(&line.text).monospace().color(line_color(line.kind));
                            ui.add(Label::new(text).wrap_mode(TextWrapMode::Extend));
                        }
                    },
                );
            });
    }

    // ------------------------------------------------------------------ library tab

    fn library_tab(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().frame(page_frame()).show(ui, |ui| {
            let out_dir = self.out_dir();
            ui.horizontal(|ui| {
                ui.heading("Library");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(dir) = &out_dir
                        && ui
                            .add_enabled(dir.is_dir(), egui::Button::new("🗁  Open folder"))
                            .on_disabled_hover_text("The folder is created by the first recompile.")
                            .clicked()
                    {
                        self.actions.push(Action::Open(dir.clone()));
                    }
                    if ui.button("🔄  Refresh").clicked() {
                        self.actions.push(Action::Rescan);
                    }
                });
            });
            if let Some(dir) = &out_dir {
                ui.label(RichText::new(dir.display().to_string()).small().color(theme::TEXT_WEAK));
            }
            ui.add_space(10.0);
            if self.games.is_empty() {
                card(ui, theme::BORDER, |ui| {
                    ui.label(RichText::new("No recompiled games yet").size(16.0).strong());
                    ui.label(
                        RichText::new("Games you recompile show up here, ready to play and configure.")
                            .color(theme::TEXT_WEAK),
                    );
                    ui.add_space(4.0);
                    if ui.add(primary_button("Recompile a game")).clicked() {
                        self.tab = Tab::Recompile;
                    }
                });
                return;
            }
            let (games, actions) = (&self.games, &mut self.actions);
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for game in games {
                    game_card(ui, game, actions);
                    ui.add_space(8.0);
                }
            });
        });
    }

    // ------------------------------------------------------------------ about tab

    fn about_tab(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().frame(page_frame()).show(ui, |ui| {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(760.0);
                ui.horizontal(|ui| {
                    logo(ui, 40.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("SwitchRecomp").size(24.0).strong().color(Color32::WHITE));
                        ui.label(
                            RichText::new(format!(
                                "Static recompiler for Nintendo Switch games  ·  {}",
                                env!("SWR_BUILD")
                            ))
                            .color(theme::TEXT_WEAK),
                        );
                    });
                });
                ui.add_space(10.0);
                ui.label(
                    "The game code (exefs) is translated from ARM64 to C once and compiled into a native \
                     program. The game data (romfs: models, textures, music) is not compiled: the game reads \
                     it as is from its _data folder.",
                );
                ui.label(
                    RichText::new("Use only your own copy of the game. No games, keys or firmware are included.")
                        .color(theme::WARNING),
                );

                ui.add_space(14.0);
                section(ui, "Controls in game");
                card(ui, theme::BORDER, |ui| {
                    egui::Grid::new("controls").num_columns(2).spacing([28.0, 6.0]).show(ui, |ui| {
                        for (what, keys) in [
                            ("Controller", "Xbox / XInput pads map by position, like the Switch controller"),
                            ("D-Pad", "Arrow keys"),
                            ("Left / right stick", "W A S D  /  I J K L"),
                            ("A  B  X  Y", "X or Space,  Z,  C,  V"),
                            ("L  R  ZL  ZR", "Q,  E,  1,  3"),
                            ("+  /  −", "Enter  /  Backspace"),
                            ("Function keys", "F1 help · F3 screenshot · F4 FPS limit · F5 diagnostic trace"),
                            ("", "F7 record video · F8 full-resolution PNG · F11 or Alt+Enter fullscreen"),
                            ("Quit", "Esc"),
                        ] {
                            ui.label(RichText::new(what).color(theme::TEXT_WEAK));
                            ui.label(keys);
                            ui.end_row();
                        }
                    });
                });

                ui.add_space(14.0);
                section(ui, "Tools");
                card(ui, theme::BORDER, |ui| {
                    let home = self.tc.home.as_ref().map(|h| h.display().to_string());
                    let data = self.data.as_ref().map(|d| d.display().to_string());
                    let python = self.tc.python.as_ref().map(|p| p.program.display().to_string());
                    let tcc = self.tc.tcc.as_ref().map(|p| p.display().to_string());
                    let gcc = self.tc.gcc.as_ref().map(|p| p.display().to_string());
                    egui::Grid::new("tools").num_columns(2).spacing([28.0, 6.0]).show(ui, |ui| {
                        for (what, path, version) in [
                            ("Tools", home, None),
                            ("Games, settings", data, None),
                            ("Python", python, self.versions.python.clone()),
                            ("TCC", tcc, self.versions.tcc.clone()),
                            ("GCC", gcc, self.versions.gcc.clone()),
                        ] {
                            ui.label(RichText::new(what).color(theme::TEXT_WEAK));
                            match path {
                                Some(p) => ui.label(match version {
                                    Some(v) => format!("{v}  ·  {p}"),
                                    None => p,
                                }),
                                None => ui.label(RichText::new("not found").color(theme::ERROR)),
                            };
                            ui.end_row();
                        }
                    });
                });

                ui.add_space(14.0);
                section(ui, "Credits");
                card(ui, theme::BORDER, |ui| {
                    ui.label(RichText::new("SwitchRecomp by MZ Labs").strong().color(Color32::WHITE));
                    ui.label(
                        "Licensed under the Apache License 2.0. Copies, forks and builds made with it must keep \
                         the NOTICE file and credit \"SwitchRecomp by MZ Labs\".",
                    );
                    ui.hyperlink_to("Discord", DISCORD);
                    ui.add_space(6.0);
                    ui.label(RichText::new("Third-party components").color(theme::TEXT_WEAK));
                    for line in [
                        "OpenH264 (Cisco, BSD 2-clause): game videos",
                        "SPIRV-Cross (Khronos, Apache 2.0): frame generation shaders",
                        "texture2ddecoder (MIT): BCn textures",
                        "TCC (LGPL), Python (PSF), GCC / MinGW-w64 (GPL with the runtime exception)",
                        "egui / eframe (MIT or Apache 2.0): this window",
                    ] {
                        ui.label(RichText::new(format!("•  {line}")).small());
                    }
                });
            });
        });
    }

    // ------------------------------------------------------------------ dialogs

    fn modals(&mut self, ctx: &egui::Context) {
        if let Some(confirm) = &self.confirm {
            let (title, body, ok) = match confirm {
                Confirm::Quit => (
                    "Quit SwitchRecomp?".to_string(),
                    "A recompile is running. Quitting stops it; files compiled so far are kept.".to_string(),
                    "Stop and quit",
                ),
                Confirm::CleanBuild(g) => (
                    format!("Delete the build cache of {}?", g.name),
                    format!(
                        "This frees {}. The cache only speeds up the next recompile of this game; \
                         the game itself, its settings and saves are not touched.",
                        library::human_size(g.build_cache)
                    ),
                    "Delete",
                ),
            };
            let mut answer = None;
            let modal = egui::Modal::new(Id::new("confirm")).show(ctx, |ui| {
                ui.set_width(400.0);
                ui.label(RichText::new(title).size(17.0).strong());
                ui.add_space(4.0);
                ui.label(body);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let danger = egui::Button::new(RichText::new(ok).color(Color32::WHITE))
                        .fill(Color32::from_rgb(0x8a, 0x2c, 0x33))
                        .corner_radius(7);
                    if ui.add(danger).clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                });
            });
            if answer.is_none() && modal.should_close() {
                answer = Some(false);
            }
            match answer {
                Some(true) => match self.confirm.take() {
                    Some(Confirm::Quit) => self.actions.push(Action::Quit),
                    Some(Confirm::CleanBuild(g)) => self.actions.push(Action::CleanBuild(g)),
                    None => {}
                },
                Some(false) => self.confirm = None,
                None => {}
            }
        }
        self.settings_editor(ctx);
    }

    fn settings_editor(&mut self, ctx: &egui::Context) {
        let Some(ed) = &mut self.editor else { return };
        let (mut save, mut close, mut pick) = (false, false, None);
        let max_height = (ctx.content_rect().height() - 230.0).max(200.0);
        let modal = egui::Modal::new(Id::new("game_settings")).show(ctx, |ui| {
            ui.set_width(640.0);
            ui.label(RichText::new(format!("{}  ·  settings", ed.game.name)).size(18.0).strong());
            ui.label(
                RichText::new(
                    "Most settings apply live while the game runs. TV mode, language, fullscreen and frame \
                     generation apply on the next start.",
                )
                .small()
                .color(theme::TEXT_WEAK),
            );
            ui.add_space(6.0);
            ScrollArea::vertical().max_height(max_height).auto_shrink([false, true]).show(ui, |ui| {
                for section in ini::SECTIONS.iter().filter(|s| cfg!(windows) || !s.windows_only) {
                    ui.add_space(6.0);
                    ui.label(RichText::new(section.title).strong().color(theme::ACCENT_HOVER));
                    egui::Grid::new(section.title).num_columns(2).spacing([24.0, 8.0]).min_col_width(240.0).show(
                        ui,
                        |ui| {
                            for opt in section.opts {
                                ui.label(opt.label).on_hover_text(opt.help.replace("\n;   ", " "));
                                let mut value = ed.ini.value(opt);
                                if option_widget(ui, opt, &mut value, &mut pick) {
                                    ed.ini.set(opt.key, &value);
                                    ed.changed = true;
                                }
                                ui.end_row();
                            }
                        },
                    );
                }
                let others: Vec<(String, String)> = ed.ini.others().cloned().collect();
                if !others.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Other").strong().color(theme::ACCENT_HOVER));
                    egui::Grid::new("other_keys").num_columns(2).spacing([24.0, 8.0]).min_col_width(240.0).show(
                        ui,
                        |ui| {
                            for (key, mut value) in others {
                                ui.label(&key);
                                if ui.add(TextEdit::singleline(&mut value).desired_width(260.0)).changed() {
                                    ed.ini.set(&key, &value);
                                    ed.changed = true;
                                }
                                ui.end_row();
                            }
                        },
                    );
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Reset to defaults").clicked() {
                    ed.ini.reset();
                    ed.changed = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add_enabled(ed.changed, primary_button("Save")).clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        });
        // clicking next to the dialog shouldn't throw edits away
        if modal.should_close() && !ed.changed {
            close = true;
        }
        if let Some((key, filter)) = pick {
            self.actions.push(Action::PickIniFile(key, filter));
        }
        if save {
            let (result, name) = (ed.ini.save(&ed.game.name), ed.game.name.clone());
            match result {
                Ok(()) => {
                    self.editor = None;
                    self.notify(format!("Saved the settings of {name}."), false);
                }
                Err(e) => self.notify(format!("Could not save the settings: {e}"), true),
            }
        } else if close {
            self.editor = None;
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_unpack(ctx);
        self.poll_job();
        if let Some(rx) = &self.versions_rx
            && let Ok(v) = rx.try_recv()
        {
            self.versions = v;
            self.versions_rx = None;
        }
        if self.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        if self.settings != self.saved {
            self.save_settings(false);
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        if self.notice.as_ref().is_some_and(|(_, _, at)| at.elapsed() < Duration::from_secs(7)) {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.unpack.is_some() {
            self.status_bar(ui);
            self.unpack_screen(ui);
            return;
        }
        self.handle_close(&ctx);
        self.handle_drop(&ctx);
        self.refresh_inspection();

        self.top_bar(ui);
        self.status_bar(ui);
        if self.tc.home.is_none() {
            self.missing_home(ui);
        } else {
            match self.tab {
                Tab::Recompile => self.recompile_tab(ui),
                Tab::Library => self.library_tab(ui),
                Tab::About => self.about_tab(ui),
            }
        }
        self.modals(&ctx);
        self.drop_overlay(&ctx);
        self.run_actions(&ctx, frame);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(job) = &mut self.job {
            job.cancel();
        }
        self.save_settings(true);
    }
}

// ---------------------------------------------------------------------- widgets

/// Games and settings sit next to the single-file exe, like they did in the
/// release folder; the local app data folder is the fallback when that folder
/// is read-only.
fn portable_dir() -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    if let Some(dir) = exe_dir.filter(|d| writable(d)) {
        return dir;
    }
    let dir = payload::cache_root().map_or_else(|| PathBuf::from("."), |r| r.join("data"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(".switchrecomp-write-test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

fn ncpu() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

fn dialog(frame: &eframe::Frame, dir: Option<&Path>) -> rfd::FileDialog {
    let d = rfd::FileDialog::new().set_parent(frame);
    match dir.filter(|d| d.is_dir()) {
        Some(dir) => d.set_directory(dir),
        None => d,
    }
}

fn fmt_elapsed(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn page_frame() -> egui::Frame {
    egui::Frame::new().fill(theme::BG).inner_margin(Margin::same(18))
}

fn card<R>(ui: &mut Ui, border: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::CARD)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(title.to_uppercase()).small().strong().color(theme::TEXT_WEAK));
}

fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).strong().color(Color32::WHITE)).fill(theme::ACCENT).corner_radius(7)
}

fn tab_button(ui: &mut Ui, active: bool, label: &str) -> egui::Response {
    let text = RichText::new(label).color(if active { Color32::WHITE } else { theme::TEXT_WEAK });
    ui.add(
        egui::Button::new(text).fill(if active { theme::ACCENT_SOFT } else { Color32::TRANSPARENT }).corner_radius(7),
    )
}

fn logo(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same((size * 0.24) as u8), theme::ACCENT);
    let (c, s) = (rect.center(), size * 0.2);
    let tri = vec![
        egui::pos2(c.x - s * 0.75, c.y - s * 1.1),
        egui::pos2(c.x + s * 1.15, c.y),
        egui::pos2(c.x - s * 0.75, c.y + s * 1.1),
    ];
    p.add(egui::Shape::convex_polygon(tri, Color32::WHITE, Stroke::NONE));
}

/// Tool status in the top bar: green dot when found, with path and version.
fn chip(ui: &mut Ui, name: &str, version: Option<&str>, path: Option<&Path>, missing: &str) {
    let found = path.is_some();
    let text = match version {
        Some(v) if found => format!("{name} {v}"),
        _ => name.to_string(),
    };
    let tip = match path {
        Some(p) => p.display().to_string(),
        None => missing.to_string(),
    };
    egui::Frame::new()
        .fill(theme::CARD)
        .corner_radius(12)
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 3.5, if found { theme::SUCCESS } else { theme::ERROR });
                ui.label(RichText::new(text).small().color(if found { theme::TEXT } else { theme::TEXT_WEAK }));
            });
        })
        .response
        .on_hover_text(tip);
}

fn compiler_tile(
    ui: &mut Ui,
    selected: bool,
    available: bool,
    title: &str,
    lines: (&str, &str),
    version: Option<&str>,
) -> egui::Response {
    let (fill, stroke) = if selected {
        (theme::ACCENT_SOFT, Stroke::new(1.5, theme::ACCENT))
    } else {
        (theme::CARD, Stroke::new(1.0, theme::BORDER))
    };
    let inner =
        egui::Frame::new().fill(fill).stroke(stroke).corner_radius(10).inner_margin(Margin::symmetric(12, 10)).show(
            ui,
            |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                    let ring = if selected { theme::ACCENT } else { theme::TEXT_WEAK };
                    ui.painter().circle_stroke(r.center(), 6.0, Stroke::new(1.5, ring));
                    if selected {
                        ui.painter().circle_filled(r.center(), 3.0, theme::ACCENT_HOVER);
                    }
                    let color = if available { Color32::WHITE } else { theme::TEXT_WEAK };
                    ui.label(RichText::new(title).size(16.0).strong().color(color));
                    if let Some(v) = version {
                        ui.label(RichText::new(v).small().color(theme::TEXT_WEAK));
                    }
                });
                let color = if available { theme::TEXT } else { theme::ERROR };
                ui.label(RichText::new(lines.0).small().color(color));
                ui.label(RichText::new(lines.1).small().color(theme::TEXT_WEAK));
            },
        );
    let sense = if available { Sense::click() } else { Sense::hover() };
    let response = ui.interact(inner.response.rect, ui.id().with(title), sense);
    if available && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

fn welcome(ui: &mut Ui) {
    card(ui, theme::BORDER, |ui| {
        ui.label(RichText::new("Recompile a game").size(18.0).strong().color(Color32::WHITE));
        ui.add_space(2.0);
        let steps = [
            "Dump the game from your own console and extract it with hactool and your own keys:",
            "Pick the folder that contains exefs and romfs, or drop it on this window.",
            "Choose TCC to build in seconds, or GCC for a game that runs much faster.",
            "Press Recompile. The game lands in the Library, ready to play.",
        ];
        for (i, step) in steps.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}.", i + 1)).strong().color(theme::ACCENT_HOVER));
                ui.label(*step);
            });
            if i == 0 {
                egui::Frame::new().fill(theme::LOG_BG).corner_radius(6).inner_margin(Margin::symmetric(10, 6)).show(
                    ui,
                    |ui| {
                        ui.label(
                            RichText::new("hactool -k prod.keys --exefsdir=exefs --romfsdir=romfs game.nca")
                                .monospace(),
                        );
                    },
                );
            }
        }
    });
}

fn line_color(kind: LineKind) -> Color32 {
    match kind {
        LineKind::Command => theme::ACCENT_HOVER,
        LineKind::Info => theme::TEXT,
        LineKind::Detail => theme::TEXT_WEAK,
        LineKind::Warning => theme::WARNING,
        LineKind::Error => theme::ERROR,
        LineKind::Success => theme::SUCCESS,
    }
}

fn game_card(ui: &mut Ui, game: &Game, actions: &mut Vec<Action>) {
    card(ui, theme::BORDER, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&game.name).size(17.0).strong().color(Color32::WHITE));
                    let color = match game.compiler {
                        CodeCompiler::Gcc => theme::SUCCESS,
                        CodeCompiler::Tcc => theme::WARNING,
                        CodeCompiler::Unknown => theme::TEXT_WEAK,
                    };
                    badge(ui, game.compiler.label(), color);
                });
                let mut facts = vec![format!("code {}", library::human_size(game.code_size))];
                if let Some(t) = game.built {
                    facts.push(format!("built {}", library::time_ago(t)));
                }
                if game.build_cache > 0 {
                    facts.push(format!("build cache {}", library::human_size(game.build_cache)));
                }
                ui.label(RichText::new(facts.join("  ·  ")).small().color(theme::TEXT_WEAK));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button("⋯", |ui| {
                    let log = game.log();
                    if ui.add_enabled(log.is_file(), egui::Button::new("📄  Open log")).clicked() {
                        actions.push(Action::Open(log));
                        ui.close();
                    }
                    if ui.button("🗁  Open data folder").clicked() {
                        actions.push(Action::Open(game.data_dir()));
                        ui.close();
                    }
                    ui.separator();
                    let label = format!("🗑  Delete build cache ({})", library::human_size(game.build_cache));
                    if ui.add_enabled(game.build_cache > 0, egui::Button::new(label)).clicked() {
                        actions.push(Action::AskCleanBuild(game.clone()));
                        ui.close();
                    }
                });
                if ui.button("🗁").on_hover_text("Show in folder").clicked() {
                    actions.push(Action::Reveal(game.exe.clone()));
                }
                if ui.button("⚙  Settings").clicked() {
                    actions.push(Action::EditSettings(game.clone()));
                }
                if ui.add(primary_button("▶  Play")).clicked() {
                    actions.push(Action::Play(game.exe.clone()));
                }
            });
        });
        if game.compiler == CodeCompiler::Tcc {
            ui.label(
                RichText::new("Built with TCC: recompile it with GCC to make the game run several times faster.")
                    .small()
                    .color(theme::WARNING),
            );
        }
    });
}

fn badge(ui: &mut Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.18))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.6)))
        .corner_radius(5)
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().strong().color(color));
        });
}

/// Draws the right widget for one .ini option; true when the value changed.
fn option_widget(ui: &mut Ui, opt: &Opt, value: &mut String, pick: &mut Option<(&'static str, FileFilter)>) -> bool {
    match &opt.kind {
        Kind::Bool => {
            let mut on = ini::as_bool(value);
            let changed = toggle(ui, &mut on).changed();
            if changed {
                *value = if on { "1" } else { "0" }.into();
            }
            changed
        }
        Kind::Choice(choices) => {
            let current = choices
                .iter()
                .find(|(v, _)| *v == value.trim())
                .map_or_else(|| format!("Custom ({value})"), |(_, l)| l.to_string());
            let mut changed = false;
            egui::ComboBox::from_id_salt(opt.key).selected_text(current).width(240.0).show_ui(ui, |ui| {
                for (v, label) in *choices {
                    if ui.selectable_label(value.trim() == *v, *label).clicked() {
                        *value = v.to_string();
                        changed = true;
                    }
                }
            });
            changed
        }
        Kind::Float { min, max, step } => {
            let mut f: f32 = value.trim().parse().unwrap_or_else(|_| opt.default.parse().unwrap_or(*min));
            let changed =
                ui.add(egui::DragValue::new(&mut f).range(*min..=*max).speed(*step).fixed_decimals(2)).changed();
            if changed {
                *value = ini::format_float(f);
            }
            changed
        }
        Kind::Int { min, max } => {
            let mut n: i32 = value.trim().parse().unwrap_or_else(|_| opt.default.parse().unwrap_or(*min));
            let changed = ui.add(egui::DragValue::new(&mut n).range(*min..=*max)).changed();
            if changed {
                *value = n.to_string();
            }
            changed
        }
        Kind::File { filter } => {
            ui.horizontal(|ui| {
                let changed = ui.add(TextEdit::singleline(value).hint_text("automatic").desired_width(250.0)).changed();
                if ui.button("…").clicked() {
                    *pick = Some((opt.key, *filter));
                }
                changed
            })
            .inner
        }
    }
}

/// iOS-style switch, after the egui demo.
fn toggle(ui: &mut Ui, on: &mut bool) -> egui::Response {
    let size = ui.spacing().interact_size.y * egui::vec2(1.7, 0.8);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, ""));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let radius = 0.5 * rect.height();
        let fill = theme::CARD_HOVER.lerp_to_gamma(theme::ACCENT, t);
        ui.painter().rect_filled(rect, CornerRadius::same(radius as u8), fill);
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
        ui.painter().circle_filled(egui::pos2(x, rect.center().y), 0.75 * radius, Color32::WHITE);
    }
    response
}
