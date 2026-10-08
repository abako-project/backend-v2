use crate::state::Error;
use generated_contracts::PrincipalId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ClientProfile {
    pub(crate) name: String,
    pub(crate) company: Option<String>,
    pub(crate) department: Option<String>,
    pub(crate) website: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) location: Option<String>,
    pub(crate) languages: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct WorkerProfile {
    pub(crate) name: String,
    pub(crate) contact_email: Option<String>,
    pub(crate) github_username: Option<String>,
    pub(crate) portfolio_url: Option<String>,
    pub(crate) biography: Option<String>,
    pub(crate) background: Option<String>,
    pub(crate) proficiency: Option<Proficiency>,
    pub(crate) location: Option<String>,
    pub(crate) languages: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Proficiency {
    Junior,
    MidLevel,
    Senior,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "section", content = "profile", rename_all = "lowercase")]
pub(crate) enum PutProfileRequest {
    Client(ClientProfile),
    Worker(WorkerProfile),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProfilesView {
    pub(crate) principal_id: PrincipalId,
    pub(crate) client: Option<ClientProfile>,
    pub(crate) worker: Option<WorkerProfile>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublicProfilesView {
    pub(crate) principal_id: PrincipalId,
    pub(crate) client: Option<PublicClientProfile>,
    pub(crate) worker: Option<PublicWorkerProfile>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublicClientProfile {
    name: String,
    company: Option<String>,
    website: Option<String>,
    description: Option<String>,
    location: Option<String>,
    languages: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PublicWorkerProfile {
    name: String,
    github_username: Option<String>,
    portfolio_url: Option<String>,
    biography: Option<String>,
    location: Option<String>,
    languages: Vec<String>,
}

impl From<ClientProfile> for PublicClientProfile {
    fn from(value: ClientProfile) -> Self {
        Self {
            name: value.name,
            company: value.company,
            website: value.website,
            description: value.description,
            location: value.location,
            languages: value.languages,
        }
    }
}

impl From<WorkerProfile> for PublicWorkerProfile {
    fn from(value: WorkerProfile) -> Self {
        Self {
            name: value.name,
            github_username: value.github_username,
            portfolio_url: value.portfolio_url,
            biography: value.biography,
            location: value.location,
            languages: value.languages,
        }
    }
}

impl From<ProfilesView> for PublicProfilesView {
    fn from(value: ProfilesView) -> Self {
        Self {
            principal_id: value.principal_id,
            client: value.client.map(PublicClientProfile::from),
            worker: value.worker.map(PublicWorkerProfile::from),
        }
    }
}

fn required_text(value: &str, max_bytes: usize) -> Result<(), Error> {
    if value.trim().is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(Error::Invalid);
    }
    Ok(())
}

fn optional_text(value: Option<&str>, max_bytes: usize) -> Result<(), Error> {
    if let Some(value) = value {
        required_text(value, max_bytes)?;
    }
    Ok(())
}

fn url(value: Option<&str>) -> Result<(), Error> {
    if let Some(value) = value {
        required_text(value, 2_048)?;
        let parsed = reqwest::Url::parse(value).map_err(|_| Error::Invalid)?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(Error::Invalid);
        }
    }
    Ok(())
}

fn languages(values: &[String]) -> Result<(), Error> {
    if values.len() > 32 {
        return Err(Error::Invalid);
    }
    for value in values {
        required_text(value, 64)?;
    }
    Ok(())
}

impl ClientProfile {
    pub(super) fn validate(&self) -> Result<(), Error> {
        required_text(&self.name, 128)?;
        optional_text(self.company.as_deref(), 128)?;
        optional_text(self.department.as_deref(), 128)?;
        url(self.website.as_deref())?;
        optional_text(self.description.as_deref(), 4_096)?;
        optional_text(self.location.as_deref(), 128)?;
        languages(&self.languages)
    }
}

impl WorkerProfile {
    pub(super) fn validate(&self) -> Result<(), Error> {
        required_text(&self.name, 128)?;
        // ponytail: basic contact syntax; use a mailbox parser if full RFC support is required.
        if let Some(email) = &self.contact_email {
            required_text(email, 254)?;
            let (local, domain) = email.split_once('@').ok_or(Error::Invalid)?;
            if local.is_empty()
                || domain.is_empty()
                || domain.contains('@')
                || email.chars().any(char::is_whitespace)
            {
                return Err(Error::Invalid);
            }
        }
        if let Some(username) = &self.github_username
            && (username.len() > 39
                || username.is_empty()
                || username.starts_with('-')
                || username.ends_with('-')
                || !username
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
        {
            return Err(Error::Invalid);
        }
        url(self.portfolio_url.as_deref())?;
        optional_text(self.biography.as_deref(), 4_096)?;
        optional_text(self.background.as_deref(), 4_096)?;
        optional_text(self.location.as_deref(), 128)?;
        languages(&self.languages)
    }
}
