//! Terminal rendering with ratatui.

use crate::app::{App, Mode};
use crate::board::Cell;
use crate::generator::Difficulty;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

// Width of the info panel beside the board.
const INFO_W: u16 = 28;

/// Cell dimensions are chosen at draw time to fit the terminal. Cells are kept
/// 3 rows tall (so pencil marks show) for as long as possible — dropping the
/// thin gridlines between cells before shrinking the cells themselves.
struct CellSize {
    w: usize,
    h: usize,
    /// Whether thin gridlines are drawn between cells within a box. When false,
    /// only the heavy box borders are drawn, which saves vertical space.
    inner_lines: bool,
}

impl CellSize {
    /// Pick the densest layout that still fits `area`, preferring tall (3-row)
    /// cells so notes remain visible.
    fn fit(area: Rect) -> CellSize {
        let avail = area.height as i32;
        // Candidates from richest to most compact; each prefers keeping notes.
        for &(h, inner) in &[
            (3, true),
            (3, false),
            (2, true),
            (2, false),
            (1, true),
            (1, false),
        ] {
            let border_rows = if inner { 10 } else { 4 };
            if avail >= 9 * h + border_rows {
                return CellSize::with_width(h, inner, area.width);
            }
        }
        // Terminal is shorter than any full board; use the smallest and clip.
        CellSize::with_width(1, false, area.width)
    }

    fn with_width(h: i32, inner_lines: bool, area_w: u16) -> CellSize {
        let max_w = (((area_w as i32 - 10) / 9).clamp(3, 7)) as usize;
        CellSize {
            w: 5.min(max_w),
            h: h as usize,
            inner_lines,
        }
    }

    fn board_w(&self) -> u16 {
        (9 * self.w + 10) as u16
    }

    fn board_h(&self) -> u16 {
        let border_rows = if self.inner_lines { 10 } else { 4 };
        (9 * self.h + border_rows) as u16
    }

    /// Whether cells are tall enough for the 3x3 pencil-mark grid.
    fn shows_notes(&self) -> bool {
        self.h >= 3
    }
}

// Palette.
const BORDER: Color = Color::Rgb(110, 110, 120);
const GIVEN: Color = Color::Rgb(235, 235, 235);
const USER: Color = Color::Rgb(120, 200, 255);
const NOTE: Color = Color::Rgb(130, 130, 140);
const WRONG: Color = Color::Rgb(240, 90, 90);
const BG_SELECT: Color = Color::Rgb(60, 90, 140);
const BG_SELECT_NOTE: Color = Color::Rgb(120, 100, 50);
const BG_PEER: Color = Color::Rgb(40, 42, 54);
const BG_SAME: Color = Color::Rgb(70, 68, 40);
const BG_CONFLICT: Color = Color::Rgb(90, 40, 45);

/// Convert HSV (hue in degrees, sat/val in 0..=1) to an RGB terminal color.
/// Used by the win animation's flowing rainbow.
fn hsv(h: f32, s: f32, v: f32) -> Color {
    let h = h.rem_euclid(360.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color::Rgb(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// The win animation's background tint for cell (r, c) at time `t` seconds since
/// solving: a rainbow that flows along the board's diagonal, lit by a bright
/// band that sweeps across it on a loop.
fn win_cell_bg(r: usize, c: usize, t: f32) -> Color {
    let diag = (r + c) as f32; // 0..=16, constant along anti-diagonals
    let hue = diag * 22.0 + t * 130.0;
    // A bright band sweeps the diagonal (0..16) then gaps before repeating.
    let front = (t * 7.0).rem_euclid(26.0) - 5.0;
    let pulse = (1.0 - (diag - front).abs() / 3.0).clamp(0.0, 1.0);
    hsv(hue, 0.85, 0.26 + 0.55 * pulse)
}

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.size();
    let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
    let body = rows[0];

    // Size cells to the available space so the board never overflows.
    let cell = CellSize::fit(body);
    let board_w = cell.board_w();
    let board_h = cell.board_h();

    // Show the info panel only when there's horizontal room for it.
    let show_panel = body.width >= board_w + 2 + INFO_W;
    let content_w = if show_panel { board_w + 2 + INFO_W } else { board_w };
    let content = centered_rect(content_w, board_h, body);

    if show_panel {
        let cols = Layout::horizontal([
            Constraint::Length(board_w),
            Constraint::Length(2),
            Constraint::Length(INFO_W),
        ])
        .split(content);
        draw_board(f, app, cols[0], &cell);
        draw_info(f, app, cols[2]);
    } else {
        draw_board(f, app, content, &cell);
    }
    draw_status_bar(f, app, rows[1]);

    if let Some(elapsed) = app.win_anim_elapsed() {
        draw_win_banner(f, app, body, elapsed.as_secs_f32());
    }
    if app.show_help {
        draw_help(f, area);
    }
    if app.difficulty_menu {
        draw_difficulty_menu(f, app, area);
    }
}

/// The character at the intersection of horizontal line `hi` and vertical line
/// `vi` (both 0..=9). Lines on a 3-boundary are heavy; the rest are light.
fn junction(hi: usize, vi: usize) -> char {
    let hh = hi % 3 == 0; // horizontal line heavy?
    let vh = vi % 3 == 0; // vertical line heavy?
    let (north, south, west, east) = (hi > 0, hi < 9, vi > 0, vi < 9);
    match (north, south, west, east) {
        (false, true, false, true) => '┏',
        (false, true, true, false) => '┓',
        (true, false, false, true) => '┗',
        (true, false, true, false) => '┛',
        (false, true, true, true) => {
            if vh {
                '┳'
            } else {
                '┯'
            }
        }
        (true, false, true, true) => {
            if vh {
                '┻'
            } else {
                '┷'
            }
        }
        (true, true, false, true) => {
            if hh {
                '┣'
            } else {
                '┠'
            }
        }
        (true, true, true, false) => {
            if hh {
                '┫'
            } else {
                '┨'
            }
        }
        (true, true, true, true) => match (vh, hh) {
            (false, false) => '┼',
            (false, true) => '┿',
            (true, false) => '╂',
            (true, true) => '╋',
        },
        _ => ' ',
    }
}

/// Build the horizontal gridline for horizontal-line index `hi`.
fn border_line(hi: usize, cw: usize) -> Line<'static> {
    let seg = if hi % 3 == 0 { '━' } else { '─' };
    let mut s = String::with_capacity(9 * cw + 10);
    for vi in 0..=9 {
        s.push(junction(hi, vi));
        if vi < 9 {
            for _ in 0..cw {
                s.push(seg);
            }
        }
    }
    Line::from(Span::styled(s, Style::default().fg(BORDER)))
}

/// The `cw` glyphs for sub-row `sub` of a cell: a centered value, the pencil
/// marks, or blanks.
fn cell_glyphs(cell: &Cell, size: &CellSize, sub: usize) -> String {
    let mut out = vec![' '; size.w];
    if let Some(v) = cell.value {
        if sub == size.h / 2 {
            out[size.w / 2] = (b'0' + v) as char;
        }
    } else if cell.notes != 0 {
        if size.shows_notes() {
            // Notes 1-3 / 4-6 / 7-9 sit on rows 0/1/2; spread across the width
            // when there's room, otherwise packed to the left.
            let cols: [usize; 3] = if size.w >= 5 { [0, 2, 4] } else { [0, 1, 2] };
            for k in 0..3 {
                let n = (sub * 3 + k + 1) as u8;
                if cell.has_note(n) {
                    out[cols[k]] = (b'0' + n) as char;
                }
            }
        } else if sub == size.h / 2 {
            // Too short for the grid: list the candidates, centered and clipped.
            let cand: Vec<char> = (1..=9)
                .filter(|&n| cell.has_note(n))
                .map(|n| (b'0' + n) as char)
                .collect();
            let shown = cand.len().min(size.w);
            let start = (size.w - shown) / 2;
            out[start..start + shown].copy_from_slice(&cand[..shown]);
        }
    }
    out.into_iter().collect()
}

/// Background color for a cell given the cursor and game state.
fn cell_bg(app: &App, r: usize, c: usize) -> Option<Color> {
    // The win animation washes over every cell, replacing the usual highlights.
    if let Some(elapsed) = app.win_anim_elapsed() {
        return Some(win_cell_bg(r, c, elapsed.as_secs_f32()));
    }
    let (cr, cc) = app.cursor;
    if (r, c) == (cr, cc) {
        return Some(match app.mode {
            Mode::Note => BG_SELECT_NOTE,
            _ => BG_SELECT,
        });
    }
    if app.board.has_conflict(r, c) || app.is_wrong(r, c) {
        return Some(BG_CONFLICT);
    }
    let cursor_val = app.board.value(cr, cc);
    if cursor_val.is_some() && app.board.value(r, c) == cursor_val {
        return Some(BG_SAME);
    }
    // With match-highlight on, light up the unit of *every* cell holding the
    // cursor's value; otherwise just the cursor's own row/column/box.
    let peer = match (app.highlight_matches, cursor_val) {
        (true, Some(v)) => unit_contains(app, r, c, v),
        _ => r == cr || c == cc || (r / 3 == cr / 3 && c / 3 == cc / 3),
    };
    if peer {
        return Some(BG_PEER);
    }
    None
}

/// Whether the row, column, or 3x3 box of cell (r, c) contains the value `v`
/// anywhere. Used by match-highlight to span all units holding a digit.
fn unit_contains(app: &App, r: usize, c: usize, v: u8) -> bool {
    let board = &app.board;
    for i in 0..9 {
        if board.value(r, i) == Some(v) || board.value(i, c) == Some(v) {
            return true;
        }
    }
    let (br, bc) = (r / 3 * 3, c / 3 * 3);
    for rr in br..br + 3 {
        for cc in bc..bc + 3 {
            if board.value(rr, cc) == Some(v) {
                return true;
            }
        }
    }
    false
}

fn draw_board(f: &mut Frame, app: &App, area: Rect, size: &CellSize) {
    let mut lines: Vec<Line> = Vec::with_capacity(size.board_h() as usize);
    for r in 0..9 {
        // Box borders (rows 0/3/6) are always drawn; thin inner borders only
        // when there's room.
        if r % 3 == 0 || size.inner_lines {
            lines.push(border_line(r, size.w));
        }
        for sub in 0..size.h {
            lines.push(content_line(app, r, sub, size));
        }
    }
    lines.push(border_line(9, size.w));
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Left), area);
}

/// One text row (`sub` of 0..size.h) across all nine cells of board-row `r`.
fn content_line(app: &App, r: usize, sub: usize, size: &CellSize) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::with_capacity(19);
    for c in 0..9 {
        let sep = if c % 3 == 0 { '┃' } else { '│' };
        spans.push(Span::styled(sep.to_string(), Style::default().fg(BORDER)));

        let cell = app.board.cell(r, c);
        let glyphs = cell_glyphs(cell, size, sub);

        let fg = if cell.value.is_some() {
            if app.board.has_conflict(r, c) || app.is_wrong(r, c) {
                WRONG
            } else if cell.given {
                GIVEN
            } else {
                USER
            }
        } else {
            NOTE
        };

        // During the win sweep, force bold white digits so they stay crisp over
        // the shifting rainbow rather than blending into it.
        let fg = if app.is_won() && cell.value.is_some() {
            Color::Rgb(255, 255, 255)
        } else {
            fg
        };

        let mut style = Style::default().fg(fg);
        if let Some(bg) = cell_bg(app, r, c) {
            style = style.bg(bg);
        }
        if (cell.given || app.is_won()) && cell.value.is_some() {
            style = style.add_modifier(Modifier::BOLD);
        }
        spans.push(Span::styled(glyphs, style));
    }
    spans.push(Span::styled("┃", Style::default().fg(BORDER)));
    Line::from(spans)
}

fn draw_info(f: &mut Frame, app: &App, area: Rect) {
    let mode = match app.mode {
        Mode::Normal => Span::styled("NORMAL", Style::default().fg(Color::Green)),
        Mode::Note => Span::styled("NOTE", Style::default().fg(Color::Yellow)),
    };
    let secs = app.elapsed().as_secs();
    let timer = format!("{:02}:{:02}", secs / 60, secs % 60);
    let label = |s: &str| Span::styled(format!("{:<11}", s), Style::default().fg(Color::Gray));

    let mut lines = vec![
        Line::from(vec![label("Difficulty"), Span::raw(app.difficulty.label())]),
        Line::from(vec![label("Mode"), mode]),
        Line::from(vec![label("Time"), Span::raw(timer)]),
        Line::from(vec![
            label("Remaining"),
            Span::raw(app.board.empty_count().to_string()),
        ]),
    ];
    if app.check_errors {
        lines.push(Line::from(Span::styled(
            "error-check ON",
            Style::default().fg(WRONG),
        )));
    }
    if app.highlight_matches {
        lines.push(Line::from(Span::styled(
            "match-highlight ON",
            Style::default().fg(Color::Rgb(180, 170, 90)),
        )));
    }

    // How many of each digit are still to be placed.
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Numbers left",
        Style::default().fg(Color::Gray),
    )));
    for row in 0..3 {
        let mut spans = Vec::with_capacity(3);
        for col in 0..3 {
            let n = (row * 3 + col + 1) as u8;
            let left = 9 - app.board.count_value(n);
            let style = if left == 0 {
                Style::default()
                    .fg(Color::Rgb(90, 140, 90))
                    .add_modifier(Modifier::DIM)
            } else {
                Style::default().fg(Color::Rgb(200, 200, 210))
            };
            let text = if left == 0 {
                format!(" {}:✓ ", n)
            } else {
                format!(" {}:{} ", n, left)
            };
            spans.push(Span::styled(text, style));
        }
        lines.push(Line::from(spans));
    }

    // What's still missing from the selected cell's row, column, and box.
    let (cr, cc) = app.cursor;
    let unit_line = |name: &str, missing: Vec<u8>| {
        let label = Span::styled(format!("{:<5}", name), Style::default().fg(Color::Gray));
        let body = if missing.is_empty() {
            Span::styled("✓", Style::default().fg(Color::Rgb(90, 140, 90)))
        } else {
            let digits: Vec<String> = missing.iter().map(|n| n.to_string()).collect();
            Span::styled(digits.join(" "), Style::default().fg(Color::Rgb(200, 200, 210)))
        };
        Line::from(vec![label, body])
    };
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Missing here",
        Style::default().fg(Color::Gray),
    )));
    lines.push(unit_line("Row", app.board.missing_in_row(cr)));
    lines.push(unit_line("Col", app.board.missing_in_col(cc)));
    lines.push(unit_line("Box", app.board.missing_in_box(cr, cc)));

    if app.is_won() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "★ Solved! ★",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Sudoku ")
        .border_style(Style::default().fg(BORDER));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let hint =
        "hjkl move  1-9 set  f find  m note  x clear  u undo  H hint  v match  d difficulty  ? help  q quit";
    let text = if app.status.is_empty() {
        hint.to_string()
    } else {
        format!("{}  —  {}", app.status, hint)
    };
    let para = Paragraph::new(Line::from(Span::styled(
        text,
        Style::default().fg(Color::Rgb(160, 160, 170)),
    )));
    f.render_widget(para, area);
}

/// A celebratory banner that pops in over the board once the puzzle is solved.
/// `t` is seconds since solving; the banner waits for the sweep to wash over the
/// board, then grows into place with a rainbow border that keeps cycling.
fn draw_win_banner(f: &mut Frame, app: &App, area: Rect, t: f32) {
    const DELAY: f32 = 0.45; // let the rainbow sweep land first
    const GROW: f32 = 0.35; // how long the box takes to unfold
    if t < DELAY {
        return;
    }

    // Eased 0..1 progress for the pop-in.
    let p = ((t - DELAY) / GROW).clamp(0.0, 1.0);
    let ease = 1.0 - (1.0 - p) * (1.0 - p);

    let secs = app.elapsed().as_secs();
    let lines = vec![
        Line::from(Span::styled(
            "✦  S O L V E D  ✦",
            Style::default()
                .fg(Color::Rgb(255, 255, 255))
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        Line::from(""),
        Line::from(Span::styled(
            format!("{} in {:02}:{:02}", app.difficulty.label(), secs / 60, secs % 60),
            Style::default().fg(Color::Rgb(220, 220, 230)),
        ))
        .alignment(Alignment::Center),
        Line::from(Span::styled(
            "n new game   q quit",
            Style::default().fg(Color::Rgb(150, 150, 160)),
        ))
        .alignment(Alignment::Center),
    ];

    // Unfold from a sliver to full size as the pop-in eases in.
    let full_w: u16 = 30;
    let full_h: u16 = 6;
    let w = (full_w as f32 * ease).round().max(1.0) as u16;
    let h = (full_h as f32 * (0.4 + 0.6 * ease)).round().max(1.0) as u16;
    let popup = centered_rect(w, h, area);

    // Border hue cycles, matching the board's flowing rainbow.
    let border = hsv(t * 130.0, 0.8, 0.95);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border).add_modifier(Modifier::BOLD));
    f.render_widget(Clear, popup);
    // Only spill the text once the box is mostly open, so it doesn't overflow
    // the sliver mid-unfold.
    if ease > 0.6 {
        f.render_widget(Paragraph::new(lines).block(block), popup);
    } else {
        f.render_widget(block, popup);
    }
}

fn draw_difficulty_menu(f: &mut Frame, app: &App, area: Rect) {
    let mut lines = vec![
        Line::from(Span::styled(
            "Select difficulty",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    for (i, d) in Difficulty::ALL.iter().enumerate() {
        let selected = i == app.menu_index;
        let text = format!(" {} {}. {} ", if selected { '>' } else { ' ' }, i + 1, d.label());
        let style = if selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(Span::styled(text, style)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "j/k move  1-4 pick  Enter ok  Esc cancel",
        Style::default().fg(Color::Gray),
    )));

    let popup = centered_rect(44, 10, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    f.render_widget(Clear, popup);
    f.render_widget(Paragraph::new(lines).block(block), popup);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            " Keybindings ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  h j k l      move cursor"),
        Line::from("  w b          jump box right / left"),
        Line::from("  0 $          start / end of row"),
        Line::from("  g g  /  G     top / bottom"),
        Line::from("  f 1-9        jump to nearest of that number"),
        Line::from("  1-9          place value (or toggle note)"),
        Line::from("  m            toggle note mode"),
        Line::from("  x  /  Del     clear cell"),
        Line::from("  u  /  Ctrl-r  undo / redo"),
        Line::from("  H            reveal current cell (hint)"),
        Line::from("  c            toggle error checking"),
        Line::from("  v            highlight all units with this digit"),
        Line::from("  d            choose difficulty"),
        Line::from("  n  /  N       new game / cycle difficulty"),
        Line::from("  ?            toggle this help"),
        Line::from("  q            quit"),
    ];
    let popup = centered_rect(46, 20, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;

    #[test]
    fn match_highlight_spans_every_units_with_the_digit() {
        let mut app = App::new(Difficulty::Easy);
        // Controlled board: two 9s in unrelated row/col/box.
        app.board = Board::new();
        app.board.set_value(0, 0, 9);
        app.board.set_value(4, 4, 9);
        app.cursor = (0, 0);

        // (4, 8) shares row 4 with the second 9 but none of the cursor's units.
        // Off: not highlighted. On: highlighted as a peer.
        app.highlight_matches = false;
        assert_eq!(cell_bg(&app, 4, 8), None);
        app.highlight_matches = true;
        assert_eq!(cell_bg(&app, 4, 8), Some(BG_PEER));

        // The other 9 itself stays a same-value match, and the cursor stays selected.
        assert_eq!(cell_bg(&app, 4, 4), Some(BG_SAME));
        assert_eq!(cell_bg(&app, 0, 0), Some(BG_SELECT));

        // A cell touching no 9's unit is left alone even with highlight on.
        assert_eq!(cell_bg(&app, 7, 2), None);
    }
}

/// A fixed-size rectangle centered within `area`.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}
