use anyhow::{bail, Context, Result};
use clap::Parser;
use reqwest::Client;
use serde::Serialize;

#[derive(Parser, Debug)]
#[command(about = "Send a faucet mint request to the zfruit server", version)]
struct Args {
    /// Base URL of the server, e.g. http://localhost:9002
    #[arg(long, default_value = "http://localhost:9002")]
    server_url: String,

    /// Name to use for the faucet mint request
    #[arg(long)]
    name: String,

    /// Optional mint amount to request
    #[arg(long)]
    amount: Option<u64>,
}

#[derive(Serialize)]
struct FaucetRequest<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    amount: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let client = Client::new();
    let endpoint = build_endpoint(&args.server_url);

    let payload = FaucetRequest {
        name: &args.name,
        amount: args.amount,
    };

    let response = client
        .post(&endpoint)
        .json(&payload)
        .send()
        .await
        .with_context(|| format!("sending request to {endpoint}"))?;

    let status = response.status();
    if status.is_success() {
        let body: serde_json::Value = response
            .json()
            .await
            .context("reading faucet response body")?;
        println!(
            "{}",
            serde_json::to_string_pretty(&body).context("formatting faucet response")?
        );
    } else {
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read error body>".to_string());
        bail!("request failed ({status}): {text}");
    }

    Ok(())
}

fn build_endpoint(server_url: &str) -> String {
    let url = if server_url.contains("://") {
        server_url.to_string()
    } else {
        format!("http://{server_url}")
    };

    let trimmed = url.trim_end_matches('/');
    if trimmed.ends_with("/api/faucet") {
        trimmed.to_string()
    } else if trimmed.ends_with("/api") {
        format!("{trimmed}/faucet")
    } else {
        format!("{trimmed}/api/faucet")
    }
}
