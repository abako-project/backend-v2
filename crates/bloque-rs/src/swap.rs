use std::sync::Arc;

use reqwest::Method;
use serde::Deserialize;

use crate::asset::Asset;
use crate::error::{Error, Result};
use crate::http::HttpClient;
use crate::util::{build_query, wire_string_enum};

wire_string_enum! {
    /// A payment rail or settlement medium used in a swap/top-up, e.g.
    /// `"pse"` or `"kusama"`.
    pub enum SwapMedium {
        Kusama => "kusama",
        Pse => "pse",
        ExternalUsBank => "external-us-bank",
        Base => "base",
        Breb => "breb",
    }
}

/// Which side of a swap/top-up order has a fixed amount — the source
/// (what's paid in) or the destination (what should land). Exactly one is
/// always fixed; the other is whatever the locked-in rate yields. Bundling
/// this with the amount itself (rather than a separate `order_type` flag
/// plus two optional amount fields) makes the API's own "provide exactly
/// one" rule a compile-time guarantee instead of a runtime check.
#[derive(Debug, Clone)]
pub enum OrderAmount {
    /// Fix the amount paid in the source asset. Sent as `type: "src"`.
    Source(String),
    /// Fix the amount received in the destination asset. Sent as `type: "dst"`.
    Destination(String),
}

impl OrderAmount {
    fn order_type(&self) -> &'static str {
        match self {
            OrderAmount::Source(_) => "src",
            OrderAmount::Destination(_) => "dst",
        }
    }
}

/// Sort direction for [`SwapClient::find_rates`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    fn as_str(self) -> &'static str {
        match self {
            SortDirection::Asc => "asc",
            SortDirection::Desc => "desc",
        }
    }
}

/// Sort key for [`SwapClient::find_rates`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Rate,
    At,
}

impl SortBy {
    fn as_str(self) -> &'static str {
        match self {
            SortBy::Rate => "rate",
            SortBy::At => "at",
        }
    }
}

/// A quoted exchange rate from [`SwapClient::find_rates`]. Pass `sig` into a
/// top-up call within `until`'s expiration to lock it in.
#[derive(Debug, Clone, Deserialize)]
pub struct Rate {
    pub id: String,
    pub sig: String,
    pub swap_sig: String,
    pub maker: String,
    pub edge: (Asset, Asset),
    #[serde(default)]
    pub fee: serde_json::Value,
    pub at: serde_json::Value,
    pub until: String,
    #[serde(rename = "from_medium")]
    pub from_mediums: Vec<SwapMedium>,
    #[serde(rename = "to_medium")]
    pub to_mediums: Vec<SwapMedium>,
    #[allow(clippy::struct_field_names)] // wire name; renaming would break the public API
    pub rate: (f64, f64),
    pub ratio: f64,
    pub from_limits: (String, String),
    pub to_limits: (String, String),
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
struct RatesEnvelope {
    rates: Vec<Rate>,
}

/// Params for [`SwapClient::find_rates`].
#[derive(Debug, Clone)]
pub struct FindRatesParams {
    /// Asset the payer is paying with, e.g. `Asset::Copb6`.
    pub from_asset: Asset,
    /// Asset that should land in the destination account, e.g. `Asset::Dusd6`.
    pub to_asset: Asset,
    /// Payment rails to quote from (e.g. `[SwapMedium::Pse]`).
    pub from_mediums: Vec<SwapMedium>,
    /// Destination rails to quote to (e.g. `[SwapMedium::Kusama]`).
    pub to_mediums: Vec<SwapMedium>,
    pub amount: Option<OrderAmount>,
    pub sort: Option<SortDirection>,
    pub sort_by: Option<SortBy>,
}

impl FindRatesParams {
    pub fn new(
        from_asset: Asset,
        to_asset: Asset,
        from_mediums: Vec<SwapMedium>,
        to_mediums: Vec<SwapMedium>,
    ) -> Self {
        Self {
            from_asset,
            to_asset,
            from_mediums,
            to_mediums,
            amount: None,
            sort: None,
            sort_by: None,
        }
    }

    pub fn with_amount(mut self, amount: OrderAmount) -> Self {
        self.amount = Some(amount);
        self
    }

    pub fn with_sort(mut self, sort: SortDirection) -> Self {
        self.sort = Some(sort);
        self
    }

    pub fn with_sort_by(mut self, sort_by: SortBy) -> Self {
        self.sort_by = Some(sort_by);
        self
    }
}

/// A created swap/top-up order.
#[derive(Debug, Clone, Deserialize)]
pub struct SwapOrder {
    pub id: String,
    pub order_sig: String,
    pub rate_sig: String,
    pub swap_sig: String,
    pub taker: String,
    pub maker: String,
    pub from_asset: Asset,
    pub to_asset: Asset,
    pub from_medium: SwapMedium,
    pub to_medium: SwapMedium,
    pub from_amount: String,
    pub to_amount: String,
    pub at: serde_json::Value,
    pub graph_id: String,
    pub status: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    pub webhook_url: Option<String>,
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub failure_details: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

/// Result of creating a top-up order. `execution`, when present, tells you
/// how the payer completes the payment — a redirect URL (PSE), BRE-B deposit
/// key, wallet address, etc. Its shape varies by rail, so it's left as raw
/// JSON here; match on `execution["result"]["how"]["type"]`
/// (`"REDIRECT" | "CALLBACK" | "IFRAME" | "BREB_DEPOSIT" | "WALLET_TRANSFER"`).
#[derive(Debug, Clone)]
pub struct CreateOrderResult {
    pub order: SwapOrder,
    pub execution: Option<serde_json::Value>,
    pub request_id: Option<String>,
}

impl CreateOrderResult {
    /// The URL to send the payer to, for the common case where completing
    /// the top-up means a redirect (PSE's normal flow). `None` for every
    /// other completion type — a BRE-B deposit key, a wallet address to
    /// send crypto to, an async callback, ... — inspect `execution`
    /// directly for those.
    pub fn redirect_url(&self) -> Option<&str> {
        self.execution
            .as_ref()?
            .get("result")?
            .get("how")?
            .get("url")?
            .as_str()
    }
}

#[derive(Deserialize)]
struct OrderEnvelope {
    result: OrderResult,
    req_id: Option<String>,
}

#[derive(Deserialize)]
struct OrderResult {
    order: SwapOrder,
    #[serde(default)]
    execution: Option<serde_json::Value>,
}

/// Render `["a","b"]`, the wire form of the list-valued query parameters.
fn json_string_array<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    serde_json::Value::from_iter(items).to_string()
}

fn idempotency_header(key: Option<&str>) -> Result<Option<reqwest::header::HeaderMap>> {
    match key {
        None => Ok(None),
        Some(k) => {
            let mut headers = reqwest::header::HeaderMap::new();
            let value = reqwest::header::HeaderValue::from_str(k)
                .map_err(|e| Error::Config(format!("invalid idempotency key: {e}")))?;
            headers.insert("Idempotency-Key", value);
            Ok(Some(headers))
        }
    }
}

fn apply_amount(body: &mut serde_json::Value, amount: &OrderAmount) {
    body["type"] = serde_json::json!(amount.order_type());
    match amount {
        OrderAmount::Source(v) => body["amount_src"] = serde_json::json!(v),
        OrderAmount::Destination(v) => body["amount_dst"] = serde_json::json!(v),
    }
}

/// A Colombian PSE bank, from [`PseClient::banks`].
#[derive(Debug, Clone, Deserialize)]
pub struct Bank {
    pub code: String,
    pub name: String,
}

#[derive(Deserialize)]
struct RawBank {
    financial_institution_code: String,
    financial_institution_name: String,
}

#[derive(Deserialize)]
struct BanksEnvelope {
    banks: Vec<RawBank>,
}

/// The PSE payer's legal-entity type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PseUserType {
    Natural,
    Legal,
}

impl PseUserType {
    fn as_u8(self) -> u8 {
        match self {
            PseUserType::Natural => 0,
            PseUserType::Legal => 1,
        }
    }
}

/// Colombian identification type, as PSE accepts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColombianIdType {
    /// Cédula de Ciudadanía.
    Cc,
    /// Número de Identificación Tributaria.
    Nit,
    /// Cédula de Extranjería.
    Ce,
}

impl ColombianIdType {
    fn as_str(self) -> &'static str {
        match self {
            ColombianIdType::Cc => "CC",
            ColombianIdType::Nit => "NIT",
            ColombianIdType::Ce => "CE",
        }
    }
}

/// Who is paying via PSE, grouped into one value. PSE's own request shape
/// has 6 fields describing the payer; passing them as one `PsePayer` instead
/// of 6 same-typed positional arguments to [`PseTopUpParams::new`] removes
/// the chance of silently swapping, say, `full_name` and `phone_number` —
/// the compiler can't catch two `String`s in the wrong order, but it can't
/// mix up fields on named structs either.
#[derive(Debug, Clone)]
pub struct PsePayer {
    pub user_type: PseUserType,
    pub email: String,
    pub legal_id_type: ColombianIdType,
    pub legal_id: String,
    pub full_name: String,
    pub phone_number: String,
}

impl PsePayer {
    pub fn new(
        user_type: PseUserType,
        email: impl Into<String>,
        legal_id_type: ColombianIdType,
        legal_id: impl Into<String>,
        full_name: impl Into<String>,
        phone_number: impl Into<String>,
    ) -> Self {
        Self {
            user_type,
            email: email.into(),
            legal_id_type,
            legal_id: legal_id.into(),
            full_name: full_name.into(),
            phone_number: phone_number.into(),
        }
    }
}

/// Params for [`PseClient::top_up`] — a real-time bank-debit top-up via
/// Colombia's PSE rail. **Money-moving**: get explicit confirmation from the
/// payer before calling this.
#[derive(Debug, Clone)]
pub struct PseTopUpParams {
    pub rate_sig: String,
    /// Destination medium inside Bloque (e.g. `SwapMedium::Kusama`, or a
    /// Bloque-specific settlement medium not yet named as its own variant —
    /// pass `SwapMedium::Other(..)` for that).
    pub to_medium: SwapMedium,
    /// The account URN that should receive the funds.
    pub deposit_account_urn: String,
    /// From [`PseClient::banks`].
    pub bank_code: String,
    pub payer: PsePayer,
    /// Where the bank sends the customer back after the PSE flow completes.
    pub redirect_url: String,
    pub amount: OrderAmount,
    pub webhook_url: Option<String>,
    pub node_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub idempotency_key: Option<String>,
}

impl PseTopUpParams {
    pub fn new(
        rate_sig: impl Into<String>,
        to_medium: SwapMedium,
        deposit_account_urn: impl Into<String>,
        bank_code: impl Into<String>,
        payer: PsePayer,
        redirect_url: impl Into<String>,
        amount: OrderAmount,
    ) -> Self {
        Self {
            rate_sig: rate_sig.into(),
            to_medium,
            deposit_account_urn: deposit_account_urn.into(),
            bank_code: bank_code.into(),
            payer,
            redirect_url: redirect_url.into(),
            amount,
            webhook_url: None,
            node_id: None,
            metadata: None,
            idempotency_key: None,
        }
    }

    pub fn with_webhook_url(mut self, v: impl Into<String>) -> Self {
        self.webhook_url = Some(v.into());
        self
    }

    pub fn with_node_id(mut self, v: impl Into<String>) -> Self {
        self.node_id = Some(v.into());
        self
    }

    pub fn with_metadata(mut self, v: serde_json::Value) -> Self {
        self.metadata = Some(v);
        self
    }

    pub fn with_idempotency_key(mut self, v: impl Into<String>) -> Self {
        self.idempotency_key = Some(v.into());
        self
    }
}

/// Colombian PSE bank-debit top-ups.
pub struct PseClient {
    http: Arc<HttpClient>,
}

impl PseClient {
    pub(crate) fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    pub async fn banks(&self) -> Result<Vec<Bank>> {
        let resp: BanksEnvelope = self
            .http
            .request(
                Method::GET,
                "/api/utils/pse/banks",
                None::<&()>,
                None,
                false,
            )
            .await?;
        Ok(resp
            .banks
            .into_iter()
            .map(|b| Bank {
                code: b.financial_institution_code,
                name: b.financial_institution_name,
            })
            .collect())
    }

    pub async fn top_up(&self, params: PseTopUpParams) -> Result<CreateOrderResult> {
        let taker_urn = self.http.require_urn()?;

        let mut body = serde_json::json!({
            "taker_urn": taker_urn,
            "rate_sig": params.rate_sig,
            "from_medium": "pse",
            "to_medium": params.to_medium.as_str(),
            "webhook_url": params.webhook_url,
            "deposit_information": { "urn": params.deposit_account_urn },
            "args": {
                "bank_code": params.bank_code,
                "user_type": params.payer.user_type.as_u8(),
                "customer_email": params.payer.email,
                "user_legal_id_type": params.payer.legal_id_type.as_str(),
                "user_legal_id": params.payer.legal_id,
                "redirect_url": params.redirect_url,
                "customer_data": {
                    "full_name": params.payer.full_name,
                    "phone_number": params.payer.phone_number,
                },
            },
        });
        apply_amount(&mut body, &params.amount);
        if let Some(node_id) = &params.node_id {
            body["node_id"] = serde_json::json!(node_id);
        }
        if let Some(metadata) = &params.metadata {
            body["metadata"] = metadata.clone();
        }

        let headers = idempotency_header(params.idempotency_key.as_deref())?;
        let resp: OrderEnvelope = self
            .http
            .request(Method::PUT, "/api/order", Some(&body), headers, false)
            .await?;
        Ok(CreateOrderResult {
            order: resp.result.order,
            execution: resp.result.execution,
            request_id: resp.req_id,
        })
    }
}

/// Where an ACH-pulled US-bank top-up should land. Carrying each
/// destination's required field on the variant itself (rather than a
/// `to_medium` string plus a grab-bag of optional `ledger_account_id` /
/// `wallet_address` fields) makes "Base needs a wallet address, Kusama needs
/// a ledger account" a compile-time guarantee instead of a runtime check.
#[derive(Debug, Clone)]
pub enum ExternalUsBankDestination {
    /// Land DUSD on Kusama, at this ledger account.
    Kusama { ledger_account_id: String },
    /// Land USDC on Base, at this `0x` wallet address.
    Base {
        wallet_address: String,
        wallet_name: Option<String>,
    },
}

impl ExternalUsBankDestination {
    pub fn kusama(ledger_account_id: impl Into<String>) -> Self {
        Self::Kusama {
            ledger_account_id: ledger_account_id.into(),
        }
    }

    pub fn base(wallet_address: impl Into<String>) -> Self {
        Self::Base {
            wallet_address: wallet_address.into(),
            wallet_name: None,
        }
    }

    pub fn base_named(wallet_address: impl Into<String>, wallet_name: impl Into<String>) -> Self {
        Self::Base {
            wallet_address: wallet_address.into(),
            wallet_name: Some(wallet_name.into()),
        }
    }
}

/// Params for [`ExternalUsBankClient::top_up`] — an ACH pull from a linked
/// US bank account. **Money-moving**: get explicit confirmation from the
/// account holder before calling this.
#[derive(Debug, Clone)]
pub struct ExternalUsBankTopUpParams {
    pub rate_sig: String,
    /// The linked `external-us-bank` account URN funds are pulled from.
    pub source_account_urn: String,
    pub destination: ExternalUsBankDestination,
    pub amount: OrderAmount,
    pub webhook_url: Option<String>,
    pub node_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub idempotency_key: Option<String>,
}

impl ExternalUsBankTopUpParams {
    pub fn new(
        rate_sig: impl Into<String>,
        source_account_urn: impl Into<String>,
        destination: ExternalUsBankDestination,
        amount: OrderAmount,
    ) -> Self {
        Self {
            rate_sig: rate_sig.into(),
            source_account_urn: source_account_urn.into(),
            destination,
            amount,
            webhook_url: None,
            node_id: None,
            metadata: None,
            idempotency_key: None,
        }
    }

    pub fn with_webhook_url(mut self, v: impl Into<String>) -> Self {
        self.webhook_url = Some(v.into());
        self
    }

    pub fn with_node_id(mut self, v: impl Into<String>) -> Self {
        self.node_id = Some(v.into());
        self
    }

    pub fn with_metadata(mut self, v: serde_json::Value) -> Self {
        self.metadata = Some(v);
        self
    }

    pub fn with_idempotency_key(mut self, v: impl Into<String>) -> Self {
        self.idempotency_key = Some(v.into());
        self
    }
}

/// US ACH top-ups from a linked external bank account.
pub struct ExternalUsBankClient {
    http: Arc<HttpClient>,
}

impl ExternalUsBankClient {
    pub(crate) fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    pub async fn top_up(&self, params: ExternalUsBankTopUpParams) -> Result<CreateOrderResult> {
        let taker_urn = self.http.require_urn()?;

        let (to_medium, deposit_information) = match &params.destination {
            ExternalUsBankDestination::Kusama { ledger_account_id } => (
                "kusama",
                serde_json::json!({ "ledger_account_id": ledger_account_id }),
            ),
            ExternalUsBankDestination::Base {
                wallet_address,
                wallet_name,
            } => {
                let mut v = serde_json::json!({ "wallet_address": wallet_address });
                if let Some(name) = wallet_name {
                    v["wallet_name"] = serde_json::json!(name);
                }
                ("base", v)
            }
        };

        let mut body = serde_json::json!({
            "taker_urn": taker_urn,
            "rate_sig": params.rate_sig,
            "from_medium": "external-us-bank",
            "to_medium": to_medium,
            "webhook_url": params.webhook_url,
            "deposit_information": deposit_information,
            "args": { "account_urn": params.source_account_urn },
        });
        apply_amount(&mut body, &params.amount);
        if let Some(node_id) = &params.node_id {
            body["node_id"] = serde_json::json!(node_id);
        }
        if let Some(metadata) = &params.metadata {
            body["metadata"] = metadata.clone();
        }

        let headers = idempotency_header(params.idempotency_key.as_deref())?;
        let resp: OrderEnvelope = self
            .http
            .request(Method::PUT, "/api/order", Some(&body), headers, false)
            .await?;
        Ok(CreateOrderResult {
            order: resp.result.order,
            execution: resp.result.execution,
            request_id: resp.req_id,
        })
    }
}

/// Rate discovery plus top-up rails for one [`crate::Session`]'s user.
///
/// Only top-up (pay-in) rails are implemented: [`PseClient`] (Colombian
/// bank debit) and [`ExternalUsBankClient`] (US ACH pull). Bloque's swap API
/// also has `bank_transfer` and `rtp` clients for **cash-out** (pay-out) —
/// out of scope here, but they follow the identical `PUT /api/order`
/// pattern with a different `from_medium`/`to_medium`/`args` shape, so
/// adding them is a small extension of this module.
pub struct SwapClient {
    http: Arc<HttpClient>,
}

impl SwapClient {
    pub(crate) fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    pub async fn find_rates(&self, params: FindRatesParams) -> Result<Vec<Rate>> {
        let mut qp = vec![
            (
                "edge".to_string(),
                json_string_array([params.from_asset.as_str(), params.to_asset.as_str()]),
            ),
            (
                "from_medium".to_string(),
                json_string_array(params.from_mediums.iter().map(SwapMedium::as_str)),
            ),
            (
                "to_medium".to_string(),
                json_string_array(params.to_mediums.iter().map(SwapMedium::as_str)),
            ),
        ];
        match &params.amount {
            Some(OrderAmount::Source(v)) => qp.push(("amount_src".to_string(), v.clone())),
            Some(OrderAmount::Destination(v)) => qp.push(("amount_dst".to_string(), v.clone())),
            None => {}
        }
        if let Some(v) = params.sort {
            qp.push(("sort".to_string(), v.as_str().to_string()));
        }
        if let Some(v) = params.sort_by {
            qp.push(("sort_by".to_string(), v.as_str().to_string()));
        }

        let query = build_query(&qp);
        let resp: RatesEnvelope = self
            .http
            .request(
                Method::GET,
                &format!("/api/rates?{query}"),
                None::<&()>,
                None,
                false,
            )
            .await?;
        Ok(resp.rates)
    }

    pub fn pse(&self) -> PseClient {
        PseClient::new(self.http.clone())
    }

    pub fn external_us_bank(&self) -> ExternalUsBankClient {
        ExternalUsBankClient::new(self.http.clone())
    }
}
