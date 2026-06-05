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
    /// The seed this puzzle was generated from; combined with the difficulty it
    /// forms the shareable code that reproduces this exact puzzle.
    seed: u32,
    pub cursor: (usize, usize),
    pub mode: Mode,
    /// Whether to flag cells whose value disagrees with the solution.
    pub check_errors: bool,
    /// Whether to highlight the rows, columns, and boxes of *every* cell sharing
    /// the cursor's value, not just the cursor's own peers.
    pub highlight_matches: bool,
    /// Whether match-highlight was ever switched on this game (for the share card).
    pub used_highlight: bool,
    /// Whether error-checking was ever switched on this game (for the share card).
    pub used_check: bool,
    /// Number of digits placed that disagreed with the solution.
    pub mistakes: u32,
    /// Number of cells revealed via the hint key.
    pub hints_used: u32,
    /// Per-cell flag: was this cell's final value filled by a hint (vs. typed)?
    hint_cells: [bool; 81],
    /// Whether the post-win share card is open.
    pub show_share: bool,
    /// Set when the user asks to copy the share card; the frontend performs the
    /// actual clipboard write and clears it.
    pub copy_requested: bool,
    pub status: String,
    pub show_help: bool,
    /// Whether the difficulty-selection menu is open.
    pub difficulty_menu: bool,
    /// Highlighted entry within the difficulty menu.
    pub menu_index: usize,
    pub should_quit: bool,
    won: bool,
    start: Instant,
    win_elapsed: Option<Duration>,
    /// When the puzzle was solved, used to drive the win animation.
    won_at: Option<Instant>,
    /// While `Some`, the user is typing/pasting a puzzle code; holds the buffer.
    pub code_entry: Option<String>,
    undo_stack: Vec<Board>,
    redo_stack: Vec<Board>,
}

impl App {
    /// Start a new game at the given difficulty from a fresh random seed.
    pub fn new(difficulty: Difficulty) -> Self {
        App::from_puzzle(generator::generate(difficulty))
    }

    /// Start the specific puzzle identified by `difficulty` and `seed`.
    pub fn new_seeded(difficulty: Difficulty, seed: u32) -> Self {
        App::from_puzzle(generator::generate_seeded(difficulty, seed))
    }

    fn from_puzzle(puzzle: generator::Puzzle) -> Self {
        App {
            board: puzzle.board,
            solution: puzzle.solution,
            difficulty: puzzle.difficulty,
            seed: puzzle.seed,
            cursor: (0, 0),
            mode: Mode::Normal,
            check_errors: false,
            highlight_matches: false,
            used_highlight: false,
            used_check: false,
            mistakes: 0,
            hints_used: 0,
            hint_cells: [false; 81],
            show_share: false,
            copy_requested: false,
            status: String::new(),
            show_help: false,
            difficulty_menu: false,
            menu_index: puzzle.difficulty.index(),
            should_quit: false,
            won: false,
            start: Instant::now(),
            win_elapsed: None,
            won_at: None,
            code_entry: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Replace the current game with a freshly generated one.
    pub fn new_game(&mut self, difficulty: Difficulty) {
        *self = App::new(difficulty);
        self.status = format!("New {} game", difficulty.label());
    }

    /// This puzzle's shareable code, e.g. `"3F9KQ2"`.
    pub fn code(&self) -> String {
        generator::encode_code(self.difficulty, self.seed)
    }

    pub fn is_won(&self) -> bool {
        self.won
    }

    /// Time since the puzzle was solved, or `None` if it isn't solved.
    /// Drives the win animation; the frontend redraws while this is `Some`.
    pub fn win_anim_elapsed(&self) -> Option<Duration> {
        self.won_at.map(|t| t.elapsed())
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

    /// Jump to the cell holding `value` that is nearest the cursor (by
    /// Manhattan distance), excluding the current cell. Ties favor the
    /// top-left-most match. Sets a status message if no such value is placed.
    pub fn find_nearest(&mut self, value: u8) {
        let (cr, cc) = self.cursor;
        let mut best: Option<(usize, (usize, usize))> = None;
        for r in 0..9 {
            for c in 0..9 {
                if (r, c) == (cr, cc) || self.board.value(r, c) != Some(value) {
                    continue;
                }
                let dist = cr.abs_diff(r) + cc.abs_diff(c);
                if best.map_or(true, |(bd, _)| dist < bd) {
                    best = Some((dist, (r, c)));
                }
            }
        }
        match best {
            Some((_, pos)) => self.cursor = pos,
            None => self.status = format!("No {} on the board", value),
        }
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

    // --- Difficulty menu ----------------------------------------------------

    pub fn open_difficulty_menu(&mut self) {
        self.difficulty_menu = true;
        self.menu_index = self.difficulty.index();
    }

    pub fn close_difficulty_menu(&mut self) {
        self.difficulty_menu = false;
    }

    /// Move the menu highlight by `delta`, wrapping around.
    pub fn menu_move(&mut self, delta: i32) {
        let len = Difficulty::ALL.len() as i32;
        self.menu_index = (self.menu_index as i32 + delta).rem_euclid(len) as usize;
    }

    /// Jump the menu highlight to a specific entry.
    pub fn menu_select(&mut self, index: usize) {
        if index < Difficulty::ALL.len() {
            self.menu_index = index;
        }
    }

    /// Start a new game at the highlighted difficulty and close the menu.
    pub fn menu_confirm(&mut self) {
        let difficulty = Difficulty::ALL[self.menu_index];
        self.new_game(difficulty);
    }

    // --- Play-by-code -------------------------------------------------------

    /// Open the prompt for entering a shared puzzle code.
    pub fn open_code_entry(&mut self) {
        self.code_entry = Some(String::new());
        self.show_share = false;
    }

    pub fn cancel_code_entry(&mut self) {
        self.code_entry = None;
    }

    /// Append typed or pasted text to the code buffer (no-op if not entering).
    pub fn code_entry_push(&mut self, text: &str) {
        if let Some(buf) = self.code_entry.as_mut() {
            buf.push_str(text);
        }
    }

    pub fn code_entry_backspace(&mut self) {
        if let Some(buf) = self.code_entry.as_mut() {
            buf.pop();
        }
    }

    /// Try to start the puzzle named by the current code buffer. On success the
    /// game is replaced; on failure the prompt stays open with an error.
    pub fn code_entry_submit(&mut self) {
        let buf = match self.code_entry.take() {
            Some(b) => b,
            None => return,
        };
        match generator::decode_code(&buf) {
            Some((difficulty, seed)) => {
                let code = generator::encode_code(difficulty, seed);
                *self = App::new_seeded(difficulty, seed);
                self.status = format!("Loaded puzzle #{} ({})", code, difficulty.label());
            }
            None => {
                self.status = "Not a valid puzzle code".into();
                self.code_entry = Some(buf); // keep it open so they can fix it
            }
        }
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
                // A typed value that contradicts the solution is a mistake; the
                // cell is now player-owned, so it's no longer a hint.
                if n != self.solution[r * 9 + c] {
                    self.mistakes += 1;
                }
                self.hint_cells[r * 9 + c] = false;
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
            self.hint_cells[r * 9 + c] = false;
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
        self.hint_cells[r * 9 + c] = true;
        self.hints_used += 1;
        self.clear_peer_notes(r, c, correct);
        self.status = "Revealed".into();
        self.check_win();
    }

    pub fn toggle_match_highlight(&mut self) {
        self.highlight_matches = !self.highlight_matches;
        self.used_highlight |= self.highlight_matches;
        self.status = if self.highlight_matches {
            "Match highlight on".into()
        } else {
            "Match highlight off".into()
        };
    }

    pub fn toggle_check(&mut self) {
        self.check_errors = !self.check_errors;
        self.used_check |= self.check_errors;
        self.status = if self.check_errors {
            "Error checking on".into()
        } else {
            "Error checking off".into()
        };
    }

    // --- Share card ---------------------------------------------------------

    /// Open the post-win share card and request a clipboard copy.
    pub fn open_share(&mut self) {
        if !self.won {
            return;
        }
        self.show_share = true;
        self.copy_requested = true;
    }

    pub fn close_share(&mut self) {
        self.show_share = false;
    }

    /// A Wordle-style, spoiler-free summary of the finished game, suitable for
    /// pasting into a chat. Each cell becomes a colored square: a clue, a value
    /// the player typed, or a hinted reveal.
    pub fn share_text(&self) -> String {
        let secs = self.elapsed().as_secs();
        let plural = |n: u32, word: &str| {
            if n == 1 {
                format!("{} {}", n, word)
            } else {
                format!("{} {}s", n, word)
            }
        };
        let flag = |on: bool| if on { "used" } else { "off" };

        let mut s = String::new();
        s.push_str(&format!(
            "Sudoku {} {} — Solved!\n",
            self.difficulty.badge(),
            self.difficulty.label()
        ));
        s.push_str(&format!(
            "⏱️ {:02}:{:02}  ❌ {}  💡 {}\n",
            secs / 60,
            secs % 60,
            plural(self.mistakes, "mistake"),
            plural(self.hints_used, "hint"),
        ));
        s.push_str(&format!(
            "🔆 Match-highlight: {}   🔍 Error-check: {}\n\n",
            flag(self.used_highlight),
            flag(self.used_check),
        ));

        for r in 0..9 {
            for c in 0..9 {
                let sq = if self.board.is_given(r, c) {
                    "🟦"
                } else if self.hint_cells[r * 9 + c] {
                    "🟧"
                } else {
                    "🟩"
                };
                s.push_str(sq);
            }
            s.push('\n');
        }
        s.push_str("\n🟦 clue  🟩 you  🟧 hint\n");
        s.push_str(&format!("\n🧩 Puzzle #{}", self.code()));
        s
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
            self.won_at = None;
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
            self.won_at = Some(Instant::now());
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
    fn pasting_a_code_reproduces_the_same_puzzle() {
        // One player's game and its share code.
        let mine = app();
        let code = mine.code();

        // A friend pastes the (possibly messy) code into their game.
        let mut friend = App::new(Difficulty::Expert); // start unrelated
        friend.open_code_entry();
        friend.code_entry_push(&format!("Puzzle #{} ", code));
        friend.code_entry_submit();

        // The prompt closed and they're on the exact same board and difficulty.
        assert!(friend.code_entry.is_none());
        assert_eq!(friend.difficulty, mine.difficulty);
        assert_eq!(friend.code(), code);
        for i in 0..81 {
            assert_eq!(
                friend.board.value(i / 9, i % 9),
                mine.board.value(i / 9, i % 9)
            );
        }
    }

    #[test]
    fn bad_code_keeps_prompt_open() {
        let mut a = app();
        a.open_code_entry();
        a.code_entry_push("###");
        a.code_entry_submit();
        // Rejected: prompt stays open with the buffer intact and an error set.
        assert_eq!(a.code_entry.as_deref(), Some("###"));
        assert!(!a.status.is_empty());
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
    fn find_nearest_jumps_to_closest() {
        let mut a = app();
        // Clear any givens so we control the board, then plant two 7s.
        a.board = crate::board::Board::new();
        a.board.set_value(0, 5, 7);
        a.board.set_value(8, 8, 7);
        a.cursor = (0, 0);
        a.find_nearest(7);
        assert_eq!(a.cursor, (0, 5)); // closer of the two

        a.cursor = (8, 7);
        a.find_nearest(7);
        assert_eq!(a.cursor, (8, 8));

        // No 3 anywhere: cursor stays put, status set.
        a.cursor = (4, 4);
        a.find_nearest(3);
        assert_eq!(a.cursor, (4, 4));
        assert!(!a.status.is_empty());
    }

    #[test]
    fn solving_arms_the_win_animation() {
        let mut a = app();
        assert!(!a.is_won());
        assert!(a.win_anim_elapsed().is_none());
        // Reveal every empty cell to force a solved board.
        for r in 0..9 {
            for c in 0..9 {
                if a.board.value(r, c).is_none() {
                    a.cursor = (r, c);
                    a.hint();
                }
            }
        }
        assert!(a.is_won());
        // The animation clock is running once solved.
        assert!(a.win_anim_elapsed().is_some());
        // Undo retreats from the win and disarms the animation.
        a.undo();
        assert!(!a.is_won());
        assert!(a.win_anim_elapsed().is_none());
    }

    #[test]
    fn share_text_summarizes_the_game() {
        let mut a = app();
        // Error-check was toggled on then off; match-highlight never used.
        a.toggle_check();
        a.toggle_check();

        let empties: Vec<(usize, usize)> = (0..9)
            .flat_map(|r| (0..9).map(move |c| (r, c)))
            .filter(|&(r, c)| a.board.value(r, c).is_none())
            .collect();

        // First empty: one wrong guess (a mistake), then the right value.
        let (r0, c0) = empties[0];
        a.cursor = (r0, c0);
        let sol0 = a.solution[r0 * 9 + c0];
        a.input_digit(if sol0 == 1 { 2 } else { 1 });
        a.input_digit(sol0);

        // Second empty: revealed with a hint.
        let (r1, c1) = empties[1];
        a.cursor = (r1, c1);
        a.hint();

        // Everything else typed correctly.
        for &(r, c) in &empties[2..] {
            a.cursor = (r, c);
            a.input_digit(a.solution[r * 9 + c]);
        }

        assert!(a.is_won());
        assert_eq!(a.mistakes, 1);
        assert_eq!(a.hints_used, 1);
        assert!(a.used_check);
        assert!(!a.used_highlight);

        let s = a.share_text();
        assert!(s.contains("Solved!"));
        assert!(s.contains("1 mistake") && !s.contains("1 mistakes"));
        assert!(s.contains("1 hint") && !s.contains("1 hints"));
        assert!(s.contains("Error-check: used"));
        assert!(s.contains("Match-highlight: off"));

        // Count squares in the 9-cell grid rows only (the legend line also
        // carries one of each swatch).
        let grid: String = s
            .lines()
            .filter(|l| l.chars().count() == 9 && l.chars().all(|ch| "🟦🟩🟧".contains(ch)))
            .collect();
        let givens = (0..81).filter(|&i| a.board.is_given(i / 9, i % 9)).count();
        assert_eq!(grid.matches('🟧').count(), 1); // exactly one hinted cell
        assert_eq!(grid.matches('🟦').count(), givens); // every clue
        assert_eq!(grid.chars().count(), 81);
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
