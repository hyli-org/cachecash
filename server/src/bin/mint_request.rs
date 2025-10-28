use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use k256::{
    elliptic_curve::sec1::ToEncodedPoint,
    sha2::{Digest, Sha256},
    SecretKey,
};
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

    /// Public key (hex) to use for the faucet mint request. If omitted, one is derived from the name.
    #[arg(long)]
    pubkey_hex: Option<String>,

    /// Optional mint amount to request
    #[arg(long)]
    amount: Option<u64>,
}

#[derive(Serialize)]
struct FaucetRequest<'a> {
    name: &'a str,
    #[serde(rename = "pubkey_hex")]
    pubkey_hex: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    amount: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let client = Client::new();
    let endpoint = build_endpoint(&args.server_url);

    let derived_pubkey = match &args.pubkey_hex {
        Some(explicit) => sanitize_pubkey_hex(explicit)?,
        None => derive_pubkey_hex(&args.name)?,
    };

    let payload = FaucetRequest {
        name: &args.name,
        pubkey_hex: &derived_pubkey,
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

fn sanitize_pubkey_hex(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("pubkey_hex must not be empty");
    }
    let normalized = trimmed.trim_start_matches("0x");
    if normalized.len() != 64 {
        bail!("pubkey_hex must be exactly 64 hex characters");
    }
    hex::decode(normalized).map_err(|err| anyhow!("invalid pubkey_hex: {err}"))?;
    Ok(normalized.to_ascii_lowercase())
}

fn derive_pubkey_hex(name: &str) -> Result<String> {
    let normalized = name.trim().to_lowercase();
    if normalized.is_empty() {
        bail!("name must not be empty");
    }

    let mut counter: u32 = 0;
    loop {
        let mut hasher = Sha256::new();
        hasher.update(normalized.as_bytes());
        hasher.update(counter.to_be_bytes());
        let digest = hasher.finalize();
        let mut private_key_bytes = [0u8; 32];
        private_key_bytes.copy_from_slice(&digest);

        match SecretKey::from_slice(&private_key_bytes) {
            Ok(secret_key) => {
                let encoded = secret_key.public_key().to_encoded_point(false);
                let x_bytes = encoded
                    .x()
                    .ok_or_else(|| anyhow!("derived public key is missing x coordinate"))?;
                let mut x_array = [0u8; 32];
                x_array.copy_from_slice(x_bytes);
                return Ok(hex::encode(x_array));
            }
            Err(_) => {
                counter = counter
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("failed to derive pubkey for provided name"))?;
            }
        }
    }
}
