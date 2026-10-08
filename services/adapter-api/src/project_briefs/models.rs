use crate::state::Error;
use generated_contracts::EntityId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProjectBrief {
    pub(crate) summary: String,
    pub(crate) project_type: ProjectType,
    pub(crate) link: Option<String>,
    pub(crate) objectives: Vec<String>,
    pub(crate) constraints: Vec<String>,
    pub(crate) indicative_budget: IndicativeBudget,
    pub(crate) delivery: DeliveryPreference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ProjectType {
    Other,
    SmartContract,
    Frontend,
    #[serde(rename = "MVP")]
    Mvp,
    Audit,
    MobileApp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct IndicativeBudget {
    pub(crate) currency: BudgetCurrency,
    pub(crate) range: BudgetRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum BudgetCurrency {
    #[serde(rename = "USD")]
    Usd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum BudgetRange {
    Below10000,
    From10000To50000,
    From50000To100000,
    Above100000,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "preference", deny_unknown_fields)]
pub(crate) enum DeliveryPreference {
    WithinOneMonth {},
    OneToThreeMonths {},
    ThreeToSixMonths {},
    SpecificDate { date: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PutProjectBriefRequest {
    pub(crate) expected_revision: i64,
    pub(crate) brief: ProjectBrief,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProjectBriefView {
    pub(crate) project_id: EntityId,
    pub(crate) revision: i64,
    pub(crate) brief: ProjectBrief,
}

impl PutProjectBriefRequest {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if !(0..i64::MAX).contains(&self.expected_revision) {
            return Err(Error::Invalid);
        }
        self.brief.validate()
    }
}

impl ProjectBrief {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.summary.contains('\0') || self.summary.chars().count() > 280 {
            return Err(Error::Invalid);
        }
        for items in [&self.objectives, &self.constraints] {
            if items.len() > 50
                || items.iter().any(|item| {
                    item.contains('\0') || item.trim().is_empty() || item.chars().count() > 2000
                })
            {
                return Err(Error::Invalid);
            }
        }
        if let Some(link) = &self.link {
            let url = reqwest::Url::parse(link).map_err(|_| Error::Invalid)?;
            if link.split_once("://").is_none()
                || link.len() > 2048
                || link.chars().any(|c| c.is_whitespace() || c.is_control())
                || !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(Error::Invalid);
            }
        }
        if let DeliveryPreference::SpecificDate { date } = &self.delivery {
            validate_date(date)?;
        }
        Ok(())
    }
}

fn validate_date(date: &str) -> Result<(), Error> {
    if date.len() != 10
        || !date.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 => b == b'-',
            _ => b.is_ascii_digit(),
        })
    {
        return Err(Error::Invalid);
    }
    let year = date[..4].parse::<u32>().map_err(|_| Error::Invalid)?;
    let month = date[5..7].parse::<u32>().map_err(|_| Error::Invalid)?;
    let day = date[8..].parse::<u32>().map_err(|_| Error::Invalid)?;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        _ => return Err(Error::Invalid),
    };
    if year == 0 || !(1..=days).contains(&day) {
        return Err(Error::Invalid);
    }
    Ok(())
}
