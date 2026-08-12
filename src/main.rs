use clap::{Parser, Subcommand};
use jellespelletjes_api::{auth, config, db, router, seed, AppState, SharedState};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "jellespelletjes-api")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the HTTP server.
    Serve,
    /// Read sudoku puzzle rows as JSONL on stdin and upsert them.
    SeedSudoku,
    /// Print the lettersoep daily puzzle for a date (default today, UTC) as JSON.
    GenerateLettersoep { date: Option<String> },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let cli = Cli::parse();
    let config = config::Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;

    match cli.command {
        Command::Serve => {
            let listen = config.listen_addr.clone();
            let state: SharedState = Arc::new(AppState {
                pool,
                config,
                http: reqwest::Client::new(),
                rate_limiter: auth::RateLimiter::default(),
            });
            let app = router(state);
            let listener = tokio::net::TcpListener::bind(&listen).await?;
            tracing::info!("listening on {listen}");
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
        }
        Command::SeedSudoku => {
            seed::seed_sudoku_from_stdin(&pool).await?;
        }
        Command::GenerateLettersoep { date } => {
            let date = match date {
                Some(d) => d.parse()?,
                None => chrono::Utc::now().date_naive(),
            };
            let puzzle = jellespelletjes_api::games::lettersoep::daily_puzzle(date)?;
            println!("{}", serde_json::to_string_pretty(&puzzle)?);
        }
    }
    Ok(())
}
