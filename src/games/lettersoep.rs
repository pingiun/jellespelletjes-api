//! Daily puzzle generator for lettersoep.
//!
//! The authoritative generator: the lettersoep client only scores moves
//! (src/engine/score.ts in the lettersoep repo, which the scoring below
//! mirrors); puzzles are generated here and served as JSON. Seeds derive
//! from the date via FNV-1a and drive a Mulberry32 RNG, so a date maps to
//! one puzzle forever; the regression test at the bottom pins day one's
//! output. Scoring changes must be mirrored in the client's score.ts.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

/// The two lettersoep games: Dutch (lettersoep, SWL word list) and English
/// (lettersoup, ENABLE2K). Each language is its own game with its own word
/// lists, letter bag, letter values and generator era; the board layout and
/// generation algorithm are shared.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Nl,
    En,
}

impl Lang {
    pub fn from_game(game: &str) -> Option<Lang> {
        match game {
            "lettersoep" => Some(Lang::Nl),
            "lettersoup" => Some(Lang::En),
            _ => None,
        }
    }

    pub fn game(self) -> &'static str {
        match self {
            Lang::Nl => "lettersoep",
            Lang::En => "lettersoup",
        }
    }

    /// Generator era per language. Bump on any generation change, so a
    /// change is an explicit new era rather than silently remapping dates.
    pub fn version(self) -> &'static str {
        match self {
            Lang::Nl => "v4", // v4: frequency-filtered pool, view targets the best move
            Lang::En => "v1",
        }
    }

    fn dict(self) -> &'static HashSet<&'static str> {
        match self {
            Lang::Nl => &DICT_NL,
            Lang::En => &DICT_EN,
        }
    }

    fn word_index(self) -> &'static [WordEntry] {
        match self {
            Lang::Nl => &WORD_INDEX_NL,
            Lang::En => &WORD_INDEX_EN,
        }
    }

    fn openings(self) -> &'static [(String, f32)] {
        match self {
            Lang::Nl => &OPENINGS_NL,
            Lang::En => &OPENINGS_EN,
        }
    }

    fn bag(self) -> &'static [(u8, u8); 26] {
        match self {
            Lang::Nl => &BAG_NL,
            Lang::En => &BAG_EN,
        }
    }

    fn letter_value(self, letter: u8) -> u32 {
        match self {
            Lang::Nl => letter_value_nl(letter),
            Lang::En => letter_value_en(letter),
        }
    }
}

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

pub fn seed_for_date(date: NaiveDate, lang: Lang) -> u32 {
    fnv1a(&format!("{}:{}:{date}", lang.game(), lang.version()))
}

pub fn puzzle_number(date: NaiveDate) -> i64 {
    (date - epoch()).num_days() + 1
}

// ---------------------------------------------------------------- letters

/// Dutch Scrabble letter values.
fn letter_value_nl(letter: u8) -> u32 {
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

/// Official English Scrabble letter values.
fn letter_value_en(letter: u8) -> u32 {
    match letter {
        b'A' | b'E' | b'I' | b'L' | b'N' | b'O' | b'R' | b'S' | b'T' | b'U' => 1,
        b'D' | b'G' => 2,
        b'B' | b'C' | b'M' | b'P' => 3,
        b'F' | b'H' | b'V' | b'W' | b'Y' => 4,
        b'K' => 5,
        b'J' | b'X' => 8,
        b'Q' | b'Z' => 10,
        _ => 0,
    }
}

/// Official Scrabble bags (blanks left out), in A..Z order — the order
/// matters because the bag Vec feeds the shuffle.
const BAG_NL: [(u8, u8); 26] = [
    (b'A', 6), (b'B', 2), (b'C', 2), (b'D', 5), (b'E', 18), (b'F', 2), (b'G', 3),
    (b'H', 2), (b'I', 4), (b'J', 2), (b'K', 3), (b'L', 3), (b'M', 3), (b'N', 10),
    (b'O', 6), (b'P', 2), (b'Q', 1), (b'R', 5), (b'S', 5), (b'T', 5), (b'U', 3),
    (b'V', 2), (b'W', 2), (b'X', 1), (b'Y', 1), (b'Z', 2),
];

const BAG_EN: [(u8, u8); 26] = [
    (b'A', 9), (b'B', 2), (b'C', 2), (b'D', 4), (b'E', 12), (b'F', 2), (b'G', 3),
    (b'H', 2), (b'I', 9), (b'J', 1), (b'K', 1), (b'L', 4), (b'M', 2), (b'N', 6),
    (b'O', 8), (b'P', 2), (b'Q', 1), (b'R', 6), (b'S', 4), (b'T', 6), (b'U', 4),
    (b'V', 2), (b'W', 2), (b'X', 1), (b'Y', 2), (b'Z', 1),
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

/// Flat 15x15 array of the fixed tiles (0 = empty): the hot search path
/// probes millions of cells, and an indexed load beats hashing by an order
/// of magnitude.
struct Grid {
    cells: [u8; 225],
}

impl Grid {
    fn from_board(board: &Board) -> Self {
        let mut cells = [0u8; 225];
        for (&(row, col), &letter) in board {
            cells[(row * BOARD_SIZE + col) as usize] = letter;
        }
        Self { cells }
    }

    #[inline]
    fn at(&self, row: i32, col: i32) -> u8 {
        if (0..BOARD_SIZE).contains(&row) && (0..BOARD_SIZE).contains(&col) {
            self.cells[(row * BOARD_SIZE + col) as usize]
        } else {
            0
        }
    }

    #[inline]
    fn has(&self, row: i32, col: i32) -> bool {
        self.at(row, col) != 0
    }
}

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
pub fn score_placement(lang: Lang, fixed: &Board, placed: &[Tile]) -> Option<Scored> {
    score_placement_grid(lang, &Grid::from_board(fixed), placed)
}

fn score_placement_grid(lang: Lang, fixed: &Grid, placed: &[Tile]) -> Option<Scored> {
    if placed.is_empty() {
        return None;
    }
    // At most 7 placed tiles: linear scans beat any map.
    let placed_at = |row: i32, col: i32| -> Option<u8> {
        placed.iter().find(|t| t.row == row && t.col == col).map(|t| t.letter)
    };
    let letter_at = |row: i32, col: i32| -> Option<u8> {
        placed_at(row, col).or_else(|| {
            let l = fixed.at(row, col);
            (l != 0).then_some(l)
        })
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
    if placed.iter().any(|t| fixed.has(t.row, t.col)) {
        return None;
    }
    let touches = placed.iter().any(|t| {
        fixed.has(t.row, t.col - 1)
            || fixed.has(t.row, t.col + 1)
            || fixed.has(t.row - 1, t.col)
            || fixed.has(t.row + 1, t.col)
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
                if placed_at(row, col).is_some() { premium_at(row, col) } else { None };
            let mut value = lang.letter_value(letter);
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

/// Per-word signature, precomputed once so each generation attempt can
/// prune the 1.1M-word list with a mask test instead of recounting. Counts
/// are padded to 32 bytes so the availability test runs as four u64 SWAR
/// comparisons instead of a 26-iteration loop.
pub struct WordEntry {
    pub word: &'static str,
    len: u8,
    /// Bit i set = letter (b'A' + i) occurs in the word.
    mask: u32,
    counts: [u8; 32],
}

pub fn index_words(words: impl Iterator<Item = &'static str>) -> Vec<WordEntry> {
    words
        .map(|word| {
            let mut counts = [0u8; 32];
            let mut mask = 0u32;
            for letter in word.bytes() {
                let i = (letter - b'A') as usize;
                counts[i] += 1;
                mask |= 1 << i;
            }
            WordEntry { word, len: word.len() as u8, mask, counts }
        })
        .collect()
}

/// Per-byte `needed[i] <= avail[i]` over all 32 bytes, as four u64 SWAR
/// steps. Valid because both sides stay below 128 (max letter count is 25).
#[inline]
fn counts_fit(needed: &[u8; 32], avail: &[u8; 32]) -> bool {
    const HIGH: u64 = 0x8080_8080_8080_8080;
    for k in 0..4 {
        let a = u64::from_le_bytes(avail[k * 8..k * 8 + 8].try_into().unwrap());
        let n = u64::from_le_bytes(needed[k * 8..k * 8 + 8].try_into().unwrap());
        // With high bits forced on, a byte only loses its high bit when the
        // subtraction borrows, i.e. when needed > avail.
        if ((a | HIGH).wrapping_sub(n)) & HIGH != HIGH {
            return false;
        }
    }
    true
}

/// Port of moves.ts findAllMoves: every dictionary word, in each
/// orientation, at each position where it fits the fixed tiles and rack.
/// Two indexes keep it fast: precomputed word signatures (mask + counts)
/// prune candidates, and per-line anchor ranges limit placements to lines
/// that can actually touch the board.
pub fn find_all_moves(
    lang: Lang,
    fixed: &Board,
    rack: &[u8],
    index: &[WordEntry],
    dict: &HashSet<&str>,
) -> Vec<Move> {
    let grid = Grid::from_board(fixed);
    let rack_counts = count_letters(rack.iter().copied());
    let board_counts = count_letters(fixed.values().copied());
    let mut avail = [0u8; 32];
    let mut avail_mask = 0u32;
    for i in 0..26 {
        avail[i] = (rack_counts[i] + board_counts[i]) as u8;
        if avail[i] > 0 {
            avail_mask |= 1 << i;
        }
    }

    let max_len = (BOARD_SIZE as usize).min(rack.len() + fixed.len());
    let candidates: Vec<&WordEntry> = index
        .iter()
        .filter(|e| {
            (2..=max_len).contains(&(e.len as usize))
                && e.mask & !avail_mask == 0
                && counts_fit(&e.counts, &avail)
        })
        .collect();

    let mut near_fixed = [false; 225];
    for &(row, col) in fixed.keys() {
        for (dr, dc) in [(0, 0), (0, 1), (0, -1), (1, 0), (-1, 0)] {
            let (r, c) = (row + dr, col + dc);
            if (0..BOARD_SIZE).contains(&r) && (0..BOARD_SIZE).contains(&c) {
                near_fixed[(r * BOARD_SIZE + c) as usize] = true;
            }
        }
    }
    // Anchor ranges: a placement must cover a near-fixed cell, so an across
    // word only fits on rows that have one, at columns overlapping them
    // (and likewise for down words).
    let mut row_span: HashMap<i32, (i32, i32)> = HashMap::new();
    let mut col_span: HashMap<i32, (i32, i32)> = HashMap::new();
    for row in 0..BOARD_SIZE {
        for col in 0..BOARD_SIZE {
            if !near_fixed[(row * BOARD_SIZE + col) as usize] {
                continue;
            }
            let r = row_span.entry(row).or_insert((col, col));
            r.0 = r.0.min(col);
            r.1 = r.1.max(col);
            let c = col_span.entry(col).or_insert((row, row));
            c.0 = c.0.min(row);
            c.1 = c.1.max(row);
        }
    }

    let mut moves: HashMap<String, Move> = HashMap::new();
    for entry in &candidates {
        let word = entry.word;
        let len = entry.len as i32;
        for across in [true, false] {
            let (d_row, d_col) = if across { (0, 1) } else { (1, 0) };
            let span = if across { &row_span } else { &col_span };
            for (&line, &(lo, hi)) in span {
                let from = (lo - len + 1).max(0);
                let to = hi.min(BOARD_SIZE - len);
                for pos in from..=to {
                    let (row, col) = if across { (line, pos) } else { (pos, line) };
                    let Some(tiles) =
                        fit_word(&grid, &rack_counts, &near_fixed, word, row, col, d_row, d_col)
                    else {
                        continue;
                    };
                    let Some(scored) = score_placement_grid(lang, &grid, &tiles) else { continue };
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
    // Deterministic order: score descending, canonical tile key as the
    // tie-break (the view targets moves[0], so ties must not be arbitrary).
    let mut all: Vec<(String, Move)> = moves.into_iter().collect();
    all.sort_by(|a, b| b.1.score.cmp(&a.1.score).then_with(|| a.0.cmp(&b.0)));
    all.into_iter().map(|(_, m)| m).collect()
}

#[allow(clippy::too_many_arguments)]
fn fit_word(
    fixed: &Grid,
    rack_counts: &[u16; 26],
    near_fixed: &[bool; 225],
    word: &str,
    row: i32,
    col: i32,
    d_row: i32,
    d_col: i32,
) -> Option<Vec<Tile>> {
    // The candidate must be the whole line segment.
    let len = word.len() as i32;
    if fixed.has(row - d_row, col - d_col) {
        return None;
    }
    if fixed.has(row + d_row * len, col + d_col * len) {
        return None;
    }

    let mut tiles = Vec::new();
    let mut rack_use = [0u16; 26];
    let mut touches = false;
    for (i, letter) in word.bytes().enumerate() {
        let r = row + d_row * i as i32;
        let c = col + d_col * i as i32;
        if near_fixed[(r * BOARD_SIZE + c) as usize] {
            touches = true;
        }
        let existing = fixed.at(r, c);
        if existing != 0 {
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
    pub generator_version: &'static str,
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

fn parse_dict(raw: &'static str) -> HashSet<&'static str> {
    raw.split('\n').map(str::trim).filter(|w| !w.is_empty()).collect()
}

static DICT_NL: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| parse_dict(include_str!("../../data/lettersoep/words.txt")));
static DICT_EN: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| parse_dict(include_str!("../../data/lettersoep/words-en.txt")));

static WORD_INDEX_NL: LazyLock<Vec<WordEntry>> =
    LazyLock::new(|| index_words(DICT_NL.iter().copied()));
static WORD_INDEX_EN: LazyLock<Vec<WordEntry>> =
    LazyLock::new(|| index_words(DICT_EN.iter().copied()));

/// Opening words: SWL words of 5-8 letters that contain a curated woordle
/// word as a substring AND clear a real-world frequency bar (wordfreq
/// Dutch zipf, second column; only >= 1.2 included, which keeps real-but-
/// rare words and drops fabricated compounds). The closest available proxy
/// for words people actually lay in Wordfeud, since no public played-game
/// data exists. File order feeds the RNG.
fn parse_openings(raw: &'static str) -> Vec<(String, f32)> {
    raw.split('\n')
        .filter_map(|line| {
            let mut parts = line.trim().split(' ');
            let word = parts.next()?.to_string();
            let zipf: f32 = parts.next()?.parse().ok()?;
            (!word.is_empty()).then_some((word, zipf))
        })
        .collect()
}

static OPENINGS_NL: LazyLock<Vec<(String, f32)>> =
    LazyLock::new(|| parse_openings(include_str!("../../data/lettersoep/openings.txt")));
static OPENINGS_EN: LazyLock<Vec<(String, f32)>> =
    LazyLock::new(|| parse_openings(include_str!("../../data/lettersoep/openings-en.txt")));

static PUZZLES: LazyLock<Mutex<HashMap<(NaiveDate, Lang), Arc<DailyPuzzle>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The daily puzzle with an in-memory cache: generation costs ~1s, and both
/// the serving route and result verification need the same puzzle.
pub fn cached_puzzle(date: NaiveDate, lang: Lang) -> anyhow::Result<Arc<DailyPuzzle>> {
    if let Some(puzzle) = PUZZLES.lock().unwrap().get(&(date, lang)) {
        return Ok(puzzle.clone());
    }
    let puzzle = Arc::new(daily_puzzle(date, lang)?);
    PUZZLES.lock().unwrap().insert((date, lang), puzzle.clone());
    Ok(puzzle)
}

pub fn daily_puzzle(date: NaiveDate, lang: Lang) -> anyhow::Result<DailyPuzzle> {
    let seed = seed_for_date(date, lang);
    let mut rng = Rng::new(seed);
    for _ in 1..=MAX_ATTEMPTS {
        let Some(placed) = simulate_opening(&mut rng, lang) else { continue };
        let board = build_board(&placed);
        let used: Vec<u8> = board.values().copied().collect();
        let rack = draw_rack(&mut rng, &used, lang.bag());
        let moves = find_all_moves(lang, &board, &rack, lang.word_index(), lang.dict());
        if moves.len() < MIN_MOVES {
            continue;
        }
        let max_score = moves[0].score;
        if max_score < MIN_MAX_SCORE {
            continue;
        }
        // The daily hunt is the bingo: reject racks that cannot play one.
        if !moves.iter().any(|m| m.bingo) {
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
            generator_version: lang.version(),
            view: pick_view(&mut rng, &board, &moves[0]),
            placed,
            rack: rack.iter().map(|&l| l as char).collect(),
            max_score,
            move_count: moves.len(),
            valid_words: valid_words.into_iter().collect(),
        });
    }
    anyhow::bail!("no acceptable puzzle found for seed {seed} in {MAX_ATTEMPTS} attempts")
}

/// Simulate the opening of a running game: a first word through the center
/// square, then more words from the pool played as actual legal moves
/// (crossings or parallel plays whose cross-words all validate). Boards
/// spread organically instead of always crossing dead-center.
///
/// Each day has a board profile for variety: 3-7 words, a length flavor
/// (short, mixed, long) and a rarity flavor — most days common words, some
/// days easier, some days rarer words that reward a deep vocabulary.
fn simulate_opening(rng: &mut Rng, lang: Lang) -> Option<Vec<PlacedWord>> {
    let openings = lang.openings();
    let dict = lang.dict();
    let word_count = 3 + rng.next_below(5); // 3..=7 words on the board
    let len_flavor = rng.next_below(3); // 0 = short 5-6, 1 = mixed, 2 = long 7-8
    let rarity = rng.next_below(4); // 0-1 = normal, 2 = common, 3 = tricky
    let fits = |word: &str, zipf: f32| -> bool {
        let len_ok = match len_flavor {
            0 => word.len() <= 6,
            2 => word.len() >= 7,
            _ => true,
        };
        let zipf_ok = match rarity {
            2 => zipf >= 3.0,
            3 => (1.2..2.5).contains(&zipf),
            _ => zipf >= 2.0,
        };
        len_ok && zipf_ok
    };
    let mut pool: Vec<&String> =
        openings.iter().filter(|(w, z)| fits(w, *z)).map(|(w, _)| w).collect();
    if pool.len() < 100 {
        // Degenerate flavor combination: fall back to the length band alone.
        pool = openings
            .iter()
            .filter(|(w, z)| fits(w, *z) || *z >= 2.0)
            .map(|(w, _)| w)
            .collect();
    }

    let first = pool[rng.next_below(pool.len() as u32) as usize];
    let len = first.len() as i32;
    let center_index = rng.next_below(first.len() as u32) as i32;
    let across = rng.next_below(2) == 0;
    let start = CENTER - center_index;
    if start < 1 || start + len > BOARD_SIZE - 1 {
        return None;
    }
    let (row, col) = if across { (CENTER, start) } else { (start, CENTER) };
    let mut placed = vec![PlacedWord {
        row,
        col,
        dir: if across { "across" } else { "down" },
        word: first.clone(),
    }];
    let mut board = build_board(&placed);

    for _ in 1..word_count {
        let mut played = false;
        for _ in 0..30 {
            let word = pool[rng.next_below(pool.len() as u32) as usize];
            let options = enumerate_word_placements(lang, &board, word, dict);
            if options.is_empty() {
                continue;
            }
            let &(row, col, across) =
                &options[rng.next_below(options.len() as u32) as usize];
            placed.push(PlacedWord {
                row,
                col,
                dir: if across { "across" } else { "down" },
                word: word.clone(),
            });
            board = build_board(&placed);
            played = true;
            break;
        }
        if !played {
            return None;
        }
    }
    Some(placed)
}

/// Every legal placement of `word` on the board (as a real move: connected,
/// non-conflicting, all formed words in the dictionary), in deterministic
/// board order so the RNG's pick is reproducible.
fn enumerate_word_placements(
    lang: Lang,
    board: &Board,
    word: &str,
    dict: &HashSet<&str>,
) -> Vec<(i32, i32, bool)> {
    let grid = Grid::from_board(board);
    let rack_counts = count_letters(word.bytes());
    let mut near_fixed = [false; 225];
    for &(row, col) in board.keys() {
        for (dr, dc) in [(0, 0), (0, 1), (0, -1), (1, 0), (-1, 0)] {
            let (r, c) = (row + dr, col + dc);
            if (0..BOARD_SIZE).contains(&r) && (0..BOARD_SIZE).contains(&c) {
                near_fixed[(r * BOARD_SIZE + c) as usize] = true;
            }
        }
    }
    let len = word.len() as i32;
    let mut out = Vec::new();
    for across in [true, false] {
        for line in 0..BOARD_SIZE {
            for pos in 0..=(BOARD_SIZE - len) {
                let (row, col) = if across { (line, pos) } else { (pos, line) };
                let (d_row, d_col) = if across { (0, 1) } else { (1, 0) };
                let Some(tiles) =
                    fit_word(&grid, &rack_counts, &near_fixed, word, row, col, d_row, d_col)
                else {
                    continue;
                };
                let Some(scored) = score_placement_grid(lang, &grid, &tiles) else { continue };
                if !scored.words.iter().all(|w| dict.contains(w.word.as_str())) {
                    continue;
                }
                out.push((row, col, across));
            }
        }
    }
    out
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

fn draw_rack(rng: &mut Rng, used: &[u8], bag_counts: &[(u8, u8); 26]) -> Vec<u8> {
    let mut remaining: HashMap<u8, u8> = bag_counts.iter().copied().collect();
    for letter in used {
        if let Some(n) = remaining.get_mut(letter) {
            if *n > 0 {
                *n -= 1;
            }
        }
    }
    // Bag built in A..Z order, exactly like the original TS iteration.
    let mut bag: Vec<u8> = Vec::new();
    for &(letter, _) in bag_counts {
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

/// A tight window placed so the best-scoring move — including the fixed
/// letters its main word runs through — is fully visible, jittered within
/// the slack so the target's position gives nothing away. Other board words
/// may fall (partly) into the fog: you walked in mid-game.
fn pick_view(rng: &mut Rng, board: &Board, best: &Move) -> ViewRect {
    let grid = Grid::from_board(board);
    let mut min_row = BOARD_SIZE;
    let mut max_row = 0;
    let mut min_col = BOARD_SIZE;
    let mut max_col = 0;
    for t in &best.tiles {
        min_row = min_row.min(t.row);
        max_row = max_row.max(t.row);
        min_col = min_col.min(t.col);
        max_col = max_col.max(t.col);
    }
    // Extend along the main word's line over fixed letters at both ends.
    let across = best.tiles.iter().all(|t| t.row == best.tiles[0].row)
        && (best.tiles.len() > 1 || {
            let t = &best.tiles[0];
            grid.has(t.row, t.col - 1) || grid.has(t.row, t.col + 1)
        });
    if across {
        while grid.has(min_row, min_col - 1) {
            min_col -= 1;
        }
        while grid.has(max_row, max_col + 1) {
            max_col += 1;
        }
    } else {
        while grid.has(min_row - 1, min_col) {
            min_row -= 1;
        }
        while grid.has(max_row + 1, max_col) {
            max_row += 1;
        }
    }
    let rows = VIEW_ROWS.max(max_row - min_row + 1);
    let cols = VIEW_COLS.max(max_col - min_col + 1);
    let top_slack = rows - (max_row - min_row + 1);
    let left_slack = cols - (max_col - min_col + 1);
    let top = (min_row - rng.next_below((top_slack + 1) as u32) as i32)
        .clamp(0, BOARD_SIZE - rows);
    let left = (min_col - rng.next_below((left_slack + 1) as u32) as i32)
        .clamp(0, BOARD_SIZE - cols);
    ViewRect { top, left, rows, cols }
}

// ---------------------------------------------------------------- verify

#[derive(Deserialize)]
pub struct SubmittedTile {
    pub row: i32,
    pub col: i32,
    pub letter: String,
}

#[derive(Deserialize)]
pub struct LettersoepSubmission {
    pub tiles: Vec<SubmittedTile>,
    pub started_at_ms: i64,
    pub finished_at_ms: i64,
    pub generator_version: String,
}

pub enum VerifyError {
    DayInFuture,
    VersionMismatch,
    BadTimes,
    IllegalMove,
    UnknownWord(String),
    Generation,
}

impl VerifyError {
    pub fn message(&self) -> String {
        match self {
            VerifyError::DayInFuture => "puzzle day is in the future".into(),
            VerifyError::VersionMismatch => "generator version mismatch".into(),
            VerifyError::BadTimes => "invalid start/finish timestamps".into(),
            VerifyError::IllegalMove => "tiles do not form a legal move".into(),
            VerifyError::UnknownWord(w) => format!("{w} is not in the word list"),
            VerifyError::Generation => "puzzle generation failed".into(),
        }
    }
}

/// Verify a submitted move by regenerating the day's puzzle and scoring the
/// tiles server-side; the normalized payload comes from our own scoring,
/// never from client claims.
pub fn verify(
    lang: Lang,
    day: i64,
    submission: &LettersoepSubmission,
) -> Result<serde_json::Value, VerifyError> {
    if submission.generator_version != lang.version() {
        return Err(VerifyError::VersionMismatch);
    }
    let today = puzzle_number(chrono::Utc::now().date_naive());
    // ±1 day of slack: players finish on their own local calendar.
    if day > today + 1 {
        return Err(VerifyError::DayInFuture);
    }
    let elapsed_ms = submission.finished_at_ms - submission.started_at_ms;
    if elapsed_ms <= 0 || elapsed_ms > 12 * 3600 * 1000 {
        return Err(VerifyError::BadTimes);
    }
    let date = epoch() + chrono::Duration::days(day - 1);
    let puzzle = cached_puzzle(date, lang).map_err(|_| VerifyError::Generation)?;

    // The tiles must come from the day's rack.
    let mut rack_counts = [0u16; 26];
    for &letter in &puzzle.rack {
        rack_counts[(letter as u8 - b'A') as usize] += 1;
    }
    let mut tiles = Vec::new();
    for t in &submission.tiles {
        let [letter] = t.letter.as_bytes() else { return Err(VerifyError::IllegalMove) };
        if !letter.is_ascii_uppercase() {
            return Err(VerifyError::IllegalMove);
        }
        if !(0..BOARD_SIZE).contains(&t.row) || !(0..BOARD_SIZE).contains(&t.col) {
            return Err(VerifyError::IllegalMove);
        }
        let idx = (letter - b'A') as usize;
        if rack_counts[idx] == 0 {
            return Err(VerifyError::IllegalMove);
        }
        rack_counts[idx] -= 1;
        tiles.push(Tile { row: t.row, col: t.col, letter: *letter });
    }

    let placed: Vec<PlacedWord> = puzzle
        .placed
        .iter()
        .map(|p| PlacedWord { row: p.row, col: p.col, dir: p.dir, word: p.word.clone() })
        .collect();
    let board = build_board(&placed);
    let scored = score_placement(lang, &board, &tiles).ok_or(VerifyError::IllegalMove)?;
    for word in &scored.words {
        if !lang.dict().contains(word.word.as_str()) {
            return Err(VerifyError::UnknownWord(word.word.clone()));
        }
    }

    Ok(serde_json::json!({
        "score": scored.score,
        "max_score": puzzle.max_score,
        "at_max": scored.score >= puzzle.max_score,
        "bingo": scored.bingo,
        "words": scored.words.iter().map(|w| w.word.clone()).collect::<Vec<_>>(),
        "time_ms": elapsed_ms,
    }))
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
        let puzzle = daily_puzzle(date, Lang::Nl).unwrap();
        assert_eq!(puzzle.seed, 900493200);
        assert_eq!(puzzle.number, 1);
        let words: Vec<(&str, i32, i32, &str)> = puzzle
            .placed
            .iter()
            .map(|p| (p.word.as_str(), p.row, p.col, p.dir))
            .collect();
        assert_eq!(
            words,
            vec![
                ("NIETSNUT", 3, 7, "down"),
                ("BRANDING", 3, 1, "across"),
                ("GEMAKJE", 0, 3, "down"),
                ("TWISTER", 10, 3, "across"),
                ("AFGUNST", 0, 1, "across"),
                ("REGELDEN", 8, 0, "across"),
            ]
        );
        assert_eq!(puzzle.rack, vec!['A', 'N', 'A', 'S', 'E', 'F', 'M']);
        assert_eq!(
            (puzzle.view.top, puzzle.view.left, puzzle.view.rows, puzzle.view.cols),
            (3, 3, 9, 8)
        );
        assert_eq!(puzzle.max_score, 136);
        assert_eq!(puzzle.move_count, 931);
        assert_eq!(puzzle.valid_words.len(), 459);
        assert_eq!(&puzzle.valid_words[..3], ["AAD", "AAM", "AAN"]);
    }

    /// Manual benchmark: cargo test --release -- --ignored --nocapture bench
    #[test]
    #[ignore]
    fn bench_generation() {
        LazyLock::force(&WORD_INDEX_NL);
        let days = 20u32;
        let start = std::time::Instant::now();
        for i in 0..days {
            let date = epoch() + chrono::Duration::days(i as i64);
            daily_puzzle(date, Lang::Nl).unwrap();
        }
        let elapsed = start.elapsed();
        println!("{days} puzzles in {elapsed:?} ({:?}/puzzle)", elapsed / days);
    }

    /// Same pin for the English game (lettersoup, ENABLE2K, era v1).
    #[test]
    fn english_day_one_is_stable() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let puzzle = daily_puzzle(date, Lang::En).unwrap();
        assert_eq!(puzzle.seed, 1211504969);
        assert_eq!(puzzle.generator_version, "v1");
        let words: Vec<(&str, i32, i32, &str)> = puzzle
            .placed
            .iter()
            .map(|p| (p.word.as_str(), p.row, p.col, p.dir))
            .collect();
        assert_eq!(
            words,
            vec![
                ("MISPRINT", 5, 7, "down"),
                ("REVELLER", 9, 7, "across"),
                ("DIPOLAR", 5, 12, "down"),
                ("GROWLER", 8, 14, "down"),
                ("ASCENTS", 13, 1, "across"),
            ]
        );
        assert_eq!(puzzle.rack, vec!['O', 'T', 'E', 'D', 'U', 'R', 'O']);
        assert_eq!(
            (puzzle.view.top, puzzle.view.left, puzzle.view.rows, puzzle.view.cols),
            (0, 0, 9, 8)
        );
        assert_eq!(puzzle.max_score, 83);
        assert_eq!(puzzle.move_count, 1216);
        assert_eq!(puzzle.valid_words.len(), 548);
        assert_eq!(&puzzle.valid_words[..3], ["AD", "ADO", "AE"]);
    }

    #[test]
    fn seed_derivation_is_stable() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 2).unwrap();
        assert_eq!(seed_for_date(date, Lang::Nl), 950826057);
    }
}
