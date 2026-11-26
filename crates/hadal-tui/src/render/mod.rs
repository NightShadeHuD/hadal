//! Rendering dispatch, shared visual vocabulary, overlays, and too-small screen.

pub mod archive;
pub mod briefing;
pub mod descent;
pub mod dive;
pub mod help;
pub mod hits;
pub mod sonar;
pub mod system;

pub use hits::{HitMap, Tab, command_for_click};

use hadal_core::Decimal;
use hadal_core::OfflineReport;
use hadal_core::StateView;
use hadal_core::content::ids::GeneratorId;
use hadal_core::decimal::{to_human_ceil, to_human_floor, to_rate};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::glyphs::Glyphs;
use crate::theme::{ColorDepth, Palette, Theme};

/// Minimum supported terminal width.
pub const MIN_WIDTH: u16 = 80;
/// Minimum supported terminal height.
pub const MIN_HEIGHT: u16 = 24;

// ---------------------------------------------------------------------
// UI state
// ---------------------------------------------------------------------

/// Which primary screen to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// The boot/title sequence with this many boot lines revealed.
    Boot {
        /// Number of boot lines revealed so far.
        revealed: usize,
    },
    /// The first-run briefing panel.
    Briefing,
    /// The SONAR console.
    Sonar,
    /// The DESCENT screen.
    Descent,
    /// The ARCHIVE screen.
    Archive,
    /// The SYSTEM screen.
    System,
    /// The 900 ms dive transition, expressed in per-mille progress.
    Dive {
        /// Depth before the Descent.
        from_m: u32,
        /// Depth after the Descent.
        to_m: u32,
        /// Progress in `0..=1000`.
        progress: u16,
    },
}

impl Screen {
    /// The tab that should appear active for this screen.
    pub fn tab(self) -> Option<Tab> {
        match self {
            Screen::Sonar | Screen::Boot { .. } | Screen::Dive { .. } => Some(Tab::Sonar),
            Screen::Briefing => None,
            Screen::Descent => Some(Tab::Descent),
            Screen::Archive => Some(Tab::Archive),
            Screen::System => Some(Tab::System),
        }
    }
}

/// Keyboard focus on the current screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// No focused list.
    None,
    /// Signal Chain row index on SONAR.
    Upgrade(usize),
    /// Flattened hull-row index on DESCENT.
    Hull(usize),
    /// SYSTEM settings/automation row.
    System(usize),
}

/// Presentation-only animation state. Never mutates game state.
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    /// Tweened Echo value for the odometer.
    pub echo_display: Decimal,
    /// Expanding pulse ring phase in `0.0..1.0`, if a pulse is animating.
    pub pulse_phase: Option<f64>,
    /// Carrier-change flash.
    pub carrier_flash: bool,
    /// Per-generator purchase flash.
    pub row_flash: [bool; 4],
}

impl Anim {
    /// Settled animation state (used by snapshots).
    #[cfg(test)]
    pub fn settled(echo: Decimal) -> Self {
        Self {
            echo_display: echo,
            pulse_phase: None,
            carrier_flash: false,
            row_flash: [false; 4],
        }
    }
}

/// Settings values displayed by SYSTEM.
#[derive(Clone, Copy, Debug)]
pub struct SettingsView {
    pub theme: Theme,
    pub reduce_motion: bool,
    pub ascii: bool,
    pub mouse: bool,
    pub bell: bool,
    pub color_depth: ColorDepth,
}

impl Default for SettingsView {
    fn default() -> Self {
        Self {
            theme: Theme::Abyss,
            reduce_motion: false,
            ascii: false,
            mouse: true,
            bell: false,
            color_depth: ColorDepth::Auto,
        }
    }
}

/// A text-entry overlay kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// Export the current save.
    Export,
    /// Import a save file.
    Import,
    /// Reset the station after typing DISCARD.
    Reset,
}

impl EditKind {
    /// Overlay title.
    pub fn title(self) -> &'static str {
        match self {
            EditKind::Export => "EXPORT SAVE",
            EditKind::Import => "IMPORT SAVE",
            EditKind::Reset => "RESET STATION",
        }
    }

    /// Input prompt.
    pub fn prompt(self) -> &'static str {
        match self {
            EditKind::Export => "Export path:",
            EditKind::Import => "Import path:",
            EditKind::Reset => "Type DISCARD to delete save and backup:",
        }
    }
}

/// A text-entry overlay snapshot.
#[derive(Clone, Copy, Debug)]
pub struct EditView<'a> {
    pub kind: EditKind,
    pub buffer: &'a str,
    pub error: Option<&'a str>,
}

/// A read-only snapshot of everything the renderer needs.
pub struct UiState<'a> {
    pub view: StateView<'a>,
    pub palette: Palette,
    pub ascii: bool,
    pub screen: Screen,
    /// Persistent warning banner (save failure, config failure, load warning).
    pub banner: Option<&'a str>,
    /// Offline summary to show over the console.
    pub offline: Option<&'a OfflineReport>,
    /// Active toasts, oldest first.
    pub toasts: &'a [String],
    pub anim: &'a Anim,
    pub focus: Focus,
    pub confirm_descent: bool,
    /// Readable one-line rejection (governor/focus errors).
    pub notice: Option<&'a str>,
    /// ARCHIVE scroll offset.
    pub archive_scroll: usize,
    /// SYSTEM settings values.
    pub settings: SettingsView,
    /// Resolved save path (already sanitized).
    pub save_path: &'a str,
    /// Last successful save (Unix seconds).
    pub last_save: Option<u64>,
    /// Current Unix time for relative save status.
    pub now_unix: u64,
    /// Text-entry overlay, if open.
    pub edit: Option<EditView<'a>>,
    /// Help overlay, if open.
    pub help: bool,
    /// Help document scroll offset.
    pub help_scroll: usize,
    /// Demo mode: never persisted; station is a late-game preview.
    pub demo: bool,
    /// Generator row under the mouse, if any (SONAR selection explainer).
    pub hovered: Option<GeneratorId>,
}

impl<'a> UiState<'a> {
    /// A minimal settled UI snapshot (handy for tests and snapshots).
    #[cfg(test)]
    pub fn settled(
        view: StateView<'a>,
        palette: Palette,
        ascii: bool,
        screen: Screen,
        anim: &'a Anim,
    ) -> Self {
        Self {
            view,
            palette,
            ascii,
            screen,
            banner: None,
            offline: None,
            toasts: &[],
            anim,
            focus: Focus::None,
            confirm_descent: false,
            notice: None,
            archive_scroll: 0,
            settings: SettingsView::default(),
            save_path: "data/save.json",
            last_save: None,
            now_unix: 0,
            edit: None,
            help: false,
            help_scroll: 0,
            demo: false,
            hovered: None,
        }
    }
}

// ---------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------

/// Draw the whole UI and return the clickable regions drawn.
pub fn render(frame: &mut Frame, ui: &UiState<'_>) -> HitMap {
    let area = frame.area();
    if too_small(area) {
        render_too_small(frame, &ui.palette);
        return HitMap::default();
    }

    let mut hits = match ui.screen {
        Screen::Boot { revealed } => {
            crate::boot::render(
                frame,
                ui.view,
                &ui.palette,
                crate::glyphs::set(ui.ascii),
                ui.ascii,
                revealed,
            );
            HitMap::default()
        }
        Screen::Briefing => {
            crate::render::briefing::render(frame, ui);
            HitMap::default()
        }
        Screen::Sonar => sonar::render(frame, ui),
        Screen::Descent => descent::render(frame, ui),
        Screen::Archive => archive::render(frame, ui),
        Screen::System => system::render(frame, ui),
        Screen::Dive {
            from_m,
            to_m,
            progress,
        } => {
            dive::render(frame, ui, from_m, to_m, progress);
            HitMap::default()
        }
    };

    if let Some(report) = ui.offline {
        render_offline_summary(frame, report, ui.palette, ui.ascii);
    }
    if let Some(edit) = ui.edit {
        render_edit(frame, ui, edit);
    }
    if ui.help {
        let (total, visible) = help::render(frame, ui);
        hits.help_total = total;
        hits.help_visible = visible;
    }
    render_overlays(frame, ui);

    // Modals own the interaction surface; keep help metrics for scroll clamping.
    if ui.offline.is_some() || ui.edit.is_some() || ui.help || ui.confirm_descent {
        let total = hits.help_total;
        let visible = hits.help_visible;
        hits = HitMap::default();
        hits.help_total = total;
        hits.help_visible = visible;
    }
    hits
}

// ---------------------------------------------------------------------
// Shared visual vocabulary
// ---------------------------------------------------------------------

/// Whether the terminal is below the supported minimum.
pub fn too_small(area: Rect) -> bool {
    area.width < MIN_WIDTH || area.height < MIN_HEIGHT
}

/// Border set for the current glyph mode.
pub fn border_set(ascii: bool) -> border::Set<'static> {
    if ascii {
        border::Set {
            top_left: "+",
            top_right: "+",
            bottom_left: "+",
            bottom_right: "+",
            vertical_left: "|",
            vertical_right: "|",
            horizontal_top: "-",
            horizontal_bottom: "-",
        }
    } else {
        border::PLAIN
    }
}

/// A bordered console frame with left and right titles on the top border.
pub fn titled_frame(
    ascii: bool,
    palette: &Palette,
    left: Line<'static>,
    right: Line<'static>,
) -> Block<'static> {
    Block::bordered()
        .border_set(border_set(ascii))
        .style(Style::new().bg(palette.bg).fg(palette.text))
        .border_style(Style::new().fg(palette.dim))
        .title_top(left.alignment(Alignment::Left))
        .title_top(right.alignment(Alignment::Right))
}

/// Draw the screen tab strip and return its hit regions.
pub fn render_tabs(
    frame: &mut Frame,
    area: Rect,
    palette: &Palette,
    active: Option<Tab>,
) -> Vec<(Rect, Tab)> {
    const TABS: [(Tab, &str); 4] = [
        (Tab::Sonar, "1 SONAR"),
        (Tab::Descent, "2 DESCENT"),
        (Tab::Archive, "3 ARCHIVE"),
        (Tab::System, "4 SYSTEM"),
    ];
    let mut hits = Vec::with_capacity(TABS.len());
    let mut x = area.x;
    let end = area.x.saturating_add(area.width);
    for (tab, label) in TABS {
        let text = if active == Some(tab) {
            format!("[{label}]")
        } else {
            format!(" {label} ")
        };
        let width = text.chars().count() as u16;
        if x.saturating_add(width) > end || area.height == 0 {
            break;
        }
        let rect = Rect::new(x, area.y, width, 1);
        let style = if active == Some(tab) {
            Style::new().fg(palette.amber).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(palette.dim)
        };
        frame.render_widget(Paragraph::new(Line::from(Span::styled(text, style))), rect);
        hits.push((rect, tab));
        x = x.saturating_add(width).saturating_add(1);
    }
    hits
}

/// A horizontal separator that joins the frame's side borders.
pub fn render_rule(frame: &mut Frame, area: Rect, palette: &Palette, glyphs: &Glyphs) {
    if area.width < 2 || area.height == 0 {
        return;
    }
    // Expand by one cell on each side so the `├`/`┤` junctions replace the
    // frame's vertical border cells instead of sitting next to them.
    let width = area.width.saturating_add(2);
    let x = area.x.saturating_sub(1);
    let inner = width.saturating_sub(2) as usize;
    let line: String = std::iter::repeat_n(glyphs.h_line, inner).collect();
    let text = format!("{}{}{}", glyphs.l_cross, line, glyphs.r_cross);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, Style::new().fg(palette.dim)))),
        Rect::new(x, area.y, width, 1),
    );
}

/// A one-row progress bar.
pub fn bar(fraction: f64, width: usize, glyphs: &Glyphs) -> String {
    if width == 0 {
        return String::new();
    }
    let filled = ((fraction.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    std::iter::repeat_n(glyphs.bar, filled)
        .chain(std::iter::repeat_n(glyphs.bar_empty, width - filled))
        .collect()
}

/// Human-readable amount (floored).
pub fn amount(value: Decimal) -> String {
    to_human_floor(value)
}

/// Human-readable cost (ceiled).
pub fn cost(value: Decimal) -> String {
    to_human_ceil(value)
}

/// Rate string; uses the rate formatter so sub-1 values keep two decimals.
pub fn rate_text(value: Decimal) -> String {
    to_rate(value)
}

/// Thousands-separated integer.
pub fn count(value: u32) -> String {
    crate::model::format_count(value)
}

/// Start index of a scroll window that keeps `focus` visible.
pub fn window_start(count: usize, visible: usize, focus: Option<usize>) -> usize {
    if visible == 0 || count <= visible {
        return 0;
    }
    match focus {
        Some(focus) if focus >= visible => (focus + 1 - visible).min(count - visible),
        _ => 0,
    }
}

/// Keep the tail of a long string (for paths and buffers).
pub fn fit_tail(text: &str, width: usize, ascii: bool) -> String {
    let length = text.chars().count();
    if length <= width {
        return text.to_owned();
    }
    if width <= 2 {
        return text
            .chars()
            .rev()
            .take(width)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
    }
    let marker = if ascii { ".." } else { "…" };
    let keep = width - marker.chars().count();
    let tail: String = text.chars().skip(length - keep).collect();
    format!("{marker}{tail}")
}

/// One dim selection explainer line for the focused upgrade/hull or hovered generator.
pub fn selection_line(ui: &UiState<'_>) -> Option<String> {
    if let Some(line) = crate::model::selection_line(ui.view, ui.focus, ui.ascii) {
        return Some(line);
    }
    if matches!(ui.screen, Screen::Sonar) {
        return ui
            .hovered
            .map(|id| crate::model::generator_selection(ui.view, id));
    }
    None
}

/// Draw the focused selection's plain-words effect and exact cost.
pub fn render_selection(frame: &mut Frame, area: Rect, ui: &UiState<'_>) {
    let Some(text) = selection_line(ui) else {
        return;
    };
    let text = format!("  {text}");
    let shown = fit_tail(&text, area.width as usize, ui.ascii);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            shown,
            Style::new().fg(ui.palette.dim),
        ))),
        area,
    );
}

/// Relative save age for SYSTEM.
pub fn save_age(now_unix: u64, saved_at: u64) -> String {
    let age = now_unix.saturating_sub(saved_at);
    if age < 60 {
        format!("{age}s ago")
    } else if age < 3600 {
        format!("{}m ago", age / 60)
    } else {
        format!("{}h ago", age / 3600)
    }
}

// ---------------------------------------------------------------------
// Too-small
// ---------------------------------------------------------------------

/// The too-small screen: dimensions only, no partial layout.
pub fn render_too_small(frame: &mut Frame, palette: &Palette) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(palette.bg)), area);
    let lines = vec![
        Line::from(Span::styled(
            "TERMINAL TOO SMALL",
            Style::new().fg(palette.alert).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("current  {}x{}", area.width, area.height),
            Style::new().fg(palette.text),
        )),
        Line::from(Span::styled(
            format!("required {MIN_WIDTH}x{MIN_HEIGHT}"),
            Style::new().fg(palette.dim),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "resize the terminal, or press q to quit",
            Style::new().fg(palette.dim),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(Style::new().bg(palette.bg)),
        area,
    );
}

// ---------------------------------------------------------------------
// Overlays
// ---------------------------------------------------------------------

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

/// A centered, bordered overlay box; returns its inner area.
pub fn overlay_box(
    frame: &mut Frame,
    area: Rect,
    width: u16,
    height: u16,
    palette: Palette,
    ascii: bool,
    title: &str,
) -> Rect {
    let popup = centered(area, width, height);
    clear_overlay_band(frame, area, popup, palette, ascii);
    frame.render_widget(ratatui::widgets::Clear, popup);
    let block = Block::bordered()
        .border_set(border_set(ascii))
        .style(Style::new().bg(palette.panel).fg(palette.text))
        .border_style(Style::new().fg(palette.amber))
        .title_top(Line::from(Span::styled(
            format!(" {title} "),
            Style::new().fg(palette.amber).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    inner
}

/// Erase the console contents behind an overlay band, edge to edge, then
/// restore the outer frame's vertical border so underlying rule junctions
/// cannot survive at the frame edge. The frame border itself is preserved.
fn clear_overlay_band(frame: &mut Frame, area: Rect, popup: Rect, palette: Palette, ascii: bool) {
    if popup.width == 0 || popup.height == 0 || area.width < 2 {
        return;
    }
    let band_width = area.width.saturating_sub(2);
    if band_width > 0 {
        let filler = " ".repeat(band_width as usize);
        let style = Style::new().bg(palette.bg);
        let mut lines = Vec::with_capacity(popup.height as usize);
        for _ in 0..popup.height {
            lines.push(Line::from(Span::styled(filler.clone(), style)));
        }
        frame.render_widget(
            Paragraph::new(lines),
            Rect::new(area.x.saturating_add(1), popup.y, band_width, popup.height),
        );
    }

    let sides = border_set(ascii);
    let side_style = Style::new().fg(palette.dim);
    let right_x = area.x.saturating_add(area.width.saturating_sub(1));
    let end_y = popup.y.saturating_add(popup.height);
    for y in popup.y..end_y {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(sides.vertical_left, side_style))),
            Rect::new(area.x, y, 1, 1),
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(sides.vertical_right, side_style))),
            Rect::new(right_x, y, 1, 1),
        );
    }
}

fn render_edit(frame: &mut Frame, ui: &UiState<'_>, edit: EditView<'_>) {
    let area = frame.area();
    let palette = ui.palette;
    let width = area.width.saturating_sub(8).clamp(40, 72);
    let inner = overlay_box(frame, area, width, 8, palette, ui.ascii, edit.kind.title());
    let text_width = inner.width.saturating_sub(3) as usize;
    let shown = fit_tail(edit.buffer, text_width, ui.ascii);
    let cursor = if ui.ascii { "|" } else { "▌" };
    let mut lines = vec![
        Line::from(Span::styled(
            edit.kind.prompt().to_owned(),
            Style::new().fg(palette.dim),
        )),
        Line::from(vec![
            Span::styled(" ", Style::new().fg(palette.text)),
            Span::styled(shown, Style::new().fg(palette.text)),
            Span::styled(
                cursor.to_owned(),
                Style::new().fg(palette.accent).add_modifier(Modifier::BOLD),
            ),
        ]),
    ];
    if let Some(error) = edit.error {
        lines.push(Line::from(Span::styled(
            error.to_owned(),
            Style::new().fg(palette.alert),
        )));
    } else {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter submit    Esc cancel",
        Style::new().fg(palette.dim),
    )));
    frame.render_widget(
        Paragraph::new(lines).style(Style::new().bg(palette.panel)),
        inner,
    );
}

fn render_offline_summary(
    frame: &mut Frame,
    report: &OfflineReport,
    palette: Palette,
    ascii: bool,
) {
    let area = frame.area();
    let inner = overlay_box(frame, area, 60, 11, palette, ascii, "WELCOME BACK");

    let cap_line = if report.cap_hit {
        format!(
            "{} credited (cap {} reached)",
            fmt_duration(report.credited_secs),
            fmt_duration(report.cap_secs)
        )
    } else {
        format!(
            "{} credited of {} elapsed, cap {}",
            fmt_duration(report.credited_secs),
            fmt_duration(report.requested_secs),
            fmt_duration(report.cap_secs)
        )
    };

    let lines = vec![
        kv_line("AWAY", &cap_line, palette),
        kv_line("ECHO", &format!("+{}", amount(report.echo_delta)), palette),
        kv_line("PURCHASES", &report.purchases.to_string(), palette),
        kv_line("DESCENTS", &report.descents.to_string(), palette),
        kv_line("FATHOMS", &format!("+{}", report.fathoms_gained), palette),
        kv_line(
            "ACHIEVEMENTS",
            &report.achievements.len().to_string(),
            palette,
        ),
        Line::from(""),
        Line::from(Span::styled(
            "any key continues",
            Style::new().fg(palette.dim),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines).style(Style::new().bg(palette.panel).fg(palette.text)),
        inner,
    );
}

fn kv_line(label: &str, value: &str, palette: Palette) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<13}"), Style::new().fg(palette.dim)),
        Span::styled(value.to_owned(), Style::new().fg(palette.text)),
    ])
}

fn render_overlays(frame: &mut Frame, ui: &UiState<'_>) {
    let area = frame.area();
    let palette = ui.palette;
    let mut bottom = area.y + area.height.saturating_sub(1);

    if let Some(text) = ui.banner
        && bottom > area.y
    {
        bottom -= 1;
        render_line_at(
            frame,
            area,
            bottom,
            text,
            Style::new()
                .bg(palette.alert)
                .fg(palette.bg)
                .add_modifier(Modifier::BOLD),
        );
    }
    if let Some(text) = ui.notice
        && bottom > area.y
        && !matches!(ui.screen, Screen::Descent)
    {
        bottom -= 1;
        render_line_at(
            frame,
            area,
            bottom,
            text,
            Style::new().bg(palette.panel).fg(palette.alert),
        );
    }
    for text in ui.toasts.iter().rev().take(3) {
        if bottom > area.y {
            bottom -= 1;
            render_line_at(
                frame,
                area,
                bottom,
                text,
                Style::new().bg(palette.panel).fg(palette.amber),
            );
        }
    }
}

fn render_line_at(frame: &mut Frame, area: Rect, y: u16, text: &str, style: Style) {
    let rect = Rect::new(area.x, y, area.width, 1);
    // Pad to the full row so a short message cannot leave stale footer cells
    // beside it.
    let padded = format!("{text:<width$}", width = area.width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(padded, style))).style(style),
        rect,
    );
}

/// Format a duration in seconds for the offline summary.
pub fn fmt_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}
