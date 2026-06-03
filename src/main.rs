mod app;
mod board;
mod generator;
mod ui;

use app::{App, Direction, Mode};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
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
    execute!(stdout, EnterAlternateScreen)?;

    // Make sure a panic doesn't leave the user's terminal in raw/alt-screen mode.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));

    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Tui) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()
}

fn run(terminal: &mut Tui) -> io::Result<()> {
    let mut app = App::new(Difficulty::Easy);
    // Tracks a pending `g` for the `gg` motion.
    let mut pending_g = false;

    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, &app))?;

        // Poll so the on-screen timer keeps ticking without input.
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(&mut app, key, &mut pending_g);
                }
            }
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent, pending_g: &mut bool) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    app.status.clear();

    // Resolve a pending `g` prefix: `gg` jumps to the top row.
    if *pending_g {
        *pending_g = false;
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
        KeyCode::Char('g') => *pending_g = true,
        KeyCode::Char('G') => app.move_bottom(),

        // Editing.
        KeyCode::Char(c @ '1'..='9') => app.input_digit(c as u8 - b'0'),
        KeyCode::Char('m') => app.toggle_mode(),
        KeyCode::Char('x') | KeyCode::Backspace | KeyCode::Delete => app.clear_cell(),
        KeyCode::Char('u') => app.undo(),
        KeyCode::Char('r') if ctrl => app.redo(),

        // Assists.
        KeyCode::Char('H') => app.hint(),
        KeyCode::Char('c') => app.toggle_check(),

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
