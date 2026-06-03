//! The Sudoku board model: cells, the 9x9 grid, and validation.

/// A single cell on the board.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    /// The committed value, 1..=9, or `None` if empty.
    pub value: Option<u8>,
    /// Whether this cell is a fixed clue from the puzzle (not user-editable).
    pub given: bool,
    /// Pencil-mark candidates as a bitmask: bit `n-1` set means note `n` is present.
    pub notes: u16,
}

impl Cell {
    pub fn has_note(&self, n: u8) -> bool {
        debug_assert!((1..=9).contains(&n));
        self.notes & (1 << (n - 1)) != 0
    }

    pub fn toggle_note(&mut self, n: u8) {
        debug_assert!((1..=9).contains(&n));
        self.notes ^= 1 << (n - 1);
    }

    pub fn remove_note(&mut self, n: u8) {
        debug_assert!((1..=9).contains(&n));
        self.notes &= !(1 << (n - 1));
    }

    pub fn clear_notes(&mut self) {
        self.notes = 0;
    }
}

/// A 9x9 Sudoku grid.
#[derive(Clone, Debug)]
pub struct Board {
    cells: [Cell; 81],
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

/// Flatten a (row, col) pair into a 0..81 index.
#[inline]
pub fn idx(row: usize, col: usize) -> usize {
    debug_assert!(row < 9 && col < 9);
    row * 9 + col
}

impl Board {
    pub fn new() -> Self {
        Board {
            cells: [Cell::default(); 81],
        }
    }

    /// Build a board from a 81-length array of digits (0 = empty). All non-zero
    /// cells are marked as givens.
    pub fn from_digits(digits: &[u8; 81]) -> Self {
        let mut board = Board::new();
        for (i, &d) in digits.iter().enumerate() {
            if d != 0 {
                board.cells[i] = Cell {
                    value: Some(d),
                    given: true,
                    notes: 0,
                };
            }
        }
        board
    }

    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.cells[idx(row, col)]
    }

    pub fn cell_mut(&mut self, row: usize, col: usize) -> &mut Cell {
        &mut self.cells[idx(row, col)]
    }

    pub fn value(&self, row: usize, col: usize) -> Option<u8> {
        self.cells[idx(row, col)].value
    }

    pub fn is_given(&self, row: usize, col: usize) -> bool {
        self.cells[idx(row, col)].given
    }

    /// Set a value on a non-given cell. Clears the cell's notes. No-op on givens.
    /// Returns true if the board changed.
    pub fn set_value(&mut self, row: usize, col: usize, value: u8) -> bool {
        debug_assert!((1..=9).contains(&value));
        let cell = &mut self.cells[idx(row, col)];
        if cell.given {
            return false;
        }
        cell.value = Some(value);
        cell.clear_notes();
        true
    }

    /// Clear a non-given cell's value (notes are left untouched). No-op on givens.
    /// Returns true if the board changed.
    pub fn clear_value(&mut self, row: usize, col: usize) -> bool {
        let cell = &mut self.cells[idx(row, col)];
        if cell.given || cell.value.is_none() {
            return false;
        }
        cell.value = None;
        true
    }

    /// Whether placing `value` at (row, col) duplicates an existing value in the
    /// same row, column, or 3x3 box (ignoring the cell itself).
    pub fn would_conflict(&self, row: usize, col: usize, value: u8) -> bool {
        // Row and column.
        for i in 0..9 {
            if i != col && self.value(row, i) == Some(value) {
                return true;
            }
            if i != row && self.value(i, col) == Some(value) {
                return true;
            }
        }
        // 3x3 box.
        let (br, bc) = (row / 3 * 3, col / 3 * 3);
        for r in br..br + 3 {
            for c in bc..bc + 3 {
                if (r, c) != (row, col) && self.value(r, c) == Some(value) {
                    return true;
                }
            }
        }
        false
    }

    /// Whether the cell at (row, col) currently holds a value that conflicts with
    /// a peer. Empty cells never conflict.
    pub fn has_conflict(&self, row: usize, col: usize) -> bool {
        match self.value(row, col) {
            Some(v) => self.would_conflict(row, col, v),
            None => false,
        }
    }

    /// True when every cell is filled and no cell conflicts: a winning board.
    pub fn is_solved(&self) -> bool {
        for row in 0..9 {
            for col in 0..9 {
                match self.value(row, col) {
                    None => return false,
                    Some(v) => {
                        if self.would_conflict(row, col, v) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    /// Number of empty cells remaining.
    pub fn empty_count(&self) -> usize {
        self.cells.iter().filter(|c| c.value.is_none()).count()
    }

    /// How many cells currently hold the value `v`.
    pub fn count_value(&self, v: u8) -> usize {
        self.cells.iter().filter(|c| c.value == Some(v)).count()
    }

    /// Digits 1..=9 not yet present in `row`, in ascending order.
    pub fn missing_in_row(&self, row: usize) -> Vec<u8> {
        Self::missing((0..9).filter_map(|c| self.value(row, c)))
    }

    /// Digits 1..=9 not yet present in `col`, in ascending order.
    pub fn missing_in_col(&self, col: usize) -> Vec<u8> {
        Self::missing((0..9).filter_map(|r| self.value(r, col)))
    }

    /// Digits 1..=9 not yet present in the 3x3 box containing (row, col).
    pub fn missing_in_box(&self, row: usize, col: usize) -> Vec<u8> {
        let (br, bc) = (row / 3 * 3, col / 3 * 3);
        let values = (br..br + 3).flat_map(|r| (bc..bc + 3).filter_map(move |c| self.value(r, c)));
        Self::missing(values)
    }

    /// Given the values present in some unit, return the absent digits 1..=9.
    fn missing(values: impl Iterator<Item = u8>) -> Vec<u8> {
        let mut present = [false; 10];
        for v in values {
            present[v as usize] = true;
        }
        (1..=9).filter(|&n| !present[n as usize]).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_toggle() {
        let mut c = Cell::default();
        assert!(!c.has_note(5));
        c.toggle_note(5);
        assert!(c.has_note(5));
        c.toggle_note(5);
        assert!(!c.has_note(5));
    }

    #[test]
    fn setting_value_clears_notes() {
        let mut b = Board::new();
        b.cell_mut(0, 0).toggle_note(3);
        assert!(b.cell(0, 0).has_note(3));
        b.set_value(0, 0, 7);
        assert_eq!(b.value(0, 0), Some(7));
        assert!(!b.cell(0, 0).has_note(3));
    }

    #[test]
    fn givens_are_immutable() {
        let mut digits = [0u8; 81];
        digits[0] = 4;
        let mut b = Board::from_digits(&digits);
        assert!(b.is_given(0, 0));
        assert!(!b.set_value(0, 0, 9));
        assert!(!b.clear_value(0, 0));
        assert_eq!(b.value(0, 0), Some(4));
    }

    #[test]
    fn detects_row_col_box_conflicts() {
        let mut b = Board::new();
        b.set_value(0, 0, 5);
        assert!(b.would_conflict(0, 8, 5)); // same row
        assert!(b.would_conflict(8, 0, 5)); // same col
        assert!(b.would_conflict(1, 1, 5)); // same box
        assert!(!b.would_conflict(4, 4, 5)); // unrelated
    }

    #[test]
    fn missing_digits_per_unit() {
        let mut b = Board::new();
        for c in 0..8 {
            b.set_value(0, c, (c + 1) as u8); // row 0 holds 1..=8
        }
        assert_eq!(b.missing_in_row(0), vec![9]);
        // Column 0 only has the 1 from above.
        assert_eq!(b.missing_in_col(0), vec![2, 3, 4, 5, 6, 7, 8, 9]);
        // Top-left box has 1, 2, 3 (cols 0..3 of row 0).
        assert_eq!(b.missing_in_box(1, 1), vec![4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn solved_detection() {
        // A valid completed grid.
        let solved: [u8; 81] = [
            5, 3, 4, 6, 7, 8, 9, 1, 2, 6, 7, 2, 1, 9, 5, 3, 4, 8, 1, 9, 8, 3, 4, 2, 5, 6, 7, 8, 5,
            9, 7, 6, 1, 4, 2, 3, 4, 2, 6, 8, 5, 3, 7, 9, 1, 7, 1, 3, 9, 2, 4, 8, 5, 6, 9, 6, 1, 5,
            3, 7, 2, 8, 4, 2, 8, 7, 4, 1, 9, 6, 3, 5, 3, 4, 5, 2, 8, 6, 1, 7, 9,
        ];
        let b = Board::from_digits(&solved);
        assert!(b.is_solved());
        assert_eq!(b.empty_count(), 0);
    }
}
