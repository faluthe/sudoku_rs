//! Application state and game logic, independent of the terminal frontend.

use crate::board::Board;
use crate::generator::{self, Difficulty};
use std::time::{Duration, Instant};

/// Input mode, à la vim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Digits place a committed value.
    Normal,
    /// Digits toggle pencil-mark notes.
    Note,
}

/// A cursor movement direction.
#[derive(Clone, Copy, Debug)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

pub struct App {
    pub board: Board,
    solution: [u8; 81],
    pub difficulty: Difficulty,
    pub cursor: (usize, usize),
    pub mode: Mode,
    /// Whether to flag cells whose value disagrees with the solution.
    pub check_errors: bool,
    pub status: String,
    pub show_help: bool,
    pub should_quit: bool,
    won: bool,
    start: Instant,
    win_elapsed: Option<Duration>,
    undo_stack: Vec<Board>,
    redo_stack: Vec<Board>,
}

impl App {
    /// Start a new game at the given difficulty.
    pub fn new(difficulty: Difficulty) -> Self {
        let puzzle = generator::generate(difficulty);
        App {
            board: puzzle.board,
            solution: puzzle.solution,
            difficulty: puzzle.difficulty,
            cursor: (0, 0),
            mode: Mode::Normal,
            check_errors: false,
            status: String::new(),
            show_help: false,
            should_quit: false,
            won: false,
            start: Instant::now(),
            win_elapsed: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Replace the current game with a freshly generated one.
    pub fn new_game(&mut self, difficulty: Difficulty) {
        *self = App::new(difficulty);
        self.status = format!("New {} game", difficulty.label());
    }

    pub fn is_won(&self) -> bool {
        self.won
    }

    /// Time elapsed, frozen at the moment of winning.
    pub fn elapsed(&self) -> Duration {
        self.win_elapsed.unwrap_or_else(|| self.start.elapsed())
    }

    // --- Navigation ---------------------------------------------------------

    pub fn move_cursor(&mut self, dir: Direction) {
        let (r, c) = self.cursor;
        self.cursor = match dir {
            Direction::Up => (r.saturating_sub(1), c),
            Direction::Down => ((r + 1).min(8), c),
            Direction::Left => (r, c.saturating_sub(1)),
            Direction::Right => (r, (c + 1).min(8)),
        };
    }

    pub fn move_row_start(&mut self) {
        self.cursor.1 = 0;
    }

    pub fn move_row_end(&mut self) {
        self.cursor.1 = 8;
    }

    pub fn move_top(&mut self) {
        self.cursor.0 = 0;
    }

    pub fn move_bottom(&mut self) {
        self.cursor.0 = 8;
    }

    /// Jump to the next 3x3 box boundary in a direction (vim `w`/`b` feel).
    pub fn move_box(&mut self, dir: Direction) {
        let (r, c) = self.cursor;
        self.cursor = match dir {
            Direction::Left => (r, c.saturating_sub(3) / 3 * 3),
            Direction::Right => (r, (c / 3 * 3 + 3).min(6)),
            Direction::Up => (r.saturating_sub(3) / 3 * 3, c),
            Direction::Down => ((r / 3 * 3 + 3).min(6), c),
        };
    }

    // --- Editing ------------------------------------------------------------

    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            Mode::Normal => Mode::Note,
            Mode::Note => Mode::Normal,
        };
    }

    /// Snapshot the board onto the undo stack and clear the redo stack.
    fn checkpoint(&mut self) {
        self.undo_stack.push(self.board.clone());
        self.redo_stack.clear();
    }

    /// Handle a digit 1..=9 according to the current mode.
    pub fn input_digit(&mut self, n: u8) {
        debug_assert!((1..=9).contains(&n));
        let (r, c) = self.cursor;
        if self.board.is_given(r, c) {
            self.status = "That cell is a clue".into();
            return;
        }
        match self.mode {
            Mode::Normal => {
                self.checkpoint();
                self.board.set_value(r, c, n);
                self.clear_peer_notes(r, c, n);
                self.check_win();
            }
            Mode::Note => {
                // Notes only make sense on empty cells.
                if self.board.value(r, c).is_some() {
                    self.status = "Clear the value before adding notes".into();
                    return;
                }
                self.checkpoint();
                self.board.cell_mut(r, c).toggle_note(n);
            }
        }
    }

    /// Clear the value (or, if empty, the notes) of the current cell.
    pub fn clear_cell(&mut self) {
        let (r, c) = self.cursor;
        if self.board.is_given(r, c) {
            return;
        }
        if self.board.value(r, c).is_some() {
            self.checkpoint();
            self.board.clear_value(r, c);
        } else if self.board.cell(r, c).notes != 0 {
            self.checkpoint();
            self.board.cell_mut(r, c).clear_notes();
        }
    }

    /// Remove note `n` from every empty peer of (r, c) once `n` is committed there.
    fn clear_peer_notes(&mut self, r: usize, c: usize, n: u8) {
        for i in 0..9 {
            self.board.cell_mut(r, i).remove_note(n);
            self.board.cell_mut(i, c).remove_note(n);
        }
        let (br, bc) = (r / 3 * 3, c / 3 * 3);
        for rr in br..br + 3 {
            for cc in bc..bc + 3 {
                self.board.cell_mut(rr, cc).remove_note(n);
            }
        }
    }

    /// Reveal the current cell's correct value from the solution.
    pub fn hint(&mut self) {
        let (r, c) = self.cursor;
        if self.board.is_given(r, c) || self.board.value(r, c).is_some() {
            self.status = "Move to an empty cell for a hint".into();
            return;
        }
        let correct = self.solution[r * 9 + c];
        self.checkpoint();
        self.board.set_value(r, c, correct);
        self.clear_peer_notes(r, c, correct);
        self.status = "Revealed".into();
        self.check_win();
    }

    pub fn toggle_check(&mut self) {
        self.check_errors = !self.check_errors;
        self.status = if self.check_errors {
            "Error checking on".into()
        } else {
            "Error checking off".into()
        };
    }

    /// Whether the cell holds a value that disagrees with the solution.
    /// Only meaningful while `check_errors` is enabled.
    pub fn is_wrong(&self, r: usize, c: usize) -> bool {
        self.check_errors
            && matches!(self.board.value(r, c), Some(v) if v != self.solution[r * 9 + c])
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(std::mem::replace(&mut self.board, prev));
            self.won = false;
            self.win_elapsed = None;
        } else {
            self.status = "Nothing to undo".into();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(std::mem::replace(&mut self.board, next));
            self.check_win();
        } else {
            self.status = "Nothing to redo".into();
        }
    }

    fn check_win(&mut self) {
        if !self.won && self.board.is_solved() {
            self.won = true;
            self.win_elapsed = Some(self.start.elapsed());
            self.status = "Solved! Press 'n' for a new game".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App::new(Difficulty::Easy)
    }

    #[test]
    fn navigation_is_clamped() {
        let mut a = app();
        a.cursor = (0, 0);
        a.move_cursor(Direction::Up);
        a.move_cursor(Direction::Left);
        assert_eq!(a.cursor, (0, 0));
        a.move_bottom();
        a.move_row_end();
        assert_eq!(a.cursor, (8, 8));
        a.move_cursor(Direction::Down);
        a.move_cursor(Direction::Right);
        assert_eq!(a.cursor, (8, 8));
    }

    #[test]
    fn box_jump() {
        let mut a = app();
        a.cursor = (0, 0);
        a.move_box(Direction::Right);
        assert_eq!(a.cursor, (0, 3));
        a.move_box(Direction::Right);
        assert_eq!(a.cursor, (0, 6));
        a.move_box(Direction::Right);
        assert_eq!(a.cursor, (0, 6));
    }

    #[test]
    fn note_mode_toggles_notes() {
        let mut a = app();
        // Find an empty, non-given cell.
        let (mut er, mut ec) = (0, 0);
        'outer: for r in 0..9 {
            for c in 0..9 {
                if !a.board.is_given(r, c) {
                    er = r;
                    ec = c;
                    break 'outer;
                }
            }
        }
        a.cursor = (er, ec);
        a.mode = Mode::Note;
        a.input_digit(4);
        assert!(a.board.cell(er, ec).has_note(4));
        a.input_digit(4);
        assert!(!a.board.cell(er, ec).has_note(4));
    }

    #[test]
    fn undo_redo_roundtrip() {
        let mut a = app();
        let (mut er, mut ec) = (0, 0);
        'outer: for r in 0..9 {
            for c in 0..9 {
                if !a.board.is_given(r, c) {
                    er = r;
                    ec = c;
                    break 'outer;
                }
            }
        }
        a.cursor = (er, ec);
        a.mode = Mode::Normal;
        a.input_digit(5);
        assert_eq!(a.board.value(er, ec), Some(5));
        a.undo();
        assert_eq!(a.board.value(er, ec), None);
        a.redo();
        assert_eq!(a.board.value(er, ec), Some(5));
    }

    #[test]
    fn hint_fills_solution_value() {
        let mut a = app();
        let (mut er, mut ec) = (0, 0);
        'outer: for r in 0..9 {
            for c in 0..9 {
                if a.board.value(r, c).is_none() {
                    er = r;
                    ec = c;
                    break 'outer;
                }
            }
        }
        a.cursor = (er, ec);
        a.hint();
        assert_eq!(a.board.value(er, ec), Some(a.solution[er * 9 + ec]));
    }
}
