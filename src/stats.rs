//! Canonical per-game statistics computed from verified results.
//! Streaks are based on consecutive `day` values; unlike woordle's local
//! stats, a skipped day breaks the streak here.

use serde_json::json;

pub struct ResultRow {
    pub day: i64,
    pub payload: serde_json::Value,
}

/// Compute stats for one game from its result rows (any order).
pub fn game_stats(game: &str, rows: &[ResultRow], today_day: Option<i64>) -> serde_json::Value {
    let mut days: Vec<(i64, bool)> = rows
        .iter()
        .map(|r| {
            let won = match game {
                "sudokudo" => true, // only verified wins are stored for sudoku
                _ => r.payload.get("won").and_then(|v| v.as_bool()).unwrap_or(false),
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

    // Distribution: sudokudo buckets solve times (minutes), woordle buckets guess counts.
    let distribution = if game == "sudokudo" {
        let bounds_min = [3.0, 5.0, 10.0, 15.0, 30.0];
        let mut buckets = [0i64; 6];
        for r in rows {
            if let Some(ms) = r.payload.get("elapsed_ms").and_then(|v| v.as_i64()) {
                let minutes = ms as f64 / 60000.0;
                let idx = bounds_min.iter().position(|b| minutes < *b).unwrap_or(5);
                buckets[idx] += 1;
            }
        }
        json!({ "type": "time", "labels": ["<3","3-5","5-10","10-15","15-30",">30"], "counts": buckets })
    } else {
        let mut buckets = [0i64; 7]; // 1..6 guesses + fail
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
        "played": played,
        "won": won,
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
        let s = game_stats("woordle", &rows, Some(13));
        assert_eq!(s["current_streak"], json!(1));
        assert_eq!(s["max_streak"], json!(2));
    }

    #[test]
    fn current_streak_survives_unplayed_today() {
        let rows = vec![row(11, true), row(12, true)];
        assert_eq!(game_stats("woordle", &rows, Some(13))["current_streak"], json!(2));
        assert_eq!(game_stats("woordle", &rows, Some(14))["current_streak"], json!(0));
    }

    #[test]
    fn loss_breaks_streak() {
        let rows = vec![row(10, true), row(11, false), row(12, true)];
        let s = game_stats("woordle", &rows, Some(12));
        assert_eq!(s["current_streak"], json!(1));
        assert_eq!(s["max_streak"], json!(1));
        assert_eq!(s["won"], json!(2));
        assert_eq!(s["played"], json!(3));
    }

    #[test]
    fn sudoku_time_distribution() {
        let rows = vec![ResultRow { day: 1, payload: json!({"elapsed_ms": 240000}) }];
        let s = game_stats("sudokudo", &rows, Some(1));
        assert_eq!(s["distribution"]["counts"], json!([0, 1, 0, 0, 0, 0]));
    }
}
