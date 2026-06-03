//! Terminal rendering with ratatui.

use crate::app::{App, Mode};
use crate::board::Cell;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

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
    let cols = Layout::horizontal([Constraint::Length(37), Constraint::Min(22)]).split(rows[0]);

    draw_board(f, app, cols[0]);
    draw_info(f, app, cols[1]);
    draw_status_bar(f, app, rows[1]);

    if app.show_help {
        draw_help(f, area);
    }
}

/// The character to draw at the intersection of box-border row `hi` (0..=9) and
/// vertical line `vi` (0..=9). Horizontal box lines are always heavy; vertical
/// lines are heavy on box boundaries (every 3rd) and light otherwise.
fn junction(hi: usize, vi: usize) -> char {
    let vert_heavy = vi % 3 == 0;
    let (north, south, west, east) = (hi > 0, hi < 9, vi > 0, vi < 9);
    match (north, south, west, east) {
        (false, true, false, true) => '┏',
        (false, true, true, false) => '┓',
        (true, false, false, true) => '┗',
        (true, false, true, false) => '┛',
        (false, true, true, true) => {
            if vert_heavy {
                '┳'
            } else {
                '┯'
            }
        }
        (true, false, true, true) => {
            if vert_heavy {
                '┻'
            } else {
                '┷'
            }
        }
        (true, true, false, true) => '┣',
        (true, true, true, false) => '┫',
        (true, true, true, true) => {
            if vert_heavy {
                '╋'
            } else {
                '┿'
            }
        }
        _ => ' ',
    }
}

/// Build a heavy horizontal box-border line for horizontal-line index `hi`.
fn border_line(hi: usize) -> Line<'static> {
    let mut s = String::with_capacity(37);
    for vi in 0..=9 {
        s.push(junction(hi, vi));
        if vi < 9 {
            s.push_str("━━━");
        }
    }
    Line::from(Span::styled(s, Style::default().fg(BORDER)))
}

/// The three glyphs for sub-row `sub` (0..3) of a cell: a centered value, or the
/// pencil marks at their fixed positions, or blanks.
fn cell_glyphs(cell: &Cell, sub: usize) -> [char; 3] {
    if let Some(v) = cell.value {
        if sub == 1 {
            return [' ', (b'0' + v) as char, ' '];
        }
        return [' ', ' ', ' '];
    }
    let mut out = [' '; 3];
    for k in 0..3 {
        let n = (sub * 3 + k + 1) as u8;
        if cell.has_note(n) {
            out[k] = (b'0' + n) as char;
        }
    }
    out
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

fn draw_board(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::with_capacity(31);

    for box_row in 0..3 {
        lines.push(border_line(box_row * 3));
        for inner in 0..3 {
            let r = box_row * 3 + inner;
            for sub in 0..3 {
                lines.push(content_line(app, r, sub));
            }
        }
    }
    lines.push(border_line(9));

    let para = Paragraph::new(lines).alignment(Alignment::Left);
    f.render_widget(para, area);
}

/// One text row (`sub` of 0..3) across all nine cells of board-row `r`.
fn content_line(app: &App, r: usize, sub: usize) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::with_capacity(19);
    for c in 0..9 {
        // Vertical separator to the left of this cell.
        let sep = if c % 3 == 0 { '┃' } else { '│' };
        spans.push(Span::styled(sep.to_string(), Style::default().fg(BORDER)));

        let cell = app.board.cell(r, c);
        let glyphs: String = cell_glyphs(cell, sub).iter().collect();

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

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Difficulty  ", Style::default().fg(Color::Gray)),
            Span::raw(app.difficulty.label()),
        ]),
        Line::from(vec![Span::styled("Mode        ", Style::default().fg(Color::Gray)), mode]),
        Line::from(vec![
            Span::styled("Time        ", Style::default().fg(Color::Gray)),
            Span::raw(timer),
        ]),
        Line::from(vec![
            Span::styled("Remaining   ", Style::default().fg(Color::Gray)),
            Span::raw(app.board.empty_count().to_string()),
        ]),
    ];
    if app.check_errors {
        lines.push(Line::from(Span::styled(
            "error-check ON",
            Style::default().fg(WRONG),
        )));
    }
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
    let hint = "hjkl move  1-9 set  m note  x clear  u undo  H hint  ? help  q quit";
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
        Line::from("  n  /  N       new game / cycle difficulty"),
        Line::from("  ?            toggle this help"),
        Line::from("  q            quit"),
    ];
    let popup = centered_rect(46, 19, area);
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
