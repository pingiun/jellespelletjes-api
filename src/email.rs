use crate::AppState;

/// Send the magic-link email via Resend, or log the link in dev mode
/// (no RESEND_API_KEY configured).
pub async fn send_magic_link(state: &AppState, to: &str, link: &str) -> anyhow::Result<()> {
    let Some(api_key) = &state.config.resend_api_key else {
        tracing::info!("DEV MODE magic link for {to}: {link}");
        return Ok(());
    };
    let body = serde_json::json!({
        "from": state.config.email_from,
        "to": [to],
        "subject": "Inloggen bij Jellespelletjes",
        "text": format!(
            "Klik op deze link om in te loggen bij Jellespelletjes:\n\n{link}\n\n\
             De link is 15 minuten geldig. Niet zelf aangevraagd? Dan kun je deze mail negeren."
        ),
    });
    let response = state
        .http
        .post("https://api.resend.com/emails")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;
    anyhow::ensure!(
        response.status().is_success(),
        "resend returned {}: {}",
        response.status(),
        response.text().await.unwrap_or_default()
    );
    Ok(())
}
