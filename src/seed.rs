//! `seed-sudoku` subcommand: read JSONL puzzle rows on stdin and insert them.
//! Existing rows are never modified — a row whose content differs from what is
//! already stored is an error, which guards against silent GENERATOR_VERSION
//! or algorithm drift between the TypeScript engine and previously-seeded data.

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Deserialize)]
struct SeedRow {
    puzzle_number: i64,
    date: String,
    generator_version: String,
    difficulty: String,
    givens: String,
    solution: String,
}

pub async fn seed_sudoku_from_stdin(pool: &sqlx::SqlitePool) -> anyhow::Result<()> {
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let (mut inserted, mut unchanged) = (0u32, 0u32);
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let row: SeedRow = serde_json::from_str(line)?;
        anyhow::ensure!(row.givens.len() == 81, "puzzle {}: givens must be 81 chars", row.puzzle_number);
        anyhow::ensure!(row.solution.len() == 81, "puzzle {}: solution must be 81 chars", row.puzzle_number);

        let existing: Option<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT date, generator_version, difficulty, givens, solution
             FROM sudoku_puzzles WHERE puzzle_number = ?",
        )
        .bind(row.puzzle_number)
        .fetch_optional(pool)
        .await?;

        match existing {
            Some((date, version, difficulty, givens, solution)) => {
                anyhow::ensure!(
                    date == row.date
                        && version == row.generator_version
                        && difficulty == row.difficulty
                        && givens == row.givens
                        && solution == row.solution,
                    "puzzle {} already seeded with DIFFERENT content — refusing to overwrite. \
                     If this is an intentional new generator era, wipe and reseed explicitly.",
                    row.puzzle_number
                );
                unchanged += 1;
            }
            None => {
                sqlx::query(
                    "INSERT INTO sudoku_puzzles
                     (puzzle_number, date, generator_version, difficulty, givens, solution)
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(row.puzzle_number)
                .bind(&row.date)
                .bind(&row.generator_version)
                .bind(&row.difficulty)
                .bind(&row.givens)
                .bind(&row.solution)
                .execute(pool)
                .await?;
                inserted += 1;
            }
        }
    }
    println!("seeded {inserted} new puzzles ({unchanged} already present, unchanged)");
    Ok(())
}
