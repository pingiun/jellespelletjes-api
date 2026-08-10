//! Verification for the four woordle.nl variants.
//!
//! The daily word is fully deterministic: `answers[days_since_start % len]`,
//! where days_since_start is a wall-clock calendar-day difference computed in
//! the *player's local timezone*. The server therefore accepts a submitted day
//! within ±1 of its own UTC-derived day. `day` is stored pre-modulo so results
//! stay unambiguous after a list wraps around (the Dutch-5 list already has).

use chrono::{NaiveDate, Utc};
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::LazyLock;

pub struct Variant {
    pub game: &'static str,
    pub start_date: NaiveDate,
    pub answers: Vec<&'static str>,
    pub allowed: HashSet<&'static str>,
    /// Offset added to `day` for the puzzle number shown to players.
    pub display_offset: i64,
}

macro_rules! words {
    ($file:literal) => {
        include_str!(concat!("../../data/woordle/", $file))
            .trim()
            .split('\n')
            .collect()
    };
}

pub static VARIANTS: LazyLock<Vec<Variant>> = LazyLock::new(|| {
    vec![
        Variant {
            game: "woordle",
            start_date: NaiveDate::from_ymd_opt(2022, 1, 7).unwrap(),
            answers: words!("puzzle-words"),
            allowed: words!("all-words"),
            display_offset: 202,
        },
        Variant {
            game: "woordle6",
            start_date: NaiveDate::from_ymd_opt(2022, 1, 10).unwrap(),
            answers: words!("puzzle-words6"),
            allowed: words!("all-words6"),
            display_offset: 1,
        },
        Variant {
            game: "wordle",
            start_date: NaiveDate::from_ymd_opt(2021, 6, 19).unwrap(),
            answers: words!("puzzle-words-en"),
            allowed: words!("all-words-en"),
            display_offset: 0,
        },
        Variant {
            game: "wordle6",
            start_date: NaiveDate::from_ymd_opt(2022, 1, 11).unwrap(),
            answers: words!("puzzle-words6-en"),
            allowed: words!("all-words6-en"),
            display_offset: 1,
        },
    ]
});

pub fn variant(game: &str) -> Option<&'static Variant> {
    VARIANTS.iter().find(|v| v.game == game)
}

/// The server's own day index for a variant (UTC calendar date).
pub fn utc_day(v: &Variant) -> i64 {
    (Utc::now().date_naive() - v.start_date).num_days()
}

pub fn solution_for_day(v: &Variant, day: i64) -> &'static str {
    let len = v.answers.len() as i64;
    v.answers[(day.rem_euclid(len)) as usize]
}

#[derive(Deserialize)]
pub struct WoordleSubmission {
    pub guesses: Vec<String>,
    pub won: bool,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Mark {
    Correct,
    Present,
    Absent,
}

impl Mark {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mark::Correct => "correct",
            Mark::Present => "present",
            Mark::Absent => "absent",
        }
    }
}

/// Two-pass Wordle evaluation with duplicate-letter accounting, ported from
/// woordle's `colorLetters_` (Main.elm): pass 1 marks exact matches and
/// removes them from the remaining-letters multiset; pass 2 marks `present`
/// only while an instance of the letter remains unclaimed.
pub fn evaluate(guess: &str, solution: &str) -> Vec<Mark> {
    let guess: Vec<char> = guess.chars().collect();
    let solution: Vec<char> = solution.chars().collect();
    let mut marks = vec![Mark::Absent; guess.len()];
    let mut left: Vec<char> = Vec::new();
    for (i, &s) in solution.iter().enumerate() {
        if guess.get(i) == Some(&s) {
            marks[i] = Mark::Correct;
        } else {
            left.push(s);
        }
    }
    for (i, &g) in guess.iter().enumerate() {
        if marks[i] == Mark::Correct {
            continue;
        }
        if let Some(pos) = left.iter().position(|&c| c == g) {
            marks[i] = Mark::Present;
            left.remove(pos);
        }
    }
    marks
}

pub enum VerifyError {
    UnknownGame,
    DayOutOfRange { server_day: i64 },
    BadGuessCount,
    UnknownWord(String),
    OutcomeMismatch,
}

impl VerifyError {
    pub fn message(&self) -> String {
        match self {
            VerifyError::UnknownGame => "unknown game".into(),
            VerifyError::DayOutOfRange { server_day } => {
                format!("day out of range (server day is {server_day}, ±1 accepted)")
            }
            VerifyError::BadGuessCount => "must have 1-6 guesses; a loss requires exactly 6".into(),
            VerifyError::UnknownWord(w) => format!("guess not in the word list: {w}"),
            VerifyError::OutcomeMismatch => "reported outcome does not match the guesses".into(),
        }
    }
}

/// Verify a submission; on success returns the normalized payload to store
/// (guesses + server-computed evaluations + solution day info).
pub fn verify(
    game: &str,
    day: i64,
    submission: &WoordleSubmission,
) -> Result<serde_json::Value, VerifyError> {
    let v = variant(game).ok_or(VerifyError::UnknownGame)?;
    let server_day = utc_day(v);
    if (day - server_day).abs() > 1 || day < 0 {
        return Err(VerifyError::DayOutOfRange { server_day });
    }
    let count = submission.guesses.len();
    if count == 0 || count > 6 || (!submission.won && count != 6) {
        return Err(VerifyError::BadGuessCount);
    }
    let solution = solution_for_day(v, day);
    let word_len = solution.chars().count();
    for guess in &submission.guesses {
        let normalized = guess.to_lowercase();
        if normalized.chars().count() != word_len || !v.allowed.contains(normalized.as_str()) {
            return Err(VerifyError::UnknownWord(guess.clone()));
        }
    }
    let last = submission.guesses.last().unwrap().to_lowercase();
    let solved = last == solution;
    if solved != submission.won {
        return Err(VerifyError::OutcomeMismatch);
    }
    // Any non-final guess equal to the solution would mean the game continued
    // after a win — reject as inconsistent.
    if submission
        .guesses
        .iter()
        .take(count - 1)
        .any(|g| g.to_lowercase() == solution)
    {
        return Err(VerifyError::OutcomeMismatch);
    }
    let evaluations: Vec<Vec<&str>> = submission
        .guesses
        .iter()
        .map(|g| {
            evaluate(&g.to_lowercase(), solution)
                .iter()
                .map(Mark::as_str)
                .collect()
        })
        .collect();
    Ok(serde_json::json!({
        "guesses": submission.guesses.iter().map(|g| g.to_lowercase()).collect::<Vec<_>>(),
        "evaluations": evaluations,
        "won": submission.won,
        "display_number": day + v.display_offset,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(s: &str) -> Vec<Mark> {
        s.chars()
            .map(|c| match c {
                'c' => Mark::Correct,
                'p' => Mark::Present,
                _ => Mark::Absent,
            })
            .collect()
    }

    #[test]
    fn evaluation_basic() {
        assert_eq!(evaluate("water", "water"), marks("ccccc"));
        // n-a-d-i-r vs w-a-t-e-r: a and r exact, rest absent
        assert_eq!(evaluate("nadir", "water"), marks(".c..c"));
        // w-a-t-e-r vs k-a-d-e-r: a, e, r exact
        assert_eq!(evaluate("water", "kader"), marks(".c.cc"));
    }

    #[test]
    fn evaluation_duplicates() {
        // solution "world" has one 'l': the exact match at pos 3 claims it,
        // so the other two l's in "lolly" must be absent.
        assert_eq!(evaluate("lolly", "world"), marks(".c.c."));
        // guess "geese" vs solution "elees": one exact e, remaining e's/s soak
        // up the multiset, g absent.
        assert_eq!(evaluate("geese", "elees"), marks(".pcpp"));
        // classic duplicate case: "sassy" vs "grass" — first s present,
        // a present, second s exact, third s exhausted (absent).
        assert_eq!(evaluate("sassy", "grass"), marks("pp.c."));
    }

    #[test]
    fn variants_load() {
        for v in VARIANTS.iter() {
            assert!(!v.answers.is_empty(), "{} answers empty", v.game);
            assert!(v.answers.len() <= v.allowed.len(), "{} lists inconsistent", v.game);
            for w in &v.answers {
                assert!(v.allowed.contains(w), "{}: answer {w} not in allowed list", v.game);
            }
        }
        assert_eq!(variant("woordle").unwrap().answers.len(), 844);
        assert_eq!(variant("woordle6").unwrap().answers.len(), 2594);
        assert_eq!(variant("wordle").unwrap().answers.len(), 2315);
        assert_eq!(variant("wordle6").unwrap().answers.len(), 5821);
    }

    #[test]
    fn wraparound_day_uses_modulo() {
        let v = variant("woordle").unwrap();
        assert_eq!(solution_for_day(v, 0), solution_for_day(v, 844));
    }

    #[test]
    fn verify_rejects_wrong_day() {
        let v = variant("woordle").unwrap();
        let day = utc_day(v);
        let sub = WoordleSubmission { guesses: vec![solution_for_day(v, day - 5).into()], won: true };
        assert!(matches!(
            verify("woordle", day - 5, &sub),
            Err(VerifyError::DayOutOfRange { .. })
        ));
    }

    #[test]
    fn verify_accepts_valid_win() {
        let v = variant("woordle").unwrap();
        let day = utc_day(v);
        let solution = solution_for_day(v, day);
        let sub = WoordleSubmission { guesses: vec![solution.to_string()], won: true };
        let payload = verify("woordle", day, &sub).ok().unwrap();
        assert_eq!(payload["evaluations"][0], serde_json::json!(["correct","correct","correct","correct","correct"]));
        assert_eq!(payload["display_number"], serde_json::json!(day + 202));
    }

    #[test]
    fn verify_rejects_outcome_mismatch() {
        let v = variant("woordle").unwrap();
        let day = utc_day(v);
        let solution = solution_for_day(v, day);
        // A valid word that is not today's solution, claimed as a win.
        let other = v.answers.iter().find(|w| **w != solution).unwrap();
        let sub = WoordleSubmission { guesses: vec![other.to_string()], won: true };
        assert!(matches!(verify("woordle", day, &sub), Err(VerifyError::OutcomeMismatch)));
    }

    #[test]
    fn verify_rejects_short_loss() {
        let v = variant("woordle").unwrap();
        let day = utc_day(v);
        let solution = solution_for_day(v, day);
        let other = v.answers.iter().find(|w| **w != solution).unwrap();
        // A loss must have exactly 6 guesses.
        let sub = WoordleSubmission { guesses: vec![other.to_string()], won: false };
        assert!(matches!(verify("woordle", day, &sub), Err(VerifyError::BadGuessCount)));
    }
}
