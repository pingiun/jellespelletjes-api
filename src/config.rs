#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub listen_addr: String,
    /// Public URL of the hub site, where magic links land (e.g. https://jellespelletjes.nl).
    pub hub_url: String,
    /// Origins allowed for CORS and as SSO targets.
    pub allowed_origins: Vec<String>,
    /// Resend API key. When absent, emails are logged instead of sent (dev mode).
    pub resend_api_key: Option<String>,
    /// From address for auth email.
    pub email_from: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://app.db?mode=rwc".to_string());
        let listen_addr =
            std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
        let hub_url = std::env::var("PUBLIC_HUB_URL")
            .unwrap_or_else(|_| "https://jellespelletjes.nl".to_string());
        let allowed_origins = std::env::var("ALLOWED_ORIGINS")
            .unwrap_or_else(|_| {
                "https://jellespelletjes.nl,https://sudokudo.nl,https://woordle.nl".to_string()
            })
            .split(',')
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        anyhow::ensure!(!allowed_origins.is_empty(), "ALLOWED_ORIGINS must not be empty");
        let resend_api_key = std::env::var("RESEND_API_KEY").ok().filter(|s| !s.is_empty());
        let email_from = std::env::var("EMAIL_FROM")
            .unwrap_or_else(|_| "Jellespelletjes <login@jellespelletjes.nl>".to_string());

        Ok(Self {
            database_url,
            listen_addr,
            hub_url: hub_url.trim_end_matches('/').to_string(),
            allowed_origins,
            resend_api_key,
            email_from,
        })
    }
}
