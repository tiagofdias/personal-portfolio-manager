//! Native immediate-mode UI (egui). Repaints only on input or AI tokens, so
//! the app idles at ~0% CPU. Visual language lives in `theme.rs`.

use crate::ai::{self, AiEvent, AiJob, ChatMessage, Role};
use crate::analysis::{self, Report, Severity};
use crate::assets::{AssetClass, AssetDb, Vehicle};
use crate::portfolio::{self, Holding, Portfolio};
use crate::profile::{QUESTIONS, RiskLevel};
use crate::storage::{self, State};
use crate::theme::{self as t, Icon};
use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, TextFormat,
    text::LayoutJob, vec2,
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Portfolio,
    Profile,
    Analysis,
    Advisor,
    Settings,
}

pub struct App {
    db: AssetDb,
    st: State,
    tab: Tab,
    report: Option<Report>,
    report_dirty: bool,
    understood: Vec<(String, f32)>,
    parse_msg: String,
    new_ticker: String,
    chat_input: String,
    job: Option<AiJob>,
    streaming: String,
    ai_error: Option<String>,
    ai_ok: Option<bool>,
    models: Option<Result<Vec<String>, String>>,
    models_rx: Option<std::sync::mpsc::Receiver<Result<Vec<String>, String>>>,
    last_change: Option<Instant>,
}

const SAT_COLORS: [Color32; 4] = [
    t::WARN,
    t::CRIT,
    Color32::from_rgb(0xE3, 0x9B, 0xD2),
    Color32::from_rgb(0x5B, 0xD6, 0xC4),
];
const REGION_COLORS: [Color32; 5] = [
    t::ACCENT,
    Color32::from_rgb(0x5A, 0x6F, 0xB8),
    Color32::from_rgb(0x43, 0x52, 0x86),
    Color32::from_rgb(0x34, 0x40, 0x68),
    Color32::from_rgb(0x2A, 0x33, 0x52),
];

fn sev_style(s: Severity) -> (&'static str, Color32, Color32) {
    match s {
        Severity::Critical => ("Critical", t::CRIT_TEXT, t::CRIT_BG),
        Severity::Warning => ("Warning", t::WARN_TEXT, t::WARN_BG),
        Severity::Info => ("Info", t::INFO_TEXT, t::INFO_BG),
        Severity::Good => ("Good", t::GOOD, t::GOOD_BG),
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        t::install(&cc.egui_ctx);
        let db = AssetDb::load(Some(&storage::data_dir().join("assets.json")));
        let st = storage::load();
        let tab = if st.portfolio.is_empty() { Tab::Portfolio } else { Tab::Analysis };
        Self {
            db,
            st,
            tab,
            report: None,
            report_dirty: true,
            understood: Vec::new(),
            parse_msg: String::new(),
            new_ticker: String::new(),
            chat_input: String::new(),
            job: None,
            streaming: String::new(),
            ai_error: None,
            ai_ok: None,
            models: None,
            models_rx: None,
            last_change: None,
        }
    }

    fn changed(&mut self) {
        self.report_dirty = true;
        self.last_change = Some(Instant::now());
    }

    fn report(&mut self) -> &Report {
        if self.report_dirty || self.report.is_none() {
            let lvl = self.st.profile.risk_level();
            self.report = Some(analysis::analyse(&self.st.portfolio, &self.db, lvl));
            self.report_dirty = false;
        }
        self.report.as_ref().unwrap()
    }

    fn system_prompt(&mut self) -> String {
        self.report();
        let facts = analysis::to_text(self.report.as_ref().unwrap(), &self.st.portfolio);
        ai::build_system(&self.st.profile, &facts)
    }

    fn send(&mut self, ctx: &egui::Context, text: String) {
        if text.trim().is_empty() || self.job.is_some() {
            return;
        }
        // Don't stack a duplicate of a question that failed to send.
        if self.ai_error.is_some() && self.st.chat.last().is_some_and(|m| m.role == Role::User && m.content == text) {
            self.st.chat.pop();
        }
        self.st.chat.push(ChatMessage { role: Role::User, content: text });
        let system = self.system_prompt();
        let c = ctx.clone();
        self.job = Some(ai::start_chat(&self.st.ai, system, &self.st.chat, move || c.request_repaint()));
        self.streaming.clear();
        self.ai_error = None;
        self.tab = Tab::Advisor;
        self.changed();
    }

    fn poll_ai(&mut self) {
        let Some(job) = &self.job else { return };
        let mut finished = false;
        while let Ok(ev) = job.rx.try_recv() {
            match ev {
                AiEvent::Token(tk) => self.streaming.push_str(&tk),
                AiEvent::Done => {
                    self.ai_ok = Some(true);
                    finished = true;
                }
                AiEvent::Error(e) => {
                    self.ai_error = Some(e);
                    self.ai_ok = Some(false);
                    finished = true;
                }
            }
        }
        if finished {
            self.job = None;
            let text = ai::strip_thinking(&std::mem::take(&mut self.streaming));
            if !text.trim().is_empty() {
                self.learn_notes(&text);
                self.st.chat.push(ChatMessage { role: Role::Assistant, content: text });
            }
            self.last_change = Some(Instant::now());
        }
    }

    fn learn_notes(&mut self, text: &str) {
        for note in notes_in(text) {
            if !self.st.profile.notes.contains(&note) {
                if !self.st.profile.notes.trim().is_empty() {
                    self.st.profile.notes.push_str("; ");
                }
                self.st.profile.notes.push_str(&note);
            }
        }
    }

    fn apply_portfolio(&mut self, p: Portfolio) {
        self.st.portfolio = p;
        self.st.portfolio.sort();
        self.changed();
    }

    fn autosave(&mut self, ctx: &egui::Context) {
        if let Some(tm) = self.last_change {
            let quiet = Duration::from_millis(800);
            if tm.elapsed() >= quiet && !ctx.input(|i| i.pointer.any_down()) {
                if let Err(e) = storage::save(&self.st) {
                    eprintln!("save failed: {e}");
                }
                self.last_change = None;
            } else {
                ctx.request_repaint_after(quiet);
            }
        }
    }

    // ----------------------------------------------------- asset styling --

    fn kind(&self, ticker: &str) -> (&'static str, Color32, Color32) {
        match self.db.get(ticker) {
            None => ("Unknown", t::MUTED, t::RAISED),
            Some(a) => match (a.class, a.vehicle) {
                (AssetClass::Equity, Vehicle::ETF) if analysis::is_broad_equity_fund(a) => ("Global fund", t::INFO_TEXT, t::INFO_BG),
                (AssetClass::Equity, Vehicle::ETF) => ("Sector ETF", t::WARN_TEXT, t::WARN_BG),
                (AssetClass::Equity, _) => ("Single stock", t::CRIT_TEXT, t::CRIT_BG),
                (AssetClass::Bonds, _) => ("Bonds", t::GOOD, t::GOOD_BG),
                (AssetClass::Cash, _) => ("Cash", t::NAV_TEXT, t::RAISED),
                (AssetClass::Commodity, _) => ("Commodity", t::WARN_TEXT, t::WARN_BG),
                (AssetClass::Crypto, _) => ("Crypto", Color32::from_rgb(0xD4, 0xB8, 0xFF), Color32::from_rgb(0x2C, 0x22, 0x42)),
                (AssetClass::RealEstate, _) => ("Real estate", t::INFO_TEXT, t::INFO_BG),
            },
        }
    }

    /// Stable color per holding role, shared by every allocation bar.
    fn color_of(&self, ticker: &str, sat_index: &mut usize) -> Color32 {
        match self.db.get(ticker) {
            Some(a) if a.class == AssetClass::Bonds => t::GOOD,
            Some(a) if a.class == AssetClass::Cash => t::FAINT,
            Some(a) if a.class == AssetClass::Commodity => Color32::from_rgb(0xD9, 0xC2, 0x7A),
            Some(a) if analysis::is_broad_equity_fund(a) => t::ACCENT,
            _ => {
                let c = SAT_COLORS[*sat_index % SAT_COLORS.len()];
                *sat_index += 1;
                c
            }
        }
    }

    fn colors_for(&self, tickers: &[&str]) -> Vec<Color32> {
        // Satellites get colors in a fixed ticker order so "Now" and
        // "Suggested" bars agree.
        let mut sats: Vec<&str> = tickers.iter().copied().filter(|tk| analysis::is_satellite(self.db.get(tk))).collect();
        sats.sort();
        tickers
            .iter()
            .map(|tk| {
                let mut i = sats.iter().position(|s| s == tk).unwrap_or(0);
                self.color_of(tk, &mut i)
            })
            .collect()
    }

    fn allocation_bar(&self, ui: &mut egui::Ui, p: &Portfolio, height: f32) {
        let tickers: Vec<&str> = p.holdings.iter().map(|h| h.ticker.as_str()).collect();
        let colors = self.colors_for(&tickers);
        let segs: Vec<(f32, Color32)> = p.holdings.iter().zip(colors).map(|(h, c)| (h.percent / 100.0, c)).collect();
        let w = ui.available_width();
        t::stacked_bar(ui, w, height, &segs);
    }

    fn allocation_legend(&self, ui: &mut egui::Ui, p: &Portfolio) {
        let tickers: Vec<&str> = p.holdings.iter().map(|h| h.ticker.as_str()).collect();
        let colors = self.colors_for(&tickers);
        let w = ui.available_width();
        let cols = if w > 420.0 { 2 } else { 1 };
        let cw = (w - 20.0 * (cols as f32 - 1.0)) / cols as f32;
        for row in p.holdings.iter().zip(colors).collect::<Vec<_>>().chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 20.0;
                for (h, c) in row {
                    let role = match self.kind(&h.ticker).0 {
                        "Global fund" => " · global core",
                        "Bonds" => " · bonds",
                        _ => "",
                    };
                    t::legend_row(ui, cw, *c, &format!("{}{role}", h.ticker), &fmt_pct(h.percent));
                }
            });
        }
    }
}

fn fmt_pct(x: f32) -> String {
    if (x - x.round()).abs() < 0.05 { format!("{:.0}%", x) } else { format!("{:.1}%", x) }
}

fn notes_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(s) = rest.find("<note>") {
        let after = &rest[s + 6..];
        let Some(e) = after.find("</note>") else { break };
        let n = after[..e].trim();
        if !n.is_empty() {
            out.push(n.to_string());
        }
        rest = &after[e..];
    }
    out
}

/// Remove machine tags the user shouldn't read (shown as cards instead).
fn display_text(s: &str) -> String {
    let mut out = s.to_string();
    for (open, close) in [("<note>", "</note>"), ("<portfolio>", "</portfolio>")] {
        while let Some(i) = out.find(open) {
            match out[i..].find(close) {
                Some(j) => out.replace_range(i..i + j + close.len(), ""),
                None => out.truncate(i),
            }
        }
    }
    strip_emoji(out.trim())
}

/// egui's fonts have no emoji: turn keycaps (1️⃣) into "1." and drop the rest.
fn strip_emoji(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() && matches!(chars.get(i + 1), Some('\u{FE0F}' | '\u{20E3}')) {
            out.push(c);
            out.push('.');
            i += 1;
            while matches!(chars.get(i), Some('\u{FE0F}' | '\u{20E3}')) {
                i += 1;
            }
            continue;
        }
        let u = c as u32;
        let emoji = (0x1F000..=0x1FAFF).contains(&u)
            || (0x2600..=0x27BF).contains(&u)
            || (0x2B00..=0x2BFF).contains(&u)
            || matches!(u, 0xFE0F | 0x20E3 | 0x200D);
        if !emoji {
            out.push(c);
        }
        i += 1;
    }
    // Collapse the double spaces left behind.
    out.lines().map(|l| l.trim_end().replace("  ", " ")).collect::<Vec<_>>().join("\n")
}

/// Minimal markdown: headings, bullets, numbered lists and **bold**.
fn markdown(ui: &mut egui::Ui, text: &str) {
    let job = |line: &str, color: Color32, size: f32| {
        let mut j = LayoutJob::default();
        for (i, part) in line.split("**").enumerate() {
            if part.is_empty() {
                continue;
            }
            let bold = i % 2 == 1;
            j.append(
                part,
                0.0,
                TextFormat {
                    font_id: if bold { t::semibold(size) } else { t::regular(size) },
                    color: if bold { t::TEXT } else { color },
                    ..Default::default()
                },
            );
        }
        j
    };
    for raw in text.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            ui.add_space(4.0);
            continue;
        }
        if let Some(h) = trimmed.strip_prefix('#') {
            ui.label(RichText::new(h.trim_start_matches('#').trim()).font(t::semibold(15.5)));
            continue;
        }
        let bullet = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .map(|b| ("•".to_string(), b))
            .or_else(|| {
                let d: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
                (!d.is_empty())
                    .then(|| trimmed[d.len()..].strip_prefix(". ").map(|b| (format!("{d}."), b)))
                    .flatten()
            });
        match bullet {
            Some((mark, body)) => {
                ui.horizontal_top(|ui| {
                    ui.add_sized([18.0, 18.0], egui::Label::new(RichText::new(mark).color(t::FAINT)));
                    ui.add(egui::Label::new(job(body, t::TEXT_SOFT, 14.5)).wrap());
                });
            }
            None => {
                ui.add(egui::Label::new(job(trimmed, t::TEXT, 14.5)).wrap());
            }
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_ai();
        if let Some(rx) = &self.models_rx {
            if let Ok(r) = rx.try_recv() {
                self.ai_ok = Some(r.is_ok());
                self.models = Some(r);
                self.models_rx = None;
            }
        }
        self.autosave(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.last_change.is_some() {
            let _ = storage::save(&self.st);
        }
    }

    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;

        egui::Panel::left("nav")
            .resizable(false)
            .exact_size(232.0)
            .frame(Frame::new().fill(t::SIDEBAR).inner_margin(Margin { left: 16, right: 16, top: 24, bottom: 20 }))
            .show(root, |ui| self.ui_nav(ui));

        egui::CentralPanel::default().frame(Frame::new().fill(t::BG)).show(root, |ui| {
            if self.tab == Tab::Advisor {
                self.ui_advisor(ui, ctx);
                return;
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                Frame::new().inner_margin(Margin { left: 40, right: 40, top: 32, bottom: 48 }).show(ui, |ui| {
                    let mw = ui.available_width().min(1200.0);
                    ui.set_max_width(mw);
                    match self.tab {
                        Tab::Portfolio => self.ui_portfolio(ui),
                        Tab::Profile => self.ui_profile(ui),
                        Tab::Analysis => self.ui_analysis(ui, ctx),
                        Tab::Settings => self.ui_settings(ui, ctx),
                        Tab::Advisor => {}
                    }
                });
            });
        });
    }
}

// ------------------------------------------------------------- screens --

impl App {
    fn ui_nav(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
            ui.painter().rect_filled(rect, 9, t::ACCENT);
            t::paint_icon(ui.painter(), Icon::Logo, rect.center(), 18.0, t::BG);
            ui.label(RichText::new("Portfolio\nManager").font(t::semibold(15.0)).line_height(Some(18.0)));
        });
        ui.add_space(20.0);
        for (tab, ic, label) in [
            (Tab::Portfolio, Icon::Grid, "My portfolio"),
            (Tab::Profile, Icon::User, "Investor profile"),
            (Tab::Analysis, Icon::Chart, "Analysis"),
            (Tab::Advisor, Icon::Chat, "AI advisor"),
            (Tab::Settings, Icon::Gear, "Settings"),
        ] {
            if t::nav_item(ui, self.tab == tab, ic, label).clicked() {
                self.tab = tab;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.label(RichText::new("Education, not financial advice.").font(t::regular(12.0)).color(t::FAINT));
            ui.add_space(4.0);
            match self.st.profile.risk_level() {
                Some(l) => ui.label(RichText::new(format!("{} · {} of 5", l.label(), l.level())).font(t::semibold(14.0))),
                None => ui.label(RichText::new("Not set yet").font(t::semibold(14.0)).color(t::WARN_TEXT)),
            };
            ui.label(RichText::new("Your profile").font(t::regular(12.0)).color(t::FAINT));
            ui.add_space(8.0);
            t::divider(ui);
        });
    }

    fn page_header(ui: &mut egui::Ui, eyebrow: &str, title: &str, sub: &str) {
        ui.label(RichText::new(eyebrow).font(t::regular(13.0)).color(t::FAINT));
        t::heading(ui, title);
        if !sub.is_empty() {
            t::muted(ui, sub);
        }
        ui.add_space(14.0);
    }

    // ---------------------------------------------------------- portfolio --

    fn ui_portfolio(&mut self, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Step 1 of 3",
            "Describe your portfolio in your own words",
            "Percentages only, never amounts. Write it the way you would say it to a friend.",
        );

        t::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Your portfolio").font(t::semibold(13.5)).color(t::NAV_TEXT));
            ui.add(
                egui::TextEdit::multiline(&mut self.st.input_text)
                    .font(t::regular(17.0))
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .margin(Margin::symmetric(16, 14))
                    .background_color(t::BG)
                    .hint_text("e.g. I have 25% NVIDIA and 50% VVSM ETF, the rest in cash"),
            );
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if !self.understood.is_empty() {
                    ui.label(RichText::new("Understood:").font(t::regular(13.0)).color(t::FAINT));
                    for (tk, pct) in &self.understood {
                        let name = self.db.get(tk).map_or(tk.clone(), |a| a.name.clone());
                        t::chip(ui, &fmt_pct(*pct), &name);
                    }
                }
                if !self.parse_msg.is_empty() {
                    ui.label(RichText::new(&self.parse_msg).color(t::WARN_TEXT));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let read = t::primary(ui, "Read portfolio").clicked();
                    let add = t::secondary(ui, "Add to current").clicked();
                    if read || add {
                        let r = portfolio::parse(&self.db, &self.st.input_text);
                        self.understood = r.portfolio.holdings.iter().map(|h| (h.ticker.clone(), h.percent)).collect();
                        self.parse_msg = if r.portfolio.is_empty() {
                            "Couldn't find any \"percentage + asset\" pairs.".into()
                        } else if !r.unknown.is_empty() {
                            format!("Not in the database: {}", r.unknown.join(", "))
                        } else {
                            String::new()
                        };
                        if !r.portfolio.is_empty() {
                            if read {
                                self.apply_portfolio(r.portfolio);
                            } else {
                                for h in r.portfolio.holdings {
                                    self.st.portfolio.add(h);
                                }
                                self.st.portfolio.sort();
                                self.changed();
                            }
                        }
                    }
                });
            });
        });
        ui.add_space(16.0);

        let total = self.st.portfolio.total();
        let mut edited = false;
        let mut remove = None;
        t::card().inner_margin(Margin::ZERO).show(ui, |ui| {
            ui.set_width(ui.available_width());
            Frame::new().inner_margin(Margin::symmetric(20, 16)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    t::section_title(ui, "Positions");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let ok = (total - 100.0).abs() < 0.5;
                        let c = if ok { t::GOOD } else { t::WARN_TEXT };
                        ui.label(RichText::new(format!("{} of 100%", fmt_pct(total))).font(t::semibold(14.0)).color(c));
                        t::bar(ui, 200.0, 8.0, total / 100.0, if ok { t::GOOD } else { t::WARN });
                    });
                });
            });
            t::divider(ui);
            if self.st.portfolio.is_empty() {
                Frame::new().inner_margin(Margin::symmetric(20, 24)).show(ui, |ui| {
                    t::muted(ui, "No positions yet. Describe your portfolio above, or add assets one by one below.");
                });
            }
            let w = ui.available_width();
            for i in 0..self.st.portfolio.holdings.len() {
                let (tk, known) = {
                    let h = &self.st.portfolio.holdings[i];
                    (h.ticker.clone(), h.known)
                };
                let (kind, fg, bg) = self.kind(&tk);
                let name = self.db.get(&tk).map(|a| a.name.clone()).unwrap_or_else(|| {
                    if known { String::new() } else { "Not in the database — treated as a single risky position".into() }
                });
                Frame::new().inner_margin(Margin::symmetric(20, 12)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let inner = w - 40.0;
                        t::column(ui, inner * 0.38, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.label(RichText::new(&tk).font(t::semibold(14.5)));
                            ui.label(RichText::new(name).font(t::regular(13.0)).color(t::MUTED));
                        });
                        t::column(ui, inner * 0.18, |ui| t::tag(ui, kind, fg, bg));
                        let h = &mut self.st.portfolio.holdings[i];
                        edited |= ui
                            .add_sized(
                                [84.0, 40.0],
                                egui::DragValue::new(&mut h.percent).range(0.0..=100.0).speed(0.25).fixed_decimals(1).suffix(" %"),
                            )
                            .changed();
                        let bw = (ui.available_width() - 56.0).max(40.0);
                        t::bar(ui, bw, 6.0, h.percent / 100.0, t::ACCENT);
                        if t::icon_button(ui, Icon::Close, &format!("Remove {tk}")).clicked() {
                            remove = Some(i);
                        }
                    });
                });
                t::divider(ui);
            }
            Frame::new().inner_margin(Margin::symmetric(20, 16)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let te = ui.add(
                        egui::TextEdit::singleline(&mut self.new_ticker)
                            .desired_width((ui.available_width() - 90.0).max(120.0))
                            .margin(Margin::symmetric(14, 11))
                            .background_color(t::BG)
                            .hint_text("Add a ticker or name, e.g. VWCE"),
                    );
                    let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (t::secondary(ui, "Add").clicked() || enter) && !self.new_ticker.trim().is_empty() {
                        let (ticker, known) = match self.db.find(self.new_ticker.trim()) {
                            Some(a) => (a.ticker.clone(), true),
                            None => (self.new_ticker.trim().to_uppercase(), false),
                        };
                        let start = (100.0 - self.st.portfolio.total()).clamp(0.0, 10.0);
                        self.st.portfolio.add(Holding { ticker, percent: start, known });
                        self.new_ticker.clear();
                        edited = true;
                    }
                });
            });
        });
        if let Some(i) = remove {
            self.st.portfolio.holdings.remove(i);
            edited = true;
        }

        let total = self.st.portfolio.total();
        if !self.st.portfolio.is_empty() && (total - 100.0).abs() > 0.5 {
            ui.add_space(16.0);
            let under = total < 100.0;
            Frame::new()
                .fill(t::BANNER_BG)
                .stroke(Stroke::new(1.0, t::BANNER_BORDER))
                .corner_radius(16)
                .inner_margin(Margin::symmetric(20, 16))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        t::icon(ui, Icon::Alert, 22.0, t::WARN_TEXT);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let title = if under {
                                format!("{} of your portfolio is not described", fmt_pct(100.0 - total))
                            } else {
                                format!("Your percentages add up to {}", fmt_pct(total))
                            };
                            ui.label(RichText::new(title).font(t::semibold(14.5)));
                            ui.label(
                                RichText::new(if under {
                                    "Is the rest cash, another fund, crypto? Add it above or treat it as cash."
                                } else {
                                    "Scale everything so it adds up to exactly 100%."
                                })
                                .color(t::BANNER_TEXT),
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if under && t::warn_button(ui, "Fill the rest with cash").clicked() {
                                self.st.portfolio.fill_remainder_with_cash();
                                edited = true;
                            }
                            if t::secondary(ui, "Scale to 100%").clicked() {
                                self.st.portfolio.normalize();
                                edited = true;
                            }
                        });
                    });
                });
        }
        if edited {
            self.st.portfolio.sort();
            self.changed();
        }

        ui.add_space(20.0);
        ui.horizontal(|ui| {
            if t::secondary_enabled(ui, !self.st.portfolio.is_empty(), "Clear all").clicked() {
                self.st.portfolio = Portfolio::default();
                self.understood.clear();
                self.changed();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !self.st.portfolio.is_empty() {
                    let (label, tab) = if self.st.profile.answered() == 0 {
                        ("Next: your investor profile  →", Tab::Profile)
                    } else {
                        ("See my analysis  →", Tab::Analysis)
                    };
                    if t::primary(ui, label).clicked() {
                        self.tab = tab;
                    }
                }
            });
        });
    }

    // ------------------------------------------------------------ profile --

    fn ui_profile(&mut self, ui: &mut egui::Ui) {
        let answered = self.st.profile.answered();
        Self::page_header(
            ui,
            &format!("Step 2 of 3 · {answered} of {} answered", QUESTIONS.len()),
            "What kind of investor are you?",
            "Seven quick questions. Your answers set how much risk the analysis and the AI treat as acceptable for you.",
        );
        let w = ui.available_width();
        let wide = w > 900.0;
        let left_w = if wide { (w - 24.0) * 0.6 } else { w };
        let mut edited = false;

        let questions = |this: &mut Self, ui: &mut egui::Ui, edited: &mut bool| {
            for (qi, q) in QUESTIONS.iter().enumerate() {
                Frame::new()
                    .fill(t::SURFACE)
                    .stroke(Stroke::new(1.0, t::BORDER))
                    .corner_radius(14)
                    .inner_margin(Margin::symmetric(18, 16))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{:02}", qi + 1)).font(t::regular(14.5)).color(t::FAINT));
                            ui.label(RichText::new(q.text).font(t::semibold(15.0)));
                        });
                        ui.add_space(2.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                            for (oi, (label, _)) in q.options.iter().enumerate() {
                                let sel = this.st.profile.answers[qi] == Some(oi);
                                if t::choice(ui, sel, label).clicked() {
                                    this.st.profile.answers[qi] = Some(oi);
                                    *edited = true;
                                }
                            }
                        });
                    });
                ui.add_space(4.0);
            }
            Frame::new()
                .fill(t::SURFACE)
                .stroke(Stroke::new(1.0, t::BORDER))
                .corner_radius(14)
                .inner_margin(Margin::symmetric(18, 16))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Anything else the advisor should know?").font(t::semibold(15.0)));
                    ui.label(RichText::new("Age, goals, values, positions you won't sell. The AI adds notes here as you chat.").font(t::regular(13.0)).color(t::MUTED));
                    *edited |= ui
                        .add(
                            egui::TextEdit::multiline(&mut this.st.profile.notes)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(14, 12))
                                .background_color(t::BG),
                        )
                        .changed();
                });
        };

        if wide {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 24.0;
                t::column(ui, left_w, |ui| questions(self, ui, &mut edited));
                let rw = ui.available_width();
                t::column(ui, rw, |ui| self.profile_result(ui));
            });
        } else {
            self.profile_result(ui);
            ui.add_space(16.0);
            questions(self, ui, &mut edited);
        }
        if edited {
            self.changed();
        }
    }

    fn profile_result(&mut self, ui: &mut egui::Ui) {
        let level = self.st.profile.risk_level();
        let capped = self.st.profile.is_capped();
        t::card_accent().inner_margin(Margin::same(22)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            t::eyebrow(ui, "Your investor type");
            let Some(l) = level else {
                ui.label(RichText::new("Not set yet").font(t::semibold(26.0)));
                t::muted(ui, "Answer the questions to see your investor type and the limits we'll use for you.");
                return;
            };
            ui.label(RichText::new(l.label()).font(t::semibold(28.0)));
            t::muted(ui, l.description());
            ui.add_space(8.0);

            // 1..5 scale
            let w = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(vec2(w, 10.0), Sense::hover());
            let seg = (w - 4.0 * 4.0) / 5.0;
            for n in 1..=5u8 {
                let x = rect.left() + (n - 1) as f32 * (seg + 4.0);
                let r = egui::Rect::from_min_size(egui::pos2(x, rect.top()), vec2(seg, 10.0));
                let c = if n <= l.level() { t::ACCENT } else { t::TRACK };
                if n == l.level() {
                    ui.painter().rect_filled(r.expand(3.0), 8, t::SELECTED);
                }
                ui.painter().rect_filled(r, 5, c);
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Conservative").font(t::regular(12.0)).color(t::FAINT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("Aggressive").font(t::regular(12.0)).color(t::FAINT));
                });
            });
            ui.add_space(6.0);
            t::divider(ui);
            ui.add_space(6.0);

            t::eyebrow(ui, "Typical mix for you");
            let (e, b, c) = l.target_mix();
            let w = ui.available_width();
            t::stacked_bar(ui, w, 12.0, &[(e / 100.0, t::ACCENT), (b / 100.0, t::GOOD), (c / 100.0, t::FAINT)]);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                for (label, v, col) in [("Equities", e, t::ACCENT), ("Bonds", b, t::GOOD), ("Cash / gold", c, t::FAINT)] {
                    ui.horizontal(|ui| {
                        t::dot(ui, col);
                        ui.label(RichText::new(format!("{label} {v:.0}%")).font(t::regular(13.0)).color(t::NAV_TEXT));
                    });
                }
            });
            ui.add_space(6.0);
            let tw = (ui.available_width() - 12.0) / 2.0;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for (label, v) in [("Max in one company", l.max_single_stock()), ("Max in satellites", l.max_satellite())] {
                    t::column(ui, tw, |ui| {
                        Frame::new().fill(t::BG).corner_radius(12).inner_margin(Margin::symmetric(14, 12)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.label(RichText::new(label).font(t::regular(12.0)).color(t::FAINT));
                            ui.label(RichText::new(fmt_pct(v)).font(t::semibold(20.0)));
                        });
                    });
                }
            });
            if capped {
                ui.label(
                    RichText::new("Your comfort with risk is higher, but your horizon, emergency fund or reaction to a crash limits how much risk is sensible.")
                        .font(t::regular(13.0))
                        .color(t::WARN_TEXT),
                );
            }
            ui.add_space(4.0);
            if !self.st.profile.is_complete() {
                ui.label(
                    RichText::new(format!("{} of {} answered — more answers give a better result.", self.st.profile.answered(), QUESTIONS.len()))
                        .font(t::regular(13.0))
                        .color(t::FAINT),
                );
            }
            let go = if self.st.portfolio.is_empty() { "Describe my portfolio" } else { "See my analysis" };
            if t::primary(ui, go).clicked() {
                self.tab = if self.st.portfolio.is_empty() { Tab::Portfolio } else { Tab::Analysis };
            }
        });
    }

    // ----------------------------------------------------------- analysis --

    fn ui_analysis(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.st.portfolio.is_empty() {
            Self::page_header(ui, "Analysis", "Nothing to analyse yet", "");
            t::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                t::muted(ui, "Describe your portfolio with percentages, e.g. \"60% world ETF, 30% bonds, 10% cash\".");
                if t::primary(ui, "Describe my portfolio").clicked() {
                    self.tab = Tab::Portfolio;
                }
            });
            return;
        }
        let r = self.report().clone();
        let level = self.st.profile.risk_level();

        // Header
        let top: Vec<String> = self.st.portfolio.holdings.iter().take(3).map(|h| format!("{} {}", h.ticker, fmt_pct(h.percent))).collect();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(format!("Analysis · {}", top.join(" · "))).font(t::regular(13.0)).color(t::FAINT));
                t::heading(ui, "How healthy is this portfolio?");
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if t::primary(ui, "Ask the AI advisor").clicked() {
                    self.send(ctx, "Analyse my portfolio: what is good, what is bad, what kind of investor this portfolio suits, and how I should improve it for my profile.".into());
                }
                if t::secondary(ui, "Edit portfolio").clicked() {
                    self.tab = Tab::Portfolio;
                }
            });
        });
        ui.add_space(16.0);

        // Summary cards
        let crit = r.findings.iter().filter(|f| f.severity == Severity::Critical).count();
        let warn = r.findings.iter().filter(|f| f.severity == Severity::Warning).count();
        let w = ui.available_width();
        let cols = if w > 980.0 { 4 } else { 2 };
        let cw = (w - 16.0 * (cols as f32 - 1.0)) / cols as f32;
        let mut cards: Vec<Box<dyn FnOnce(&mut egui::Ui) + '_>> = Vec::new();
        let health = r.health;
        cards.push(Box::new(move |ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                let (label, col) = match health {
                    80..=100 => ("Healthy", t::GOOD),
                    55..=79 => ("Fair", t::WARN),
                    _ => ("Needs attention", t::CRIT),
                };
                t::score_ring(ui, 84.0, health, col);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    t::eyebrow(ui, "Health score");
                    ui.label(RichText::new(label).font(t::semibold(17.0)).color(if col == t::CRIT { t::CRIT_TEXT } else { col }));
                    ui.label(RichText::new(format!("{crit} critical · {warn} warnings")).font(t::regular(13.0)).color(t::MUTED));
                });
            });
        }));
        let revealed = r.revealed;
        cards.push(Box::new(move |ui: &mut egui::Ui| {
            t::eyebrow(ui, "Risk fit");
            ui.label(RichText::new(revealed.map_or("—", |v| v.label())).font(t::semibold(22.0)));
            let sub = match (level, revealed) {
                (Some(p), Some(v)) => {
                    let d = v.level() as i8 - p.level() as i8;
                    match d {
                        0 => format!("Matches your profile ({})", p.label()),
                        1 => format!("You are {} — one level riskier than your profile", p.label()),
                        d if d > 1 => format!("You are {} — {d} levels riskier than your profile", p.label()),
                        -1 => format!("You are {} — a little more cautious, which is fine", p.label()),
                        _ => format!("You are {} — safer than you need to be", p.label()),
                    }
                }
                _ => "Answer the profile to compare".into(),
            };
            ui.label(RichText::new(sub).font(t::regular(13.0)).color(t::MUTED));
        }));
        let top_name = r.companies.first().cloned();
        let direct = top_name.as_ref().and_then(|(tk, _)| self.st.portfolio.holdings.iter().find(|h| &h.ticker == tk)).map(|h| h.percent);
        let limit = level.unwrap_or(RiskLevel::Balanced).max_single_stock();
        cards.push(Box::new(move |ui: &mut egui::Ui| {
            t::eyebrow(ui, "Biggest single name");
            match &top_name {
                Some((tk, wv)) => {
                    ui.label(RichText::new(format!("{tk} {}", fmt_pct(*wv))).font(t::semibold(22.0)));
                    let inside = wv - direct.unwrap_or(0.0);
                    let sub = match direct {
                        Some(d) if inside > 0.05 => format!("{} direct + {} inside funds · limit {}", fmt_pct(d), fmt_pct(inside), fmt_pct(limit)),
                        _ => format!("Limit for your profile {}", fmt_pct(limit)),
                    };
                    ui.label(RichText::new(sub).font(t::regular(13.0)).color(t::MUTED));
                }
                None => {
                    ui.label(RichText::new("Spread out").font(t::semibold(22.0)));
                }
            }
        }));
        let eff = r.effective_positions;
        let ter = r.ter;
        cards.push(Box::new(move |ui: &mut egui::Ui| {
            t::eyebrow(ui, "Diversification");
            let n = if eff >= 500.0 { "500+".to_string() } else { format!("≈{eff:.0}") };
            ui.label(RichText::new(format!("{n} positions")).font(t::semibold(22.0)));
            let cost = ter.map_or(String::new(), |c| format!(" · avg cost {c:.2}%/yr"));
            ui.label(RichText::new(format!("Effective count{cost}")).font(t::regular(13.0)).color(t::MUTED));
        }));
        let mut it = cards.into_iter();
        loop {
            let row: Vec<_> = it.by_ref().take(cols).collect();
            if row.is_empty() {
                break;
            }
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                for f in row {
                    t::column(ui, cw, |ui| {
                        t::card().show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.set_min_height(84.0);
                            ui.spacing_mut().item_spacing.y = 4.0;
                            f(ui);
                        });
                    });
                }
            });
            ui.add_space(16.0);
        }

        // Findings + right column
        let w = ui.available_width();
        let wide = w > 900.0;
        let lw = if wide { (w - 24.0) * 0.58 } else { w };
        let findings = |ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                t::section_title(ui, "Good and bad points");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("Ranked by importance").font(t::regular(13.0)).color(t::FAINT));
                });
            });
            for f in &r.findings {
                let (label, fg, bg) = sev_style(f.severity);
                Frame::new()
                    .fill(t::SURFACE)
                    .stroke(Stroke::new(1.0, t::BORDER))
                    .corner_radius(14)
                    .inner_margin(Margin::symmetric(18, 14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_top(|ui| {
                            t::column(ui, 78.0, |ui| t::tag(ui, label, fg, bg));
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.add(egui::Label::new(RichText::new(&f.title).font(t::semibold(15.0))).wrap());
                                if !f.detail.is_empty() {
                                    ui.add(egui::Label::new(RichText::new(&f.detail).color(t::MUTED)).wrap());
                                }
                            });
                        });
                    });
            }
        };
        let mut right = |ui: &mut egui::Ui| self.analysis_side(ui, ctx, &r, level);
        if wide {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 24.0;
                t::column(ui, lw, findings);
                let rw = ui.available_width();
                t::column(ui, rw, |ui| right(ui));
            });
        } else {
            findings(ui);
            ui.add_space(16.0);
            right(ui);
        }
    }

    fn analysis_side(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, r: &Report, level: Option<RiskLevel>) {
        let limit = level.unwrap_or(RiskLevel::Balanced).max_single_stock();
        t::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            t::section_title(ui, "What you really own");
            t::eyebrow(ui, "Single names, incl. inside funds");
            let max = r.companies.first().map_or(1.0, |c| c.1).max(1.0);
            for (tk, wv) in r.companies.iter().take(5) {
                t::bar_row(ui, tk, wv / max, &fmt_pct(*wv), if *wv > limit { t::CRIT } else { t::ACCENT });
            }
            if r.companies.is_empty() {
                t::muted(ui, "Only diversified funds — no single company stands out.");
            }
            ui.add_space(4.0);
            t::divider(ui);
            ui.add_space(4.0);
            t::eyebrow(ui, "Sectors");
            let w = ui.available_width();
            let mut segs: Vec<(f32, Color32)> = Vec::new();
            for (i, (_, v)) in r.sectors.iter().take(5).enumerate() {
                segs.push((v / 100.0, REGION_COLORS[i]));
            }
            t::stacked_bar(ui, w, 10.0, &segs);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                for (i, (k, v)) in r.sectors.iter().take(5).enumerate() {
                    t::dot(ui, REGION_COLORS[i]);
                    ui.label(RichText::new(format!("{k} {}", fmt_pct(*v))).font(t::regular(13.0)).color(t::MUTED));
                }
            });
            ui.add_space(4.0);
            t::eyebrow(ui, "Regions");
            let mut segs: Vec<(f32, Color32)> = Vec::new();
            for (i, (_, v)) in r.regions.iter().take(5).enumerate() {
                segs.push((v / 100.0, REGION_COLORS[i]));
            }
            t::stacked_bar(ui, w, 10.0, &segs);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                for (i, (k, v)) in r.regions.iter().take(5).enumerate() {
                    t::dot(ui, REGION_COLORS[i]);
                    ui.label(RichText::new(format!("{k} {}", fmt_pct(*v))).font(t::regular(13.0)).color(t::MUTED));
                }
                if r.total < 99.5 {
                    ui.label(RichText::new(format!("Undescribed {}", fmt_pct(100.0 - r.total))).font(t::regular(13.0)).color(t::MUTED));
                }
            });
        });
        ui.add_space(16.0);

        match (&r.proposal, level) {
            (Some(p), Some(l)) if *p != self.st.portfolio => {
                let new_health = analysis::analyse(p, &self.db, Some(l)).health;
                let p = p.clone();
                t::card_accent().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    t::section_title(ui, &format!("A version that fits a {} investor", l.label().to_lowercase()));
                    t::muted(ui, "Keeps your picks as satellites, adds a global core and a cushion.");
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Now").font(t::regular(12.0)).color(t::FAINT));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(format!("Health {} → {}", r.health, new_health)).font(t::semibold(12.5)).color(t::GOOD));
                        });
                    });
                    self.allocation_bar(ui, &self.st.portfolio.clone(), 14.0);
                    ui.label(RichText::new("Suggested").font(t::regular(12.0)).color(t::FAINT));
                    self.allocation_bar(ui, &p, 14.0);
                    ui.add_space(4.0);
                    self.allocation_legend(ui, &p);
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        if t::primary(ui, "Apply this portfolio").clicked() {
                            self.apply_portfolio(p.clone());
                        }
                        if t::secondary(ui, "Discuss with the AI").clicked() {
                            let msg = format!("The rule engine suggests: {}. Is this right for me? What would you change, and why?", p.describe());
                            self.send(ctx, msg);
                        }
                    });
                });
                ui.add_space(16.0);
            }
            (_, None) => {
                t::card_accent().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    t::section_title(ui, "Get a portfolio that fits you");
                    t::muted(ui, "Answer seven quick questions so suggestions match the risk you can really live with.");
                    if t::primary(ui, "Take the investor profile").clicked() {
                        self.tab = Tab::Profile;
                    }
                });
                ui.add_space(16.0);
            }
            _ => {}
        }

        if !r.suggestions.is_empty() {
            t::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                t::section_title(ui, "Next steps");
                for (i, s) in r.suggestions.iter().enumerate() {
                    ui.horizontal_top(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 11.0, t::RAISED);
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, (i + 1).to_string(), t::semibold(12.0), t::TEXT);
                        let tw = ui.available_width();
                        t::column(ui, tw, |ui| ui.add(egui::Label::new(RichText::new(s).color(t::TEXT_SOFT)).wrap()));
                    });
                }
            });
        }
    }

    // ------------------------------------------------------------ advisor --

    fn ui_advisor(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::Panel::top("adv_head")
            .frame(Frame::new().fill(t::BG).inner_margin(Margin::symmetric(32, 18)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(RichText::new("AI advisor").font(t::semibold(22.0)));
                        ui.horizontal(|ui| {
                            let (c, s) = match self.ai_ok {
                                Some(true) => (t::GOOD, "connected"),
                                Some(false) => (t::CRIT, "not reachable"),
                                None => (t::FAINT, "runs on this computer"),
                            };
                            t::dot(ui, c);
                            ui.label(RichText::new(format!("{} · {s} · free and private", self.st.ai.model)).font(t::regular(13.0)).color(t::MUTED));
                        });
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if t::secondary_enabled(ui, self.job.is_none() && !self.st.chat.is_empty(), "New conversation").clicked() {
                            self.st.chat.clear();
                            self.ai_error = None;
                            self.changed();
                        }
                    });
                });
            });

        // Composer
        let mut send_now = false;
        egui::Panel::bottom("adv_input")
            .frame(Frame::new().fill(t::BG).inner_margin(Margin { left: 32, right: 32, top: 14, bottom: 22 }))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let busy = self.job.is_some();
                    let te = ui.add(
                        egui::TextEdit::multiline(&mut self.chat_input)
                            .desired_rows(2)
                            .desired_width(ui.available_width() - 64.0)
                            .margin(Margin::symmetric(14, 12))
                            .background_color(t::SURFACE)
                            .hint_text("Ask anything, or tell the advisor about yourself…  (Enter to send, Shift+Enter for a new line)"),
                    );
                    if te.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift) {
                        while self.chat_input.ends_with('\n') {
                            self.chat_input.pop();
                        }
                        send_now = true;
                    }
                    let (rect, resp) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::click());
                    let hovered = resp.hovered();
                    if busy {
                        ui.painter().rect_stroke(rect, 12, Stroke::new(1.0, t::BORDER_STRONG), egui::StrokeKind::Inside);
                        ui.painter().rect_filled(egui::Rect::from_center_size(rect.center(), vec2(12.0, 12.0)), 2, t::TEXT);
                        if resp.on_hover_text("Stop").clicked() {
                            if let Some(j) = &self.job {
                                j.cancel();
                            }
                        }
                    } else {
                        let fill = if hovered { Color32::from_rgb(0x93, 0xAE, 0xFF) } else { t::ACCENT };
                        ui.painter().rect_filled(rect, 12, fill);
                        t::paint_icon(ui.painter(), Icon::Arrow, rect.center(), 18.0, t::BG);
                        if resp.on_hover_text("Send").clicked() {
                            send_now = true;
                        }
                    }
                });
            });
        if send_now && self.job.is_none() {
            let txt = std::mem::take(&mut self.chat_input);
            self.send(ctx, txt);
        }

        if ui.available_width() > 860.0 {
            egui::Panel::right("adv_ctx")
                .resizable(false)
                .exact_size(290.0)
                .frame(Frame::new().fill(t::BG).inner_margin(Margin::symmetric(24, 24)))
                .show(ui, |ui| self.advisor_context(ui));
        }

        let mut apply: Option<Portfolio> = None;
        let mut ask: Option<String> = None;
        let mut goto: Option<Tab> = None;
        egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            Frame::new().inner_margin(Margin::symmetric(32, 24)).show(ui, |ui| {
                let mw = ui.available_width().min(820.0);
                ui.set_max_width(mw);
                let w = ui.available_width();
                ui.spacing_mut().item_spacing.y = 18.0;

                if self.st.chat.is_empty() && self.job.is_none() {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
                        ui.painter().rect_filled(rect, 16, t::RAISED);
                        t::paint_icon(ui.painter(), Icon::Sparkle, rect.center(), 26.0, t::ACCENT);
                        ui.label(RichText::new("Your personal portfolio advisor").font(t::semibold(20.0)));
                        t::muted(ui, "It sees your analysis and profile, remembers what you tell it, and can propose portfolios you apply with one click.");
                    });
                    ui.add_space(6.0);
                    for q in [
                        "Analyse my portfolio and tell me how to improve it.",
                        "What kind of investor am I, based on my answers and my portfolio?",
                        "I want more growth but I'm scared of big losses. What should I change?",
                    ] {
                        let b = ui.add_sized(
                            [w, 44.0],
                            egui::Button::new(RichText::new(q).color(t::TEXT_SOFT)).fill(t::SURFACE).stroke(Stroke::new(1.0, t::BORDER)).corner_radius(12),
                        );
                        if b.clicked() {
                            ask = Some(q.into());
                        }
                    }
                }

                for m in &self.st.chat {
                    match m.role {
                        Role::User => {
                            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                Frame::new()
                                    .fill(t::SELECTED)
                                    .corner_radius(CornerRadius { nw: 16, ne: 16, sw: 16, se: 4 })
                                    .inner_margin(Margin::symmetric(16, 12))
                                    .show(ui, |ui| {
                                        ui.set_max_width(w * 0.78);
                                        ui.add(egui::Label::new(RichText::new(&m.content)).wrap());
                                    });
                            });
                        }
                        Role::Assistant => {
                            ui.horizontal_top(|ui| {
                                let (rect, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
                                ui.painter().rect_filled(rect, 9, t::RAISED);
                                t::paint_icon(ui.painter(), Icon::Sparkle, rect.center(), 16.0, t::ACCENT);
                                let cw = ui.available_width();
                                t::column(ui, cw, |ui| {
                                    ui.spacing_mut().item_spacing.y = 6.0;
                                    markdown(ui, &display_text(&m.content));
                                    if let Some(block) = ai::extract_proposal(&m.content) {
                                        let p = portfolio::parse_block(&self.db, &block);
                                        if !p.is_empty() {
                                            ui.add_space(4.0);
                                            let lvl = self.st.profile.risk_level();
                                            let before = analysis::analyse(&self.st.portfolio, &self.db, lvl).health;
                                            let after = analysis::analyse(&p, &self.db, lvl).health;
                                            Frame::new()
                                                .fill(t::SURFACE)
                                                .stroke(Stroke::new(1.0, t::BORDER_ACCENT))
                                                .corner_radius(14)
                                                .inner_margin(Margin::same(16))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.horizontal(|ui| {
                                                        ui.label(RichText::new("Proposed portfolio").font(t::semibold(14.5)));
                                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                            let c = if after >= before { t::GOOD } else { t::WARN_TEXT };
                                                            ui.label(RichText::new(format!("Health {before} → {after}")).font(t::semibold(12.5)).color(c));
                                                        });
                                                    });
                                                    self.allocation_bar(ui, &p, 12.0);
                                                    self.allocation_legend(ui, &p);
                                                    if (p.total() - 100.0).abs() > 0.5 {
                                                        ui.label(RichText::new(format!("Adds up to {} — it will be shown as is.", fmt_pct(p.total()))).font(t::regular(12.5)).color(t::WARN_TEXT));
                                                    }
                                                    ui.horizontal_wrapped(|ui| {
                                                        if t::primary(ui, "Apply this portfolio").clicked() {
                                                            apply = Some(p.clone());
                                                        }
                                                        if t::secondary(ui, "Compare with current").clicked() {
                                                            goto = Some(Tab::Analysis);
                                                        }
                                                    });
                                                });
                                        }
                                    }
                                    let notes = notes_in(&m.content);
                                    if !notes.is_empty() {
                                        Frame::new().fill(t::GOOD_BG).corner_radius(8).inner_margin(Margin::symmetric(12, 6)).show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                t::icon(ui, Icon::Check, 14.0, t::GOOD);
                                                ui.label(RichText::new(format!("Remembered: {}", notes.join(" · "))).font(t::regular(13.0)).color(t::GOOD));
                                            });
                                        });
                                    }
                                });
                            });
                        }
                    }
                }

                if self.job.is_some() {
                    ui.horizontal_top(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
                        ui.painter().rect_filled(rect, 9, t::RAISED);
                        t::paint_icon(ui.painter(), Icon::Sparkle, rect.center(), 16.0, t::ACCENT);
                        let cw = ui.available_width();
                        t::column(ui, cw, |ui| {
                            let shown = display_text(&ai::strip_thinking(&self.streaming));
                            if shown.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(RichText::new("Thinking…").color(t::MUTED));
                                });
                            } else {
                                markdown(ui, &shown);
                            }
                        });
                    });
                }

                if let Some(e) = &self.ai_error {
                    Frame::new()
                        .fill(t::CRIT_BG)
                        .corner_radius(14)
                        .inner_margin(Margin::symmetric(18, 14))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_top(|ui| {
                                t::icon(ui, Icon::Alert, 20.0, t::CRIT_TEXT);
                                ui.vertical(|ui| {
                                    ui.label(RichText::new("The AI isn't available").font(t::semibold(15.0)).color(t::CRIT_TEXT));
                                    ui.add(egui::Label::new(RichText::new(e).color(t::TEXT_SOFT)).wrap());
                                    if t::secondary(ui, "Open settings").clicked() {
                                        goto = Some(Tab::Settings);
                                    }
                                });
                            });
                        });
                }
            });
        });
        if let Some(p) = apply {
            self.apply_portfolio(p);
            let note = format!("I applied your proposal. My portfolio is now: {}", self.st.portfolio.describe());
            self.st.chat.push(ChatMessage { role: Role::User, content: note });
        }
        if let Some(q) = ask {
            self.send(ctx, q);
        }
        if let Some(tb) = goto {
            self.tab = tb;
        }
    }

    fn advisor_context(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 6.0;
        t::eyebrow(ui, "What the advisor knows");
        ui.add_space(8.0);
        ui.label(RichText::new("Investor type").font(t::regular(13.0)).color(t::MUTED));
        match self.st.profile.risk_level() {
            Some(l) => ui.label(RichText::new(format!("{} · {} of 5", l.label(), l.level())).font(t::semibold(14.5))),
            None => ui.label(RichText::new("Not answered yet").font(t::semibold(14.5)).color(t::WARN_TEXT)),
        };
        ui.add_space(10.0);
        ui.label(RichText::new("Current portfolio").font(t::regular(13.0)).color(t::MUTED));
        if self.st.portfolio.is_empty() {
            ui.label(RichText::new("Not described yet").font(t::semibold(14.5)).color(t::WARN_TEXT));
        } else {
            let p = self.st.portfolio.clone();
            ui.add(
                egui::Label::new(
                    RichText::new(p.holdings.iter().map(|h| format!("{} {}", h.ticker, fmt_pct(h.percent))).collect::<Vec<_>>().join(" · "))
                        .font(t::semibold(14.5)),
                )
                .wrap(),
            );
            ui.add_space(2.0);
            self.allocation_bar(ui, &p, 8.0);
        }
        ui.add_space(10.0);
        ui.label(RichText::new("Notes from your chats").font(t::regular(13.0)).color(t::MUTED));
        let notes: Vec<String> = self.st.profile.notes.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if notes.is_empty() {
            ui.label(RichText::new("Nothing yet — tell it about your goals.").font(t::regular(13.0)).color(t::FAINT));
        } else {
            for n in &notes {
                Frame::new().fill(t::RAISED).corner_radius(10).inner_margin(Margin::symmetric(10, 5)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(RichText::new(n).font(t::regular(13.0))).wrap());
                });
            }
        }
        ui.add_space(14.0);
        Frame::new().fill(t::SURFACE).stroke(Stroke::new(1.0, t::BORDER)).corner_radius(12).inner_margin(Margin::same(14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    RichText::new("Every number comes from the analysis engine, not the AI. The AI explains and helps you decide.")
                        .font(t::regular(13.0))
                        .color(t::MUTED),
                )
                .wrap(),
            );
        });
    }

    // ----------------------------------------------------------- settings --

    fn ui_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        Self::page_header(
            ui,
            "Settings",
            "AI and data",
            "Any OpenAI-compatible endpoint works. The default is a free local Ollama server: private, offline, no account.",
        );
        t::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            t::section_title(ui, "AI connection");
            ui.add_space(4.0);
            let before = self.st.ai.clone();
            let s = &mut self.st.ai;
            egui::Grid::new("ai_settings").num_columns(2).spacing([20.0, 12.0]).show(ui, |ui| {
                for (label, value, pw) in [("Endpoint", &mut s.base_url, false), ("Model", &mut s.model, false), ("API key (optional)", &mut s.api_key, true)] {
                    ui.label(RichText::new(label).color(t::MUTED));
                    ui.add(
                        egui::TextEdit::singleline(value)
                            .password(pw)
                            .desired_width(380.0)
                            .margin(Margin::symmetric(12, 10))
                            .background_color(t::BG),
                    );
                    ui.end_row();
                }
                ui.label(RichText::new("Creativity").color(t::MUTED));
                ui.add(egui::Slider::new(&mut s.temperature, 0.0..=1.0));
                ui.end_row();
                ui.label(RichText::new("Messages of memory").color(t::MUTED));
                ui.add(egui::Slider::new(&mut s.history, 4..=40));
                ui.end_row();
            });
            if self.st.ai != before {
                self.last_change = Some(Instant::now());
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if t::primary_enabled(ui, self.models_rx.is_none(), "Test connection").clicked() {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let settings = self.st.ai.clone();
                    let c = ctx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(ai::list_models(&settings));
                        c.request_repaint();
                    });
                    self.models_rx = Some(rx);
                }
                if t::secondary(ui, "Reset to defaults").clicked() {
                    self.st.ai = ai::AiSettings::default();
                    self.last_change = Some(Instant::now());
                }
                if self.models_rx.is_some() {
                    ui.spinner();
                }
            });
            match &self.models {
                Some(Ok(list)) => {
                    ui.horizontal(|ui| {
                        t::dot(ui, t::GOOD);
                        ui.label(RichText::new(format!("Connected · {} models — click one to use it", list.len())).color(t::GOOD));
                    });
                    let mut pick = None;
                    ui.horizontal_wrapped(|ui| {
                        for m in list {
                            if t::choice(ui, *m == self.st.ai.model, m).clicked() {
                                pick = Some(m.clone());
                            }
                        }
                    });
                    if let Some(m) = pick {
                        self.st.ai.model = m;
                        self.last_change = Some(Instant::now());
                    }
                }
                Some(Err(e)) => {
                    ui.add(egui::Label::new(RichText::new(e).color(t::CRIT_TEXT)).wrap());
                }
                None => {}
            }
        });
        ui.add_space(16.0);
        t::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            t::section_title(ui, "Your data");
            t::muted(ui, format!("Saved on this computer in {}", storage::data_dir().display()));
            t::muted(ui, format!("{} assets in the database. Add or correct assets with an assets.json file in that folder.", self.db.all().len()));
        });
    }
}
