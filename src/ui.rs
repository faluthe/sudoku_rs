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
const BG_PEER: Color = Color::Rgb(40, 42, 54);
const BG_SAME: Color = Color::Rgb(70, 68, 40);
const BG_CONFLICT: Color = Color::Rgb(90, 40, 45);

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
    let (cr, cc) = app.cursor;
    if (r, c) == (cr, cc) {
        return Some(BG_SELECT);
    }
    if app.board.has_conflict(r, c) || app.is_wrong(r, c) {
        return Some(BG_CONFLICT);
    }
    let cursor_val = app.board.value(cr, cc);
    if cursor_val.is_some() && app.board.value(r, c) == cursor_val {
        return Some(BG_SAME);
    }
    let peer = r == cr || c == cc || (r / 3 == cr / 3 && c / 3 == cc / 3);
    if peer {
        return Some(BG_PEER);
    }
    None
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

        let mut style = Style::default().fg(fg);
        if let Some(bg) = cell_bg(app, r, c) {
            style = style.bg(bg);
        }
        if cell.given && cell.value.is_some() {
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
    let hint = "hjkl move  1-9 set  m note  x clear  u undo  H hint  d difficulty  ? help  q quit";
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
        Line::from("  1-9          place value (or toggle note)"),
        Line::from("  m            toggle note mode"),
        Line::from("  x  /  Del     clear cell"),
        Line::from("  u  /  Ctrl-r  undo / redo"),
        Line::from("  H            reveal current cell (hint)"),
        Line::from("  c            toggle error checking"),
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

/// A fixed-size rectangle centered within `area`.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}
