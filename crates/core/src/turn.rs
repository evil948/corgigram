use anyhow::{Context, Result};
use corgigram_transport::TurnCredentials;
use serde::Deserialize;

#[derive(Deserialize)]
struct ElixirTurnResponse {
    password: String,
    uris: Vec<String>,
    username: String,
}

/// Public TURN with ephemeral credentials (works when metered openrelay is blocked).
pub async fn fetch_elixir_webrtc_turn(client_label: &str) -> Result<TurnCredentials> {
    let url = format!(
        "https://turn.elixir-webrtc.org/?service=turn&username={client_label}"
    );
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .context("build turn fetch client")?
        .post(&url)
        .send()
        .await
        .context("fetch elixir-webrtc turn credentials")?
        .error_for_status()
        .context("elixir-webrtc turn credentials status")?;
    let body: ElixirTurnResponse = resp
        .json()
        .await
        .context("parse elixir-webrtc turn credentials")?;

    let mut urls = body.uris.clone();
    for uri in &body.uris {
        if uri.contains("transport=udp") {
            if let Some(host_port) = uri.strip_prefix("turn:").and_then(|s| s.split('?').next()) {
                urls.push(format!("turn:{host_port}?transport=tcp"));
            }
        }
    }

    Ok(TurnCredentials {
        urls,
        username: body.username,
        credential: body.password,
    })
}
