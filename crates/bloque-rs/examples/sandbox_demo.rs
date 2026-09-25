//! Minimum demo against the real Bloque sandbox: loads an existing session
//! (as saved by the official TS SDK / CLI at `~/.bloque/session.json`) and
//! lists the account's pockets/cards and their balances. Read-only — no
//! money-moving calls.
//!
//! This crate has no login flow of its own (see `Session::from_token`'s
//! docs) — it's just reusing a token obtained elsewhere, which is exactly
//! what `Session::from_token` is for.
//!
//! ```text
//! cargo run --example sandbox_demo
//! ```

// Examples talk to the terminal by design.
#![allow(clippy::print_stdout)]

use bloque::{Mode, Session};
use serde::Deserialize;

#[derive(Deserialize)]
struct SavedSession {
    #[serde(rename = "accessToken")]
    access_token: String,
    urn: String,
    origin: String,
    mode: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = format!("{}/.bloque/session.json", std::env::var("HOME")?);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("couldn't read {path} (log in with the TS SDK/CLI first): {e}"))?;
    let saved: SavedSession = serde_json::from_str(&raw)?;

    let mode = match saved.mode.as_str() {
        "sandbox" => Mode::Sandbox,
        "production" => Mode::Production,
        other => return Err(format!("unknown mode {other:?} in {path}").into()),
    };

    let session = Session::from_token(mode, saved.origin, saved.urn, saved.access_token)?;
    println!("connected as {}", session.urn());

    match session.kyc().get_verification(&session.urn()).await {
        Ok(v) => println!("kyc status: {:?} ({})", v.status, v.url),
        Err(e) => println!("kyc status: not available yet ({e})"),
    }

    let accounts = session.accounts().list(None).await?;
    if accounts.is_empty() {
        println!("no accounts yet");
    }
    for account in accounts {
        println!(
            "- {} [{:?}, {:?}]",
            account.urn, account.medium, account.status
        );
        let balance = session.accounts().balance(&account.urn).await?;
        for (asset, amount) in balance {
            println!(
                "    {asset}: {} (pending {})",
                amount.current, amount.pending
            );
        }
    }

    Ok(())
}
