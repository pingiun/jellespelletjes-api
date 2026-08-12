//! Daily puzzle generator for lettersoep.
//!
//! The authoritative generator: the lettersoep client only scores moves
//! (src/engine/score.ts in the lettersoep repo, which the scoring below
//! mirrors); puzzles are generated here and served as JSON. Seeds derive
//! from the date via FNV-1a and drive a Mulberry32 RNG, so a date maps to
//! one puzzle forever; the regression test at the bottom pins day one's
//! output. Scoring changes must be mirrored in the client's score.ts.

use chrono::NaiveDate;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

pub const GENERATOR_VERSION: &str = "v1";

/// Launch day: puzzle #1 appears on this UTC date (placeholder for now).
pub fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()
}

// ---------------------------------------------------------------- rng

/// FNV-1a 32-bit hash of a string's UTF-8 bytes.
pub fn fnv1a(input: &str) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    for byte in input.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

/// Mulberry32, matching the TypeScript implementation exactly.
pub struct Rng {
    state: u32,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_add(0x6d2b79f5);
        let mut z = self.state;
        z = (z ^ (z >> 15)).wrapping_mul(z | 1);
        z ^= z.wrapping_add((z ^ (z >> 7)).wrapping_mul(z | 61));
        z ^ (z >> 14)
    }

    /// Integer in [0, bound) without modulo bias, via rejection sampling.
    pub fn next_below(&mut self, bound: u32) -> u32 {
        let limit = (4_294_967_296u64 - (4_294_967_296u64 % bound as u64)) as u64;
        let mut value = self.next_u32() as u64;
        while value >= limit {
            value = self.next_u32() as u64;
        }
        (value % bound as u64) as u32
    }

    /// In-place Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.next_below((i + 1) as u32) as usize;
            items.swap(i, j);
        }
    }
}

pub fn seed_for_date(date: NaiveDate) -> u32 {
    fnv1a(&format!("lettersoep:{GENERATOR_VERSION}:{date}"))
}

pub fn puzzle_number(date: NaiveDate) -> i64 {
    (date - epoch()).num_days() + 1
}

// ---------------------------------------------------------------- letters

/// Dutch Scrabble letter values.
fn letter_value(letter: u8) -> u32 {
    match letter {
        b'A' | b'E' | b'I' | b'N' | b'O' => 1,
        b'D' | b'R' | b'S' | b'T' => 2,
        b'B' | b'G' | b'K' | b'L' | b'M' | b'P' => 3,
        b'F' | b'H' | b'J' | b'U' | b'V' | b'Z' => 4,
        b'C' | b'W' => 5,
        b'X' | b'Y' => 8,
        b'Q' => 10,
        _ => 0,
    }
}

/// Official Dutch Scrabble bag (blanks left out), in A..Z order — the order
/// matters because the bag Vec feeds the shuffle.
const BAG_COUNTS: [(u8, u8); 26] = [
    (b'A', 6), (b'B', 2), (b'C', 2), (b'D', 5), (b'E', 18), (b'F', 2), (b'G', 3),
    (b'H', 2), (b'I', 4), (b'J', 2), (b'K', 3), (b'L', 3), (b'M', 3), (b'N', 10),
    (b'O', 6), (b'P', 2), (b'Q', 1), (b'R', 5), (b'S', 5), (b'T', 5), (b'U', 3),
    (b'V', 2), (b'W', 2), (b'X', 1), (b'Y', 1), (b'Z', 2),
];

fn is_vowel(letter: u8) -> bool {
    matches!(letter, b'A' | b'E' | b'I' | b'O' | b'U')
}

// ---------------------------------------------------------------- board

const BOARD_SIZE: i32 = 15;
const CENTER: i32 = 7;

#[derive(Clone, Copy, PartialEq)]
enum Premium {
    Tw,
    Dw,
    Tl,
    Dl,
}

/// Official Scrabble premium layout, upper-left quadrant, mirrored.
fn premium_at(row: i32, col: i32) -> Option<Premium> {
    use Premium::*;
    const Q: [[Option<Premium>; 8]; 8] = [
        [Some(Tw), None, None, Some(Dl), None, None, None, Some(Tw)],
        [None, Some(Dw), None, None, None, Some(Tl), None, None],
        [None, None, Some(Dw), None, None, None, Some(Dl), None],
        [Some(Dl), None, None, Some(Dw), None, None, None, Some(Dl)],
        [None, None, None, None, Some(Dw), None, None, None],
        [None, Some(Tl), None, None, None, Some(Tl), None, None],
        [None, None, Some(Dl), None, None, None, Some(Dl), None],
        [Some(Tw), None, None, Some(Dl), None, None, None, Some(Dw)],
    ];
    let mirror = |i: i32| if i <= CENTER { i } else { BOARD_SIZE - 1 - i };
    let (r, c) = (mirror(row), mirror(col));
    if (0..8).contains(&r) && (0..8).contains(&c) {
        Q[r as usize][c as usize]
    } else {
        None
    }
}

type Board = HashMap<(i32, i32), u8>;

#[derive(Clone, Copy, Serialize)]
pub struct Tile {
    pub row: i32,
    pub col: i32,
    #[serde(serialize_with = "serialize_letter")]
    pub letter: u8,
}

fn serialize_letter<S: serde::Serializer>(letter: &u8, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_char(*letter as char)
}

// ---------------------------------------------------------------- scoring

const BINGO_BONUS: u32 = 50;
const RACK_SIZE: usize = 7;

pub struct ScoredWord {
    pub word: String,
    pub score: u32,
}

pub struct Scored {
    pub words: Vec<ScoredWord>,
    pub bingo: bool,
    pub score: u32,
}

/// Port of score.ts scorePlacement: validate a placement's geometry against
/// the fixed board and score it (premiums only under new tiles, cross-words
/// count, 7 tiles = bingo).
pub fn score_placement(fixed: &Board, placed: &[Tile]) -> Option<Scored> {
    if placed.is_empty() {
        return None;
    }
    let placed_map: HashMap<(i32, i32), u8> =
        placed.iter().map(|t| ((t.row, t.col), t.letter)).collect();
    let letter_at = |row: i32, col: i32| -> Option<u8> {
        placed_map.get(&(row, col)).or_else(|| fixed.get(&(row, col))).copied()
    };

    let across = placed.iter().all(|t| t.row == placed[0].row);
    if !across && !placed.iter().all(|t| t.col == placed[0].col) {
        return None;
    }

    // Contiguous from first to last new tile, with fixed tiles filling gaps.
    let fixed_axis = if across { placed[0].row } else { placed[0].col };
    let mut positions: Vec<i32> =
        placed.iter().map(|t| if across { t.col } else { t.row }).collect();
    positions.sort_unstable();
    for p in positions[0]..=positions[positions.len() - 1] {
        let present =
            if across { letter_at(fixed_axis, p) } else { letter_at(p, fixed_axis) };
        if present.is_none() {
            return None;
        }
    }

    // No overlap with fixed tiles; must touch at least one.
    if placed.iter().any(|t| fixed.contains_key(&(t.row, t.col))) {
        return None;
    }
    let touches = placed.iter().any(|t| {
        fixed.contains_key(&(t.row, t.col - 1))
            || fixed.contains_key(&(t.row, t.col + 1))
            || fixed.contains_key(&(t.row - 1, t.col))
            || fixed.contains_key(&(t.row + 1, t.col))
    });
    if !touches {
        return None;
    }

    let score_word = |seed: &Tile, d_row: i32, d_col: i32| -> Option<ScoredWord> {
        let (mut row, mut col) = (seed.row, seed.col);
        while letter_at(row - d_row, col - d_col).is_some() {
            row -= d_row;
            col -= d_col;
        }
        let mut word = String::new();
        let mut score = 0u32;
        let mut multiplier = 1u32;
        while let Some(letter) = letter_at(row, col) {
            word.push(letter as char);
            let premium =
                if placed_map.contains_key(&(row, col)) { premium_at(row, col) } else { None };
            let mut value = letter_value(letter);
            match premium {
                Some(Premium::Dl) => value *= 2,
                Some(Premium::Tl) => value *= 3,
                Some(Premium::Dw) => multiplier *= 2,
                Some(Premium::Tw) => multiplier *= 3,
                None => {}
            }
            score += value;
            row += d_row;
            col += d_col;
        }
        if word.len() < 2 {
            return None;
        }
        Some(ScoredWord { word, score: score * multiplier })
    };

    let mut words = Vec::new();
    let (d_row, d_col) = if across { (0, 1) } else { (1, 0) };
    if let Some(main) = score_word(&placed[0], d_row, d_col) {
        words.push(main);
    }
    for tile in placed {
        if let Some(cross) = score_word(tile, d_col, d_row) {
            words.push(cross);
        }
    }
    if words.is_empty() {
        return None;
    }

    let bingo = placed.len() == RACK_SIZE;
    let score =
        words.iter().map(|w| w.score).sum::<u32>() + if bingo { BINGO_BONUS } else { 0 };
    Some(Scored { words, bingo, score })
}

// ---------------------------------------------------------------- moves

pub struct Move {
    pub tiles: Vec<Tile>,
    pub words: Vec<ScoredWord>,
    pub score: u32,
    pub bingo: bool,
}

fn count_letters(letters: impl Iterator<Item = u8>) -> [u16; 26] {
    let mut counts = [0u16; 26];
    for letter in letters {
        counts[(letter - b'A') as usize] += 1;
    }
    counts
}

/// Port of moves.ts findAllMoves: every dictionary word, in each
/// orientation, at each position where it fits the fixed tiles and rack.
pub fn find_all_moves(fixed: &Board, rack: &[u8], dict: &HashSet<&str>) -> Vec<Move> {
    let rack_counts = count_letters(rack.iter().copied());
    let board_counts = count_letters(fixed.values().copied());

    let max_len = (BOARD_SIZE as usize).min(rack.len() + fixed.len());
    let candidates: Vec<&str> = dict
        .iter()
        .filter(|word| {
            let len = word.len();
            if !(2..=max_len).contains(&len) {
                return false;
            }
            let counts = count_letters(word.bytes());
            counts
                .iter()
                .enumerate()
                .all(|(i, &n)| n <= rack_counts[i] + board_counts[i])
        })
        .copied()
        .collect();

    let mut near_fixed: HashSet<(i32, i32)> = HashSet::new();
    for &(row, col) in fixed.keys() {
        for (dr, dc) in [(0, 0), (0, 1), (0, -1), (1, 0), (-1, 0)] {
            near_fixed.insert((row + dr, col + dc));
        }
    }

    let mut moves: HashMap<String, Move> = HashMap::new();
    for word in &candidates {
        for across in [true, false] {
            let (d_row, d_col) = if across { (0, 1) } else { (1, 0) };
            let len = word.len() as i32;
            let row_max = if across { BOARD_SIZE } else { BOARD_SIZE - len + 1 };
            let col_max = if across { BOARD_SIZE - len + 1 } else { BOARD_SIZE };
            for row in 0..row_max {
                for col in 0..col_max {
                    let Some(tiles) =
                        fit_word(fixed, &rack_counts, &near_fixed, word, row, col, d_row, d_col)
                    else {
                        continue;
                    };
                    let Some(scored) = score_placement(fixed, &tiles) else { continue };
                    if !scored.words.iter().all(|w| dict.contains(w.word.as_str())) {
                        continue;
                    }
                    let mut parts: Vec<String> = tiles
                        .iter()
                        .map(|t| format!("{},{},{}", t.row, t.col, t.letter as char))
                        .collect();
                    parts.sort();
                    moves.entry(parts.join(";")).or_insert(Move {
                        tiles,
                        words: scored.words,
                        score: scored.score,
                        bingo: scored.bingo,
                    });
                }
            }
        }
    }
    let mut all: Vec<Move> = moves.into_values().collect();
    all.sort_by(|a, b| b.score.cmp(&a.score));
    all
}

#[allow(clippy::too_many_arguments)]
fn fit_word(
    fixed: &Board,
    rack_counts: &[u16; 26],
    near_fixed: &HashSet<(i32, i32)>,
    word: &str,
    row: i32,
    col: i32,
    d_row: i32,
    d_col: i32,
) -> Option<Vec<Tile>> {
    // The candidate must be the whole line segment.
    let len = word.len() as i32;
    if fixed.contains_key(&(row - d_row, col - d_col)) {
        return None;
    }
    if fixed.contains_key(&(row + d_row * len, col + d_col * len)) {
        return None;
    }

    let mut tiles = Vec::new();
    let mut rack_use = [0u16; 26];
    let mut touches = false;
    for (i, letter) in word.bytes().enumerate() {
        let r = row + d_row * i as i32;
        let c = col + d_col * i as i32;
        if near_fixed.contains(&(r, c)) {
            touches = true;
        }
        if let Some(&existing) = fixed.get(&(r, c)) {
            if existing != letter {
                return None;
            }
            continue;
        }
        let idx = (letter - b'A') as usize;
        rack_use[idx] += 1;
        if rack_use[idx] > rack_counts[idx] {
            return None;
        }
        tiles.push(Tile { row: r, col: c, letter });
    }
    if tiles.is_empty() || !touches {
        return None;
    }
    Some(tiles)
}

// ---------------------------------------------------------------- generator

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacedWord {
    pub row: i32,
    pub col: i32,
    pub dir: &'static str,
    pub word: String,
}

#[derive(Serialize)]
pub struct ViewRect {
    pub top: i32,
    pub left: i32,
    pub rows: i32,
    pub cols: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyPuzzle {
    pub date: String,
    pub number: i64,
    pub seed: u32,
    pub view: ViewRect,
    pub placed: Vec<PlacedWord>,
    pub rack: Vec<char>,
    pub max_score: u32,
    pub move_count: usize,
    pub valid_words: Vec<String>,
}

const VIEW_ROWS: i32 = 9;
const VIEW_COLS: i32 = 8;
const MIN_MOVES: usize = 100;
const MIN_MAX_SCORE: u32 = 30;
const MAX_ATTEMPTS: u32 = 100;

static DICT: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    include_str!("../../data/lettersoep/words.txt")
        .split('\n')
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .collect()
});

/// Opening words: the curated woordle five-letter list, uppercased, kept in
/// file order (the RNG indexes into it).
static OPENINGS: LazyLock<Vec<String>> = LazyLock::new(|| {
    include_str!("../../data/woordle/puzzle-words")
        .split('\n')
        .map(|w| w.trim().to_uppercase())
        .filter(|w| w.len() == 5 && DICT.contains(w.as_str()))
        .collect()
});

pub fn daily_puzzle(date: NaiveDate) -> anyhow::Result<DailyPuzzle> {
    let seed = seed_for_date(date);
    let mut rng = Rng::new(seed);
    for _ in 1..=MAX_ATTEMPTS {
        let Some(placed) = place_opening_words(&mut rng, &OPENINGS) else { continue };
        let board = build_board(&placed);
        let used: Vec<u8> = board.values().copied().collect();
        let rack = draw_rack(&mut rng, &used);
        let moves = find_all_moves(&board, &rack, &DICT);
        if moves.len() < MIN_MOVES {
            continue;
        }
        let max_score = moves[0].score;
        if max_score < MIN_MAX_SCORE {
            continue;
        }
        let valid_words: BTreeSet<String> = moves
            .iter()
            .flat_map(|m| m.words.iter().map(|w| w.word.clone()))
            .collect();
        return Ok(DailyPuzzle {
            date: date.to_string(),
            number: puzzle_number(date),
            seed,
            view: pick_view(&mut rng, &board),
            placed,
            rack: rack.iter().map(|&l| l as char).collect(),
            max_score,
            move_count: moves.len(),
            valid_words: valid_words.into_iter().collect(),
        });
    }
    anyhow::bail!("no acceptable puzzle found for seed {seed} in {MAX_ATTEMPTS} attempts")
}

fn place_opening_words(rng: &mut Rng, openings: &[String]) -> Option<Vec<PlacedWord>> {
    let first = &openings[rng.next_below(openings.len() as u32) as usize];
    let center_index = rng.next_below(first.len() as u32) as i32;
    let first_col = CENTER - center_index;

    for _ in 0..20 {
        let second = &openings[rng.next_below(openings.len() as u32) as usize];
        if second == first {
            continue;
        }
        let mut crossings = Vec::new();
        for (i1, l1) in first.bytes().enumerate() {
            for (i2, l2) in second.bytes().enumerate() {
                if l1 == l2 {
                    crossings.push((i1 as i32, i2 as i32));
                }
            }
        }
        if crossings.is_empty() {
            continue;
        }
        let (i1, i2) = crossings[rng.next_below(crossings.len() as u32) as usize];
        let second_row = CENTER - i2;
        if second_row < 1 || second_row + second.len() as i32 > BOARD_SIZE - 1 {
            continue;
        }
        if first_col < 1 || first_col + first.len() as i32 > BOARD_SIZE - 1 {
            continue;
        }
        return Some(vec![
            PlacedWord { row: CENTER, col: first_col, dir: "across", word: first.clone() },
            PlacedWord { row: second_row, col: first_col + i1, dir: "down", word: second.clone() },
        ]);
    }
    None
}

fn build_board(placed: &[PlacedWord]) -> Board {
    let mut board = Board::new();
    for p in placed {
        for (i, letter) in p.word.bytes().enumerate() {
            let (r, c) = match p.dir {
                "down" => (p.row + i as i32, p.col),
                _ => (p.row, p.col + i as i32),
            };
            board.insert((r, c), letter);
        }
    }
    board
}

fn draw_rack(rng: &mut Rng, used: &[u8]) -> Vec<u8> {
    let mut remaining: HashMap<u8, u8> = BAG_COUNTS.iter().copied().collect();
    for letter in used {
        if let Some(n) = remaining.get_mut(letter) {
            if *n > 0 {
                *n -= 1;
            }
        }
    }
    // Bag built in A..Z order, exactly like the TS Object.entries iteration.
    let mut bag: Vec<u8> = Vec::new();
    for (letter, _) in BAG_COUNTS {
        for _ in 0..remaining[&letter] {
            bag.push(letter);
        }
    }
    for _ in 0..40 {
        rng.shuffle(&mut bag);
        let rack = &bag[..RACK_SIZE];
        let vowels = rack.iter().filter(|&&l| is_vowel(l)).count();
        if (2..=4).contains(&vowels) {
            return rack.to_vec();
        }
    }
    bag[..RACK_SIZE].to_vec()
}

fn pick_view(rng: &mut Rng, board: &Board) -> ViewRect {
    let mut min_row = BOARD_SIZE;
    let mut max_row = 0;
    let mut min_col = BOARD_SIZE;
    let mut max_col = 0;
    for &(row, col) in board.keys() {
        min_row = min_row.min(row);
        max_row = max_row.max(row);
        min_col = min_col.min(col);
        max_col = max_col.max(col);
    }
    let rows = VIEW_ROWS.max(max_row - min_row + 3);
    let cols = VIEW_COLS.max(max_col - min_col + 3);
    let top_slack = rows - (max_row - min_row + 1);
    let left_slack = cols - (max_col - min_col + 1);
    let top = (min_row - 1 - rng.next_below(1.max(top_slack - 1) as u32) as i32)
        .clamp(0, BOARD_SIZE - rows);
    let left = (min_col - 1 - rng.next_below(1.max(left_slack - 1) as u32) as i32)
        .clamp(0, BOARD_SIZE - cols);
    ViewRect { top, left, rows, cols }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression pin: day one's puzzle, cross-checked at porting time
    /// against the original TypeScript generator. If this changes, a code
    /// change silently remapped every date to a different puzzle.
    #[test]
    fn day_one_is_stable() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let puzzle = daily_puzzle(date).unwrap();
        assert_eq!(puzzle.seed, 3068585657);
        assert_eq!(puzzle.number, 1);
        let words: Vec<(&str, i32, i32, &str)> = puzzle
            .placed
            .iter()
            .map(|p| (p.word.as_str(), p.row, p.col, p.dir))
            .collect();
        assert_eq!(words, vec![("PAUZE", 7, 3, "across"), ("VLOER", 4, 7, "down")]);
        assert_eq!(puzzle.rack, vec!['R', 'E', 'G', 'Z', 'E', 'E', 'O']);
        assert_eq!(
            (puzzle.view.top, puzzle.view.left, puzzle.view.rows, puzzle.view.cols),
            (3, 1, 9, 8)
        );
        assert_eq!(puzzle.max_score, 76);
        assert_eq!(puzzle.move_count, 374);
        assert_eq!(puzzle.valid_words.len(), 312);
        assert_eq!(&puzzle.valid_words[..3], ["AGEER", "AGERE", "AR"]);
    }

    #[test]
    fn seed_derivation_is_stable() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 2).unwrap();
        assert_eq!(seed_for_date(date), 3018252800);
    }
}
