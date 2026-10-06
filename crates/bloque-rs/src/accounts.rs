use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Method;
use serde::Deserialize;

use crate::asset::Asset;
use crate::error::{ApiErrorDetail, Error, Result};
use crate::http::HttpClient;
use crate::util::wire_string_enum;

wire_string_enum! {
    /// The kind of instrument an [`Account`] is.
    pub enum AccountMedium {
        Virtual => "virtual",
        Card => "card",
        Polygon => "polygon",
        Bancolombia => "bancolombia",
        UsAccount => "us-account",
        Us2Account => "us2-account",
        ExternalUsBank => "external-us-bank",
        Breb => "breb",
    }
}

wire_string_enum! {
    /// Lifecycle state of an [`Account`].
    pub enum AccountStatus {
        Active => "active",
        Disabled => "disabled",
        Frozen => "frozen",
        Deleted => "deleted",
        CreationInProgress => "creation_in_progress",
        CreationFailed => "creation_failed",
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenBalance {
    pub current: String,
    pub pending: String,
    #[serde(rename = "in")]
    pub incoming: String,
    pub out: String,
}

/// A Bloque account of any medium (pocket, card, Polygon wallet, bank
/// account, ...). Fields common to every medium are typed here; anything
/// medium-specific (a card's last four digits, a Polygon address, ...) lives
/// in `details` as raw JSON — this crate doesn't replicate the full
/// medium-union type the TS SDK has, to stay small. Inspect `details` at
/// runtime, or extend this struct if you need one medium's fields typed.
#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    pub urn: String,
    pub id: String,
    pub medium: AccountMedium,
    pub status: AccountStatus,
    pub owner_urn: Option<String>,
    #[serde(rename = "ledger_account_id")]
    pub ledger_id: Option<String>,
    pub webhook_url: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
    #[serde(default)]
    pub details: serde_json::Value,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    #[serde(default)]
    pub balance: Option<HashMap<Asset, TokenBalance>>,
}

#[derive(Deserialize)]
struct AccountEnvelope {
    account: Account,
}

#[derive(Deserialize)]
struct CreateAccountEnvelope {
    result: CreateAccountResult,
}

#[derive(Deserialize)]
struct CreateAccountResult {
    account: Account,
}

#[derive(Deserialize)]
struct ListAccountsEnvelope {
    accounts: Vec<Account>,
}

#[derive(Deserialize)]
struct BalanceEnvelope {
    balance: HashMap<Asset, TokenBalance>,
}

/// Params for [`AccountsClient::create_virtual_account`].
#[derive(Debug, Clone, Default)]
pub struct CreateVirtualAccountParams {
    pub name: Option<String>,
    /// Attach to an existing ledger to **share balance** with another
    /// account (card, Polygon wallet, bank account, ...) that was created
    /// with the same `ledger_id`. Omit to provision a fresh ledger.
    pub ledger_id: Option<String>,
    pub webhook_url: Option<String>,
    pub metadata: serde_json::Map<String, serde_json::Value>,
    /// Poll until the account reaches `active` before returning. Turn this
    /// on whenever you need `ledger_id` right away (e.g. to create a card
    /// linked to this pocket next) — otherwise the account may still be
    /// `creation_in_progress` when this call returns.
    pub wait_for_ledger: bool,
}

/// Params for [`AccountsClient::create_card`]. Creates a **virtual** card
/// only — physical card issuance (which needs a mailing address) isn't
/// implemented here. Spending-control / fee metadata can be passed through
/// `metadata` raw; see the Bloque cards-and-spending-controls reference for
/// its shape.
#[derive(Debug, Clone, Default)]
pub struct CreateCardParams {
    pub name: Option<String>,
    /// Link this card to a pocket's ledger so they share balance.
    pub ledger_id: Option<String>,
    pub webhook_url: Option<String>,
    pub metadata: serde_json::Map<String, serde_json::Value>,
    pub wait_for_ledger: bool,
}

/// Accounts for one [`crate::Session`]'s user: create pockets/cards, fetch
/// balances, list what they own.
pub struct AccountsClient {
    http: Arc<HttpClient>,
}

impl AccountsClient {
    pub(crate) fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    fn build_metadata(
        name: Option<&str>,
        extra: &serde_json::Map<String, serde_json::Value>,
    ) -> serde_json::Map<String, serde_json::Value> {
        let mut metadata = serde_json::Map::new();
        metadata.insert("source".into(), serde_json::json!("bloque-rs"));
        if let Some(name) = name {
            metadata.insert("name".into(), serde_json::json!(name));
        }
        for (k, v) in extra {
            metadata.insert(k.clone(), v.clone());
        }
        metadata
    }

    /// Create a virtual account ("pocket") for this user — the base account
    /// every other medium (card, Polygon wallet, bank account) can share a
    /// balance with via `ledger_id`.
    pub async fn create_virtual_account(
        &self,
        params: CreateVirtualAccountParams,
    ) -> Result<Account> {
        let holder_urn = self.http.require_urn()?;
        let metadata = Self::build_metadata(params.name.as_deref(), &params.metadata);

        let body = serde_json::json!({
            "holder_urn": holder_urn,
            "webhook_url": params.webhook_url,
            "ledger_account_id": params.ledger_id,
            "input": {},
            "metadata": metadata,
        });

        let resp: CreateAccountEnvelope = self
            .http
            .request(
                Method::POST,
                "/api/mediums/virtual",
                Some(&body),
                None,
                false,
            )
            .await?;
        let account = resp.result.account;

        if params.wait_for_ledger {
            self.wait_for_active(&account.urn, Duration::from_mins(1))
                .await
        } else {
            Ok(account)
        }
    }

    /// Create a virtual card for this user.
    pub async fn create_card(&self, params: CreateCardParams) -> Result<Account> {
        let holder_urn = self.http.require_urn()?;
        let metadata = Self::build_metadata(params.name.as_deref(), &params.metadata);

        let body = serde_json::json!({
            "holder_urn": holder_urn,
            "webhook_url": params.webhook_url,
            "ledger_account_id": params.ledger_id,
            "input": { "create": { "card_type": "VIRTUAL" } },
            "metadata": metadata,
        });

        let resp: CreateAccountEnvelope = self
            .http
            .request(Method::POST, "/api/mediums/card", Some(&body), None, false)
            .await?;
        let account = resp.result.account;

        if params.wait_for_ledger {
            self.wait_for_active(&account.urn, Duration::from_mins(1))
                .await
        } else {
            Ok(account)
        }
    }

    async fn wait_for_active(&self, urn: &str, timeout: Duration) -> Result<Account> {
        let start = Instant::now();
        loop {
            let account = self.get(urn).await?;
            if account.status == AccountStatus::Active {
                return Ok(account);
            }
            if account.status == AccountStatus::CreationFailed {
                return Err(Error::Api {
                    status: None,
                    message: format!("account creation failed: {urn}"),
                    detail: ApiErrorDetail::default(),
                });
            }
            if start.elapsed() > timeout {
                return Err(Error::Timeout(timeout));
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    /// Fetch a single account by URN, of any medium.
    pub async fn get(&self, urn: &str) -> Result<Account> {
        let resp: AccountEnvelope = self
            .http
            .request(
                Method::GET,
                &format!("/api/accounts/{urn}"),
                None::<&()>,
                None,
                false,
            )
            .await?;
        Ok(resp.account)
    }

    /// List every account this user holds, optionally filtered to one medium.
    pub async fn list(&self, medium: Option<AccountMedium>) -> Result<Vec<Account>> {
        let holder_urn = self.http.require_urn()?;
        let mut qp = vec![("holder_urn".to_string(), holder_urn)];
        if let Some(m) = medium {
            qp.push(("medium".to_string(), m.as_str().to_string()));
        }
        let query = crate::util::build_query(&qp);
        let resp: ListAccountsEnvelope = self
            .http
            .request(
                Method::GET,
                &format!("/api/accounts?{query}"),
                None::<&()>,
                None,
                false,
            )
            .await?;
        Ok(resp.accounts)
    }

    /// Current balances for an account, keyed by asset.
    pub async fn balance(&self, urn: &str) -> Result<HashMap<Asset, TokenBalance>> {
        let resp: BalanceEnvelope = self
            .http
            .request(
                Method::GET,
                &format!("/api/accounts/{urn}/balance"),
                None::<&()>,
                None,
                false,
            )
            .await?;
        Ok(resp.balance)
    }
}
