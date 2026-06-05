mod app;
mod board;
mod generator;
mod ui;

use app::{App, Direction, Mode};
use crossterm::{
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use generator::Difficulty;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};
use std::time::Duration;

fn main() -> io::Result<()> {
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal);
    restore_terminal(&mut terminal)?;
    result
}

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn setup_terminal() -> io::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // Bracketed paste lets a pasted puzzle code arrive as one event instead of
    // a stream of keypresses (whose embedded newline would submit early).
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;

    // Make sure a panic doesn't leave the user's terminal in raw/alt-screen mode.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen);
        default_hook(info);
    }));

    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Tui) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), DisableBracketedPaste, LeaveAlternateScreen)?;
    terminal.show_cursor()
}

fn run(terminal: &mut Tui) -> io::Result<()> {
    let mut app = App::new(Difficulty::Easy);
    // Pending prefixes for the `gg` motion and the `f<digit>` find motion.
    let mut pending = Pending::default();
    // Held open for the whole session: on X11/Wayland the clipboard contents
    // are served by the owning process, so this must outlive each copy.
    let mut clipboard = arboard::Clipboard::new().ok();

    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, &app))?;

        // While the win animation plays, redraw at ~30fps for a smooth sweep;
        // otherwise poll lazily, just often enough to keep the timer ticking.
        let tick = if app.is_won() {
            Duration::from_millis(33)
        } else {
            Duration::from_millis(250)
        };
        // Poll so the on-screen timer keeps ticking without input.
        if event::poll(tick)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    handle_key(&mut app, key, &mut pending);
                }
                // Pasted text only matters while entering a puzzle code.
                Event::Paste(text) => app.code_entry_push(&text),
                _ => {}
            }
        }

        // The share card asks the frontend to put its text on the system
        // clipboard; do it here where we own the terminal's stdout.
        if app.copy_requested {
            app.copy_requested = false;
            let copied = copy_to_clipboard(clipboard.as_mut(), &app.share_text());
            app.status = if copied {
                "Copied to clipboard".into()
            } else {
                "Couldn't copy to clipboard".into()
            };
        }
    }
    Ok(())
}

/// Put `text` on the system clipboard. Prefers a real clipboard via arboard
/// (which works in terminals like GNOME Terminal that ignore OSC 52), and falls
/// back to the OSC 52 escape for remote/SSH sessions with no reachable display.
fn copy_to_clipboard(clipboard: Option<&mut arboard::Clipboard>, text: &str) -> bool {
    if let Some(c) = clipboard {
        if c.set_text(text).is_ok() {
            return true;
        }
    }
    copy_osc52(text).is_ok()
}

/// Copy via the OSC 52 terminal escape — a no-dependency fallback that some
/// terminals (and many over SSH) honor.
fn copy_osc52(text: &str) -> io::Result<()> {
    use std::io::Write;
    let mut out = io::stdout();
    write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?;
    out.flush()
}

/// Minimal standard-alphabet base64 encoder (OSC 52 wants base64 payloads).
fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | (*chunk.get(2).unwrap_or(&0) as u32);
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// Tracks multi-key motions awaiting their second keypress.
#[derive(Default)]
struct Pending {
    /// A `g` was pressed; the next `g` jumps to the top.
    g: bool,
    /// An `f` was pressed; the next digit jumps to the nearest cell with it.
    find: bool,
}

fn handle_key(app: &mut App, key: KeyEvent, pending: &mut Pending) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    app.status.clear();

    // The code-entry prompt is modal: type or paste a code, Enter loads it.
    if app.code_entry.is_some() {
        match key.code {
            KeyCode::Enter => app.code_entry_submit(),
            KeyCode::Esc => app.cancel_code_entry(),
            KeyCode::Backspace => app.code_entry_backspace(),
            KeyCode::Char(c) => app.code_entry_push(&c.to_string()),
            _ => {}
        }
        return;
    }

    // The share card is modal: y re-copies, Esc/s/q closes it.
    if app.show_share {
        match key.code {
            KeyCode::Char('y') => app.copy_requested = true,
            KeyCode::Esc | KeyCode::Char('s') | KeyCode::Char('q') => app.close_share(),
            _ => {}
        }
        return;
    }

    // The difficulty menu is modal: it swallows all input while open.
    if app.difficulty_menu {
        match key.code {
            KeyCode::Esc | KeyCode::Char('d') | KeyCode::Char('q') => app.close_difficulty_menu(),
            KeyCode::Char('j') | KeyCode::Down => app.menu_move(1),
            KeyCode::Char('k') | KeyCode::Up => app.menu_move(-1),
            KeyCode::Enter | KeyCode::Char('l') => app.menu_confirm(),
            KeyCode::Char(c @ '1'..='4') => {
                app.menu_select((c as u8 - b'1') as usize);
                app.menu_confirm();
            }
            _ => {}
        }
        return;
    }

    // Resolve a pending `f` prefix: `f<digit>` jumps to the nearest match.
    if pending.find {
        pending.find = false;
        if let KeyCode::Char(c @ '1'..='9') = key.code {
            app.find_nearest(c as u8 - b'0');
        }
        return;
    }

    // Resolve a pending `g` prefix: `gg` jumps to the top row.
    if pending.g {
        pending.g = false;
        if let KeyCode::Char('g') = key.code {
            app.move_top();
            return;
        }
    }

    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Esc => {
            if app.show_help {
                app.show_help = false;
            } else {
                app.mode = Mode::Normal;
            }
        }
        KeyCode::Char('?') => app.show_help = !app.show_help,

        // Movement.
        KeyCode::Char('h') | KeyCode::Left => app.move_cursor(Direction::Left),
        KeyCode::Char('j') | KeyCode::Down => app.move_cursor(Direction::Down),
        KeyCode::Char('k') | KeyCode::Up => app.move_cursor(Direction::Up),
        KeyCode::Char('l') | KeyCode::Right => app.move_cursor(Direction::Right),
        KeyCode::Char('w') => app.move_box(Direction::Right),
        KeyCode::Char('b') => app.move_box(Direction::Left),
        KeyCode::Char('0') => app.move_row_start(),
        KeyCode::Char('$') => app.move_row_end(),
        KeyCode::Char('g') => pending.g = true,
        KeyCode::Char('G') => app.move_bottom(),
        KeyCode::Char('f') => pending.find = true,

        // Editing.
        KeyCode::Char(c @ '1'..='9') => app.input_digit(c as u8 - b'0'),
        KeyCode::Char('m') => app.toggle_mode(),
        KeyCode::Char('x') | KeyCode::Backspace | KeyCode::Delete => app.clear_cell(),
        KeyCode::Char('u') => app.undo(),
        KeyCode::Char('r') if ctrl => app.redo(),

        // Assists.
        KeyCode::Char('H') => app.hint(),
        KeyCode::Char('c') => app.toggle_check(),
        KeyCode::Char('v') => app.toggle_match_highlight(),
        KeyCode::Char('s') => app.open_share(),
        KeyCode::Char('p') => app.open_code_entry(),
        KeyCode::Char('d') => app.open_difficulty_menu(),

        // Game control.
        KeyCode::Char('n') => {
            let d = app.difficulty;
            app.new_game(d);
        }
        KeyCode::Char('N') => {
            let d = app.difficulty.next();
            app.new_game(d);
        }

        _ => {}
    }
}

#[cfg(test)]
mod render_test {
    use super::*;
    use ratatui::backend::TestBackend;

    fn rendered(app: &App, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| ui::draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf.get(x, y).symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn renders_board_and_panel() {
        let app = App::new(Difficulty::Easy);
        let screen = rendered(&app, 90, 40);
        // Heavy outer corners of the 3x3 box grid.
        assert!(screen.contains('┏') && screen.contains('┛'));
        // A heavy/light junction that only the full gridline produces.
        assert!(screen.contains('╋'));
        assert!(screen.contains("Numbers left"));
        assert!(screen.contains("Difficulty"));
    }

    #[test]
    fn renders_on_small_terminal_without_panic() {
        let app = App::new(Difficulty::Easy);
        // Smaller than the board; must clip rather than panic.
        let _ = rendered(&app, 40, 20);
    }

    #[test]
    fn board_fits_short_terminal() {
        let app = App::new(Difficulty::Easy);
        // A height that can't hold the full 37-row board. The board should
        // shrink to fit, so its bottom corners must still be drawn.
        let screen = rendered(&app, 100, 24);
        assert!(screen.contains('┗') && screen.contains('┛'));
    }

    #[test]
    fn win_banner_appears_after_solving() {
        let mut app = App::new(Difficulty::Easy);
        // Reveal every cell to solve the board and start the win animation.
        for r in 0..9 {
            for c in 0..9 {
                if app.board.value(r, c).is_none() {
                    app.cursor = (r, c);
                    app.hint();
                }
            }
        }
        assert!(app.is_won());
        // Wait past the banner's pop-in delay, then it should be on screen.
        std::thread::sleep(Duration::from_millis(900));
        let screen = rendered(&app, 90, 40);
        assert!(screen.contains("S O L V E D"));
        // The full footer must fit — the last word was getting clipped when the
        // banner used a fixed width narrower than its content.
        assert!(screen.contains("s share") && screen.contains("q quit"));
    }

    #[test]
    fn share_card_renders_without_clipping() {
        let mut app = App::new(Difficulty::Easy);
        for r in 0..9 {
            for c in 0..9 {
                if app.board.value(r, c).is_none() {
                    app.cursor = (r, c);
                    app.hint();
                }
            }
        }
        assert!(app.is_won());
        app.open_share();
        let screen = rendered(&app, 90, 40);
        // The whole card is on screen — including the labels that were being
        // cut off when the popup was a fixed narrow width.
        assert!(screen.contains("Share result"));
        assert!(screen.contains("Match-highlight"));
        assert!(screen.contains("Error-check"));
        assert!(screen.contains("clue") && screen.contains("hint"));
        assert!(screen.contains("y copy"));
    }

    #[test]
    fn notes_visible_below_fullscreen() {
        // At a height too small for the full 37-row board, adding notes must
        // still change what's drawn (i.e. notes render, not just get stored).
        let mut app = App::new(Difficulty::Easy);
        let before = rendered(&app, 100, 34);
        'find: for r in 0..9 {
            for c in 0..9 {
                if app.board.value(r, c).is_none() {
                    app.board.cell_mut(r, c).toggle_note(2);
                    app.board.cell_mut(r, c).toggle_note(8);
                    break 'find;
                }
            }
        }
        let after = rendered(&app, 100, 34);
        assert_ne!(before, after);
    }
}

