pub mod lettersoep;
pub mod sudokudo;
pub mod woordle;

/// Games known to the API. `woordle*` are the four woordle.nl variants.
pub const GAMES: &[&str] =
    &["lettersoep", "sudokudo", "sudokudo-expert", "woordle", "woordle6", "wordle", "wordle6"];

pub fn is_known_game(game: &str) -> bool {
    GAMES.contains(&game)
}
