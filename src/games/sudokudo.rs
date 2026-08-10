//! Verification for sudokudo results against the pre-seeded puzzles table.
//! The server never generates sudokus; the sudokudo TypeScript engine is the
//! single source of truth and seeds rows via the `seed-sudoku` subcommand.

use serde::Deserialize;

#[derive(Deserialize)]
pub struct SudokuSubmission {
    /// 81 digit characters, the player's completed grid.
    pub solution: String,
    pub started_at_ms: i64,
    pub finished_at_ms: i64,
    pub generator_version: String,
}

pub struct SeededPuzzle {
    pub generator_version: String,
    pub difficulty: String,
    pub solution: String,
}

pub enum VerifyError {
    NotSeeded,
    DayInFuture,
    VersionMismatch,
    WrongSolution,
    BadTimes,
}

impl VerifyError {
    pub fn message(&self) -> String {
        match self {
            VerifyError::NotSeeded => "puzzle not seeded on the server yet".into(),
            VerifyError::DayInFuture => "puzzle day is in the future".into(),
            VerifyError::VersionMismatch => "generator version mismatch".into(),
            VerifyError::WrongSolution => "solution does not match the daily puzzle".into(),
            VerifyError::BadTimes => "invalid start/finish timestamps".into(),
        }
    }
}

/// Verify a submission against the seeded puzzle; on success returns the
/// normalized payload to store.
pub fn verify(
    seeded: Option<&SeededPuzzle>,
    today_puzzle_number: i64,
    day: i64,
    submission: &SudokuSubmission,
) -> Result<serde_json::Value, VerifyError> {
    let seeded = seeded.ok_or(VerifyError::NotSeeded)?;
    if day > today_puzzle_number {
        return Err(VerifyError::DayInFuture);
    }
    if submission.generator_version != seeded.generator_version {
        return Err(VerifyError::VersionMismatch);
    }
    if submission.solution != seeded.solution {
        return Err(VerifyError::WrongSolution);
    }
    let elapsed_ms = submission.finished_at_ms - submission.started_at_ms;
    if elapsed_ms <= 0 || submission.started_at_ms <= 0 {
        return Err(VerifyError::BadTimes);
    }
    // Implausibly fast solves are stored but flagged; ranking policy can
    // decide what to do with them later.
    let suspect = elapsed_ms < 5_000;
    Ok(serde_json::json!({
        "elapsed_ms": elapsed_ms,
        "difficulty": seeded.difficulty,
        "generator_version": seeded.generator_version,
        "suspect": suspect,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> SeededPuzzle {
        SeededPuzzle {
            generator_version: "v2".into(),
            difficulty: "easy".into(),
            solution: "1".repeat(81),
        }
    }

    fn submission() -> SudokuSubmission {
        SudokuSubmission {
            solution: "1".repeat(81),
            started_at_ms: 1_000_000,
            finished_at_ms: 1_300_000,
            generator_version: "v2".into(),
        }
    }

    #[test]
    fn accepts_correct_solution() {
        let payload = verify(Some(&seeded()), 10, 10, &submission()).ok().unwrap();
        assert_eq!(payload["elapsed_ms"], serde_json::json!(300_000));
        assert_eq!(payload["suspect"], serde_json::json!(false));
    }

    #[test]
    fn rejects_wrong_solution() {
        let mut sub = submission();
        sub.solution = "2".repeat(81);
        assert!(matches!(
            verify(Some(&seeded()), 10, 10, &sub),
            Err(VerifyError::WrongSolution)
        ));
    }

    #[test]
    fn rejects_future_day() {
        assert!(matches!(
            verify(Some(&seeded()), 9, 10, &submission()),
            Err(VerifyError::DayInFuture)
        ));
    }

    #[test]
    fn flags_suspect_times() {
        let mut sub = submission();
        sub.finished_at_ms = sub.started_at_ms + 800;
        let payload = verify(Some(&seeded()), 10, 10, &sub).ok().unwrap();
        assert_eq!(payload["suspect"], serde_json::json!(true));
    }
}
