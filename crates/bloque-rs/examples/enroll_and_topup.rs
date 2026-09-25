//! End-to-end example: an org enrolls a user, gives them a pocket, quotes a
//! top-up rate, and starts a PSE (Colombian bank-debit) top-up.
//!
//! Requires `ORIGIN` and `ORIGIN_KEY` env vars for a sandbox origin
//! configured with an `API_KEY` challenge type. Run with:
//!
//! ```text
//! ORIGIN=my-origin ORIGIN_KEY=my-origin-key cargo run --example enroll_and_topup
//! ```

// Examples talk to the terminal by design.
#![allow(clippy::print_stdout)]

use bloque::{
    Asset, Bloque, ColombianIdType, CountryCode, CreateVirtualAccountParams, FindRatesParams,
    IndividualProfile, OrderAmount, PsePayer, PseTopUpParams, PseUserType, SwapMedium,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let origin = std::env::var("ORIGIN")?;
    let origin_key = std::env::var("ORIGIN_KEY")?;

    let bloque = Bloque::sandbox(origin, origin_key)?;

    // 1. Enroll the user. In a real app, check your own database first —
    //    `connect()` doesn't tell you whether an alias was ever registered.
    let alias = "@alice-demo";
    let session = bloque
        .register_individual(
            alias,
            IndividualProfile {
                first_name: Some("Alice".into()),
                last_name: Some("Smith".into()),
                email: Some("alice@example.com".into()),
                phone: Some("+13055551234".into()),
                birthdate: Some("1990-01-01".into()),
                city: Some("Miami".into()),
                state: Some("FL".into()),
                postal_code: Some("33101".into()),
                country_of_birth_code: Some(CountryCode::new("USA")?),
                country_of_residence_code: Some(CountryCode::new("USA")?),
                ..Default::default()
            },
        )
        .await?;
    println!("enrolled: {}", session.urn());

    // 2. Give them an account (a pocket). `wait_for_ledger: true` so
    //    `ledger_id` is ready immediately.
    let pocket = session
        .accounts()
        .create_virtual_account(CreateVirtualAccountParams {
            name: Some("Main pocket".into()),
            wait_for_ledger: true,
            ..Default::default()
        })
        .await?;
    println!("pocket: {} (ledger {:?})", pocket.urn, pocket.ledger_id);

    // 3. Quote a COP -> DUSD rate for a PSE top-up.
    let rates = session
        .swap()
        .find_rates(
            FindRatesParams::new(
                Asset::Copb6,
                Asset::Dusd6,
                vec![SwapMedium::Pse],
                vec![SwapMedium::Kusama],
            )
            .with_amount(OrderAmount::Source("500000".into())), // 5,000.00 COP
        )
        .await?;
    let rate = rates.first().ok_or("no rate available")?;
    println!(
        "quoted rate: {} -> {} until {}",
        rate.sig, pocket.urn, rate.until
    );

    // 4. Start the top-up. This is money-moving — in a real app, get
    //    explicit confirmation from Alice before calling this, and show her
    //    the redirect URL to complete the payment on her bank's PSE page.
    let payer = PsePayer::new(
        PseUserType::Natural,
        "alice@example.com",
        ColombianIdType::Cc,
        "123456789",
        "Alice Smith",
        "+13055551234",
    );
    let order = session
        .swap()
        .pse()
        .top_up(PseTopUpParams::new(
            rate.sig.clone(),
            SwapMedium::from("kreivo"), // Bloque-internal settlement medium
            pocket.urn.clone(),
            "1007", // bank code, from session.swap().pse().banks()
            payer,
            "https://example.com/payment-status",
            OrderAmount::Source("500000".into()),
        ))
        .await?;

    println!("order {} status {}", order.order.id, order.order.status);
    if let Some(url) = order.redirect_url() {
        println!("complete payment: {url}");
    }

    Ok(())
}
