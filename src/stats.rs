//! Canonical per-game statistics computed from verified results.
//! Streaks are based on consecutive `day` values; unlike woordle's local
//! stats, a skipped day breaks the streak here.

use serde_json::json;

pub struct ResultRow {
    pub day: i64,
    pub payload: serde_json::Value,
}

/// Pre-account local stats carried over from the one-time import
/// (woordle and sudokudo both store these camelCase).
#[derive(Default, Clone, Copy)]
pub struct Baseline {
    pub current_streak: i64,
    pub max_streak: i64,
    pub games_played: i64,
    pub games_won: i64,
    /// Woordle guess distribution: 1..6 guesses + fail.
    pub guesses: [i64; 7],
    /// Sudokudo solve-time distribution buckets.
    pub buckets: [i64; 6],
}

pub fn baseline_from_import(payload: &serde_json::Value) -> Baseline {
    let get = |key: &str| payload.get(key).and_then(|v| v.as_i64()).unwrap_or(0).max(0);
    let mut guesses = [0i64; 7];
    if let Some(g) = payload.get("guesses") {
        for (i, key) in ["1", "2", "3", "4", "5", "6", "fail"].iter().enumerate() {
            guesses[i] = g.get(key).and_then(|v| v.as_i64()).unwrap_or(0).max(0);
        }
    }
    let mut buckets = [0i64; 6];
    if let Some(arr) = payload.get("buckets").and_then(|v| v.as_array()) {
        for (i, v) in arr.iter().take(6).enumerate() {
            buckets[i] = v.as_i64().unwrap_or(0).max(0);
        }
    }
    Baseline {
        current_streak: get("currentStreak"),
        max_streak: get("maxStreak"),
        games_played: get("gamesPlayed"),
        games_won: get("gamesWon"),
        guesses,
        buckets,
    }
}

/// Compute stats for one game from its result rows (any order).
pub fn game_stats(
    game: &str,
    rows: &[ResultRow],
    today_day: Option<i64>,
    baseline: Option<Baseline>,
) -> serde_json::Value {
    let mut days: Vec<(i64, bool)> = rows
        .iter()
        .map(|r| {
            let won = if game.starts_with("sudokudo") {
                true // only verified wins are stored for sudoku
            } else {
                r.payload.get("won").and_then(|v| v.as_bool()).unwrap_or(false)
            };
            (r.day, won)
        })
        .collect();
    days.sort_unstable();

    let played = days.len() as i64;
    let won = days.iter().filter(|(_, w)| *w).count() as i64;

    // Current streak: run of consecutive won days ending at the latest result,
    // which must reach today (or yesterday, when today isn't played yet).
    let mut current_streak = 0i64;
    if let Some(&(last_day, _)) = days.last() {
        let reaches_present = today_day.is_none_or(|t| last_day >= t - 1);
        if reaches_present {
            let mut expected = last_day;
            for &(day, day_won) in days.iter().rev() {
                if day == expected && day_won {
                    current_streak += 1;
                    expected -= 1;
                } else {
                    break;
                }
            }
        }
    }

    // The imported pre-account streak has no day anchor, so it chains into
    // the verified streak only while the verified history is one unbroken
    // winning run from the first submitted game — any verified loss or gap
    // legitimately broke the streak, and the baseline stops applying. With
    // no verified games yet the imported streak stands as-is (local woordle
    // semantics: absence does not break a streak).
    let base = baseline.unwrap_or_default();
    if days.is_empty() || current_streak == played {
        current_streak += base.current_streak;
    }

    let mut max_streak = 0i64;
    let mut run = 0i64;
    let mut prev: Option<i64> = None;
    for &(day, day_won) in &days {
        if day_won {
            run = match prev {
                Some(p) if day == p + 1 => run + 1,
                _ => 1,
            };
            max_streak = max_streak.max(run);
            prev = Some(day);
        } else {
            run = 0;
            prev = None;
        }
    }
    max_streak = max_streak.max(current_streak).max(base.max_streak);

    // Distribution: sudokudo buckets solve times (minutes), woordle buckets guess counts.
    let distribution = if game.starts_with("sudokudo") {
        let bounds_min = [3.0, 5.0, 10.0, 15.0, 30.0];
        let mut buckets = base.buckets;
        for r in rows {
            if let Some(ms) = r.payload.get("elapsed_ms").and_then(|v| v.as_i64()) {
                let minutes = ms as f64 / 60000.0;
                let idx = bounds_min.iter().position(|b| minutes < *b).unwrap_or(5);
                buckets[idx] += 1;
            }
        }
        json!({ "type": "time", "labels": ["<3","3-5","5-10","10-15","15-30",">30"], "counts": buckets })
    } else {
        let mut buckets = base.guesses; // 1..6 guesses + fail
        for r in rows {
            let won = r.payload.get("won").and_then(|v| v.as_bool()).unwrap_or(false);
            let guesses = r
                .payload
                .get("guesses")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            if won && (1..=6).contains(&guesses) {
                buckets[guesses - 1] += 1;
            } else {
                buckets[6] += 1;
            }
        }
        json!({ "type": "guesses", "labels": ["1","2","3","4","5","6","X"], "counts": buckets })
    };

    json!({
        "played": played + base.games_played,
        "won": won + base.games_won,
        "current_streak": current_streak,
        "max_streak": max_streak,
        "distribution": distribution,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(day: i64, won: bool) -> ResultRow {
        ResultRow { day, payload: json!({ "won": won, "guesses": ["a","b","c"] }) }
    }

    #[test]
    fn streaks_break_on_gaps() {
        let rows = vec![row(10, true), row(11, true), row(13, true)];
        let s = game_stats("woordle", &rows, Some(13), None);
        assert_eq!(s["current_streak"], json!(1));
        assert_eq!(s["max_streak"], json!(2));
    }

    #[test]
    fn current_streak_survives_unplayed_today() {
        let rows = vec![row(11, true), row(12, true)];
        assert_eq!(game_stats("woordle", &rows, Some(13), None)["current_streak"], json!(2));
        assert_eq!(game_stats("woordle", &rows, Some(14), None)["current_streak"], json!(0));
    }

    #[test]
    fn loss_breaks_streak() {
        let rows = vec![row(10, true), row(11, false), row(12, true)];
        let s = game_stats("woordle", &rows, Some(12), None);
        assert_eq!(s["current_streak"], json!(1));
        assert_eq!(s["max_streak"], json!(1));
        assert_eq!(s["won"], json!(2));
        assert_eq!(s["played"], json!(3));
    }

    #[test]
    fn baseline_chains_into_unbroken_verified_run() {
        let base = Some(Baseline { current_streak: 40, max_streak: 50, ..Default::default() });
        let rows = vec![row(11, true), row(12, true)];
        let s = game_stats("woordle", &rows, Some(12), base);
        assert_eq!(s["current_streak"], json!(42));
        assert_eq!(s["max_streak"], json!(50));
    }

    #[test]
    fn baseline_stands_alone_without_verified_results() {
        let s = game_stats("woordle", &[], Some(12), Some(Baseline { current_streak: 7, max_streak: 9, ..Default::default() }));
        assert_eq!(s["current_streak"], json!(7));
        assert_eq!(s["max_streak"], json!(9));
    }

    #[test]
    fn baseline_dropped_after_verified_loss_or_gap() {
        let base = Some(Baseline { current_streak: 40, max_streak: 41, ..Default::default() });
        // A loss in verified history breaks the chain.
        let rows = vec![row(10, false), row(11, true), row(12, true)];
        let s = game_stats("woordle", &rows, Some(12), base);
        assert_eq!(s["current_streak"], json!(2));
        assert_eq!(s["max_streak"], json!(41));
        // A skipped day does too.
        let rows = vec![row(9, true), row(11, true), row(12, true)];
        let s = game_stats("woordle", &rows, Some(12), base);
        assert_eq!(s["current_streak"], json!(2));
        // An expired verified streak (not reaching the present) drops both.
        let rows = vec![row(5, true)];
        let s = game_stats("woordle", &rows, Some(12), base);
        assert_eq!(s["current_streak"], json!(0));
    }

    #[test]
    fn baseline_counts_and_distribution_merge() {
        let base = baseline_from_import(&json!({
            "currentStreak": 2, "maxStreak": 5, "gamesPlayed": 10, "gamesWon": 8,
            "guesses": {"1": 1, "2": 0, "3": 4, "4": 2, "5": 1, "6": 0, "fail": 2}
        }));
        // One verified 3-guess win on top of the imported history.
        let rows = vec![row(12, true)];
        let s = game_stats("woordle", &rows, Some(12), Some(base));
        assert_eq!(s["played"], json!(11));
        assert_eq!(s["won"], json!(9)); // win% = 9/11 covers the whole history
        assert_eq!(s["distribution"]["counts"], json!([1, 0, 5, 2, 1, 0, 2]));
        assert_eq!(s["current_streak"], json!(3));
    }

    #[test]
    fn baseline_parses_camel_case_import() {
        let b = baseline_from_import(&json!({"currentStreak": 3, "maxStreak": 8, "gamesPlayed": 10}));
        assert_eq!(b.current_streak, 3);
        assert_eq!(b.max_streak, 8);
        let b = baseline_from_import(&json!({"currentStreak": -2}));
        assert_eq!(b.current_streak, 0);
    }

    #[test]
    fn sudoku_time_distribution() {
        let rows = vec![ResultRow { day: 1, payload: json!({"elapsed_ms": 240000}) }];
        let s = game_stats("sudokudo", &rows, Some(1), None);
        assert_eq!(s["distribution"]["counts"], json!([0, 1, 0, 0, 0, 0]));
    }
}
