# sudoku

A terminal Sudoku with vim-style navigation, built with [ratatui](https://ratatui.rs).

## Features

- Automatic puzzle generation with a guaranteed unique solution, in four
  difficulties (Easy / Medium / Hard / Expert).
- Vim-style keyboard navigation.
- Live highlighting of the selected cell's row, column, and 3×3 box, plus every
  cell sharing the selected number. An optional match-highlight mode (`v`)
  extends this to the row, column, and box of every cell holding that number.
- Conflict highlighting, and an optional error-check mode that flags entries
  disagreeing with the solution.
- Pencil-mark notes shown at fixed positions within each cell.
- A running count of how many of each digit 1–9 remain to be placed.
- A difficulty selector menu, undo / redo, hints, and a running timer.
- A celebratory completion animation: a rainbow wave sweeps the board and a
  banner pops in when you solve the puzzle.
- A Wordle-style shareable result on completion (`s`): a spoiler-free emoji
  grid of clues vs. your placements vs. hints, plus your time, mistakes, hints,
  and which assists you used — copied to the system clipboard.
- Reproducible puzzles via short share codes. Every puzzle has a code (shown in
  the side panel and included in the shared result); press `p` to type or paste
  a code and play the exact same puzzle a friend did.
- A responsive layout: the board scales to the terminal size (and the side
  panel hides when the window is too narrow). Pencil-mark notes are shown when
  the window is tall enough for full-size cells.

## Running

```sh
cargo run --release
```

## Controls

| Key            | Action                              |
| -------------- | ----------------------------------- |
| `h` `j` `k` `l`| Move cursor (arrow keys also work)  |
| `w` / `b`      | Jump one 3×3 box right / left       |
| `0` / `$`      | Jump to start / end of row          |
| `gg` / `G`     | Jump to top / bottom row            |
| `f` `1`–`9`    | Jump to the nearest cell with that number |
| `1`–`9`        | Place a value (or toggle a note)    |
| `m`            | Toggle note mode                    |
| `x` / `Del`    | Clear the current cell              |
| `u` / `Ctrl-r` | Undo / redo                         |
| `H`            | Reveal the current cell (hint)      |
| `c`            | Toggle error checking               |
| `v`            | Toggle match highlighting           |
| `d`            | Open the difficulty selector        |
| `n` / `N`      | New game / cycle difficulty         |
| `s`            | Share result (after solving)        |
| `p`            | Play a shared puzzle code           |
| `?`            | Toggle the help popup               |
| `q`            | Quit                                |

In **note mode**, digits `1`–`9` toggle pencil marks on the selected empty cell
instead of committing a value.
