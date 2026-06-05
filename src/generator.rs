//! Puzzle generation: a backtracking solver, a uniqueness counter, and a
//! generator that carves a uniquely-solvable puzzle out of a full solution.

use crate::board::Board;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

/// A flat 9x9 grid of digits, where 0 means empty.
type Grid = [u8; 81];

/// Puzzle difficulty, expressed as a target number of remaining clues.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
    Expert,
}

impl Difficulty {
    /// All difficulties, in ascending order.
    pub const ALL: [Difficulty; 4] = [
        Difficulty::Easy,
        Difficulty::Medium,
        Difficulty::Hard,
        Difficulty::Expert,
    ];

    /// This difficulty's position within [`Difficulty::ALL`].
    pub fn index(self) -> usize {
        match self {
            Difficulty::Easy => 0,
            Difficulty::Medium => 1,
            Difficulty::Hard => 2,
            Difficulty::Expert => 3,
        }
    }

    /// The difficulty at position `i` within [`Difficulty::ALL`], if any.
    pub fn from_index(i: usize) -> Option<Difficulty> {
        Difficulty::ALL.get(i).copied()
    }

    /// How many clues to aim to leave on the board.
    fn target_clues(self) -> usize {
        match self {
            Difficulty::Easy => 42,
            Difficulty::Medium => 34,
            Difficulty::Hard => 30,
            Difficulty::Expert => 26,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Medium => "Medium",
            Difficulty::Hard => "Hard",
            Difficulty::Expert => "Expert",
        }
    }

    /// A colored emoji that flags the difficulty in shared results.
    pub fn badge(self) -> &'static str {
        match self {
            Difficulty::Easy => "🟢",
            Difficulty::Medium => "🟡",
            Difficulty::Hard => "🟠",
            Difficulty::Expert => "🔴",
        }
    }

    /// Cycle to the next difficulty (wraps around).
    pub fn next(self) -> Difficulty {
        match self {
            Difficulty::Easy => Difficulty::Medium,
            Difficulty::Medium => Difficulty::Hard,
            Difficulty::Hard => Difficulty::Expert,
            Difficulty::Expert => Difficulty::Easy,
        }
    }
}

/// A generated puzzle: the playable board plus its unique full solution.
pub struct Puzzle {
    pub board: Board,
    pub solution: Grid,
    pub difficulty: Difficulty,
    /// The seed it was generated from; the same seed and difficulty always
    /// reproduce this exact puzzle.
    pub seed: u32,
}

/// Whether `val` can be placed at flat index `i` without violating row/col/box.
fn valid(grid: &Grid, i: usize, val: u8) -> bool {
    let (r, c) = (i / 9, i % 9);
    for k in 0..9 {
        if grid[r * 9 + k] == val || grid[k * 9 + c] == val {
            return false;
        }
    }
    let (br, bc) = (r / 3 * 3, c / 3 * 3);
    for rr in br..br + 3 {
        for cc in bc..bc + 3 {
            if grid[rr * 9 + cc] == val {
                return false;
            }
        }
    }
    true
}

/// Fill `grid` completely with a random valid solution via backtracking.
/// Returns false only if the (partial) grid is unsolvable.
fn fill(grid: &mut Grid, rng: &mut impl Rng) -> bool {
    let pos = match grid.iter().position(|&v| v == 0) {
        Some(p) => p,
        None => return true, // fully filled
    };
    let mut candidates: [u8; 9] = [1, 2, 3, 4, 5, 6, 7, 8, 9];
    candidates.shuffle(rng);
    for &val in &candidates {
        if valid(grid, pos, val) {
            grid[pos] = val;
            if fill(grid, rng) {
                return true;
            }
            grid[pos] = 0;
        }
    }
    false
}

/// Count solutions of `grid`, stopping early once `limit` is reached.
fn count_solutions(grid: &mut Grid, limit: usize) -> usize {
    let pos = match grid.iter().position(|&v| v == 0) {
        Some(p) => p,
        None => return 1,
    };
    let mut total = 0;
    for val in 1..=9 {
        if valid(grid, pos, val) {
            grid[pos] = val;
            total += count_solutions(grid, limit);
            grid[pos] = 0;
            if total >= limit {
                return total;
            }
        }
    }
    total
}

/// Generate a puzzle of the given difficulty from a fresh random seed.
pub fn generate(difficulty: Difficulty) -> Puzzle {
    generate_seeded(difficulty, rand::thread_rng().gen())
}

/// Generate a puzzle of the given difficulty with a guaranteed unique solution,
/// deterministically from `seed`. The same `(difficulty, seed)` pair always
/// yields the identical puzzle, which is what makes share codes reproducible.
pub fn generate_seeded(difficulty: Difficulty, seed: u32) -> Puzzle {
    let mut rng = StdRng::seed_from_u64(seed as u64);

    // 1. Build a complete, valid solution.
    let mut solution: Grid = [0; 81];
    fill(&mut solution, &mut rng);

    // 2. Carve holes while keeping the solution unique.
    let mut puzzle = solution;
    let mut positions: Vec<usize> = (0..81).collect();
    positions.shuffle(&mut rng);

    let mut clues = 81;
    let target = difficulty.target_clues();
    for &pos in &positions {
        if clues <= target {
            break;
        }
        let removed = puzzle[pos];
        if removed == 0 {
            continue;
        }
        puzzle[pos] = 0;
        let mut probe = puzzle;
        if count_solutions(&mut probe, 2) == 1 {
            clues -= 1;
        } else {
            puzzle[pos] = removed; // removal broke uniqueness; restore
        }
    }

    Puzzle {
        board: Board::from_digits(&puzzle),
        solution,
        difficulty,
        seed,
    }
}

// --- Share codes ------------------------------------------------------------
//
// A share code packs the difficulty and seed into one base-36 token: the low
// two bits are the difficulty index, the rest is the 32-bit seed. So a single
// short string fully determines the puzzle.

/// The shareable code for a `(difficulty, seed)` puzzle, e.g. `"3F9KQ2"`.
pub fn encode_code(difficulty: Difficulty, seed: u32) -> String {
    let packed = (seed as u64) << 2 | difficulty.index() as u64;
    to_base36(packed)
}

/// Parse a share code back into its difficulty and seed. Tolerant of
/// surrounding text: it grabs the token after a `#` if present, else the whole
/// trimmed input, and ignores case and non-alphanumeric noise. Returns `None`
/// if the token isn't a valid code.
pub fn decode_code(input: &str) -> Option<(Difficulty, u32)> {
    let region = input.rsplit_once('#').map_or(input, |(_, after)| after);
    // The first maximal run of alphanumerics (skips leading spaces, stops at the
    // first separator), so both a bare code and a pasted "#CODE …" line work.
    let token: String = region
        .chars()
        .skip_while(|c| !c.is_ascii_alphanumeric())
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    if token.is_empty() {
        return None;
    }
    let packed = u64::from_str_radix(&token, 36).ok()?;
    let seed = packed >> 2;
    if seed > u32::MAX as u64 {
        return None; // not one of our codes
    }
    let difficulty = Difficulty::from_index((packed & 3) as usize)?;
    Some((difficulty, seed as u32))
}

/// Encode `n` as an uppercase base-36 string.
fn to_base36(mut n: u64) -> String {
    const D: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    if n == 0 {
        return "0".to_string();
    }
    let mut bytes = Vec::new();
    while n > 0 {
        bytes.push(D[(n % 36) as usize]);
        n /= 36;
    }
    bytes.reverse();
    String::from_utf8(bytes).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_full_valid(grid: &Grid) -> bool {
        grid.iter().all(|&v| (1..=9).contains(&v))
            && (0..81).all(|i| {
                let v = grid[i];
                let mut tmp = *grid;
                tmp[i] = 0;
                valid(&tmp, i, v)
            })
    }

    #[test]
    fn fill_produces_valid_solution() {
        let mut grid: Grid = [0; 81];
        assert!(fill(&mut grid, &mut rand::thread_rng()));
        assert!(is_full_valid(&grid));
    }

    #[test]
    fn same_seed_reproduces_the_same_puzzle() {
        for diff in Difficulty::ALL {
            let a = generate_seeded(diff, 123_456);
            let b = generate_seeded(diff, 123_456);
            assert_eq!(a.solution, b.solution);
            assert_eq!(a.seed, 123_456);
            for i in 0..81 {
                assert_eq!(a.board.value(i / 9, i % 9), b.board.value(i / 9, i % 9));
            }
        }
        // A different seed should (essentially always) give a different board.
        let a = generate_seeded(Difficulty::Easy, 1);
        let b = generate_seeded(Difficulty::Easy, 2);
        assert_ne!(a.solution, b.solution);
    }

    #[test]
    fn share_code_roundtrips() {
        for diff in Difficulty::ALL {
            for &seed in &[0u32, 1, 42, 123_456, u32::MAX] {
                let code = encode_code(diff, seed);
                assert_eq!(decode_code(&code), Some((diff, seed)));
                // Case-insensitive, and tolerant of a '#' prefix and trailing text.
                let messy = format!("Puzzle #{} — come beat my time!", code.to_lowercase());
                assert_eq!(decode_code(&messy), Some((diff, seed)));
            }
        }
        assert_eq!(decode_code(""), None);
        assert_eq!(decode_code("###"), None);
    }

    #[test]
    fn generated_puzzles_are_unique_and_solvable() {
        for diff in [
            Difficulty::Easy,
            Difficulty::Medium,
            Difficulty::Hard,
            Difficulty::Expert,
        ] {
            let p = generate(diff);
            assert!(is_full_valid(&p.solution));

            // Reconstruct the flat puzzle from the board's givens.
            let mut grid: Grid = [0; 81];
            for r in 0..9 {
                for c in 0..9 {
                    if let Some(v) = p.board.value(r, c) {
                        grid[r * 9 + c] = v;
                    }
                }
            }
            // Exactly one solution, and it equals the stored solution.
            let mut probe = grid;
            assert_eq!(count_solutions(&mut probe, 2), 1);
            let mut solved = grid;
            fill(&mut solved, &mut rand::thread_rng());
            assert_eq!(solved, p.solution);
        }
    }
}
