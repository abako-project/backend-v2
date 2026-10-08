use super::media::{ImageKind, ProfileSection};
use super::models::{ClientProfile, Proficiency, ProfilesView, WorkerProfile};
use crate::state::{Error, now};
use generated_contracts::PrincipalId;
use sqlx::{PgPool, Row};

impl Proficiency {
    fn as_db_value(&self) -> &'static str {
        match self {
            Self::Junior => "junior",
            Self::MidLevel => "mid-level",
            Self::Senior => "senior",
        }
    }

    fn from_db_value(value: &str) -> Result<Self, Error> {
        match value {
            "junior" => Ok(Self::Junior),
            "mid-level" => Ok(Self::MidLevel),
            "senior" => Ok(Self::Senior),
            _ => Err(Error::Internal),
        }
    }
}

async fn client(pool: &PgPool, id: PrincipalId) -> Result<Option<ClientProfile>, Error> {
    let row = sqlx::query("SELECT name, company, department, website, description, location, languages FROM client_profiles WHERE principal_id = $1")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
    row.map(|row| {
        Ok(ClientProfile {
            name: row.try_get("name")?,
            company: row.try_get("company")?,
            department: row.try_get("department")?,
            website: row.try_get("website")?,
            description: row.try_get("description")?,
            location: row.try_get("location")?,
            languages: row.try_get("languages")?,
        })
    })
    .transpose()
}

async fn worker(pool: &PgPool, id: PrincipalId) -> Result<Option<WorkerProfile>, Error> {
    let row = sqlx::query("SELECT name, contact_email, github_username, portfolio_url, biography, background, proficiency, location, languages FROM worker_profiles WHERE principal_id = $1")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
    row.map(|row| {
        let proficiency: Option<String> = row.try_get("proficiency")?;
        Ok(WorkerProfile {
            name: row.try_get("name")?,
            contact_email: row.try_get("contact_email")?,
            github_username: row.try_get("github_username")?,
            portfolio_url: row.try_get("portfolio_url")?,
            biography: row.try_get("biography")?,
            background: row.try_get("background")?,
            proficiency: proficiency
                .as_deref()
                .map(Proficiency::from_db_value)
                .transpose()?,
            location: row.try_get("location")?,
            languages: row.try_get("languages")?,
        })
    })
    .transpose()
}

pub(crate) async fn read(pool: &PgPool, id: PrincipalId) -> Result<ProfilesView, Error> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM principals WHERE principal_id = $1)",
    )
    .bind(id.to_string())
    .fetch_one(pool)
    .await?;
    if !exists {
        return Err(Error::NotFound);
    }
    let (client, worker) = tokio::try_join!(client(pool, id), worker(pool, id))?;
    Ok(ProfilesView {
        principal_id: id,
        client,
        worker,
    })
}

pub(super) async fn write_client(
    pool: &PgPool,
    id: PrincipalId,
    profile: ClientProfile,
) -> Result<(), Error> {
    profile.validate()?;
    sqlx::query("INSERT INTO client_profiles (principal_id, name, company, department, website, description, location, languages, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (principal_id) DO UPDATE SET name=EXCLUDED.name, company=EXCLUDED.company, department=EXCLUDED.department, website=EXCLUDED.website, description=EXCLUDED.description, location=EXCLUDED.location, languages=EXCLUDED.languages, updated_at=EXCLUDED.updated_at")
        .bind(id.to_string())
        .bind(profile.name)
        .bind(profile.company)
        .bind(profile.department)
        .bind(profile.website)
        .bind(profile.description)
        .bind(profile.location)
        .bind(profile.languages)
        .bind(now()?)
        .execute(pool)
        .await?;
    Ok(())
}

pub(super) async fn write_worker(
    pool: &PgPool,
    id: PrincipalId,
    profile: WorkerProfile,
) -> Result<(), Error> {
    profile.validate()?;
    sqlx::query("INSERT INTO worker_profiles (principal_id, name, contact_email, github_username, portfolio_url, biography, background, proficiency, location, languages, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT (principal_id) DO UPDATE SET name=EXCLUDED.name, contact_email=EXCLUDED.contact_email, github_username=EXCLUDED.github_username, portfolio_url=EXCLUDED.portfolio_url, biography=EXCLUDED.biography, background=EXCLUDED.background, proficiency=EXCLUDED.proficiency, location=EXCLUDED.location, languages=EXCLUDED.languages, updated_at=EXCLUDED.updated_at")
        .bind(id.to_string())
        .bind(profile.name)
        .bind(profile.contact_email)
        .bind(profile.github_username)
        .bind(profile.portfolio_url)
        .bind(profile.biography)
        .bind(profile.background)
        .bind(profile.proficiency.as_ref().map(Proficiency::as_db_value))
        .bind(profile.location)
        .bind(profile.languages)
        .bind(now()?)
        .execute(pool)
        .await?;
    Ok(())
}

pub(super) async fn write_image(
    pool: &PgPool,
    id: PrincipalId,
    section: ProfileSection,
    kind: ImageKind,
    bytes: &[u8],
) -> Result<(), Error> {
    let updated = sqlx::query(section.update_sql())
        .bind(bytes)
        .bind(kind.mime())
        .bind(id.to_string())
        .execute(pool)
        .await?;
    if updated.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    Ok(())
}

pub(super) async fn read_image(
    pool: &PgPool,
    id: PrincipalId,
    section: ProfileSection,
) -> Result<(ImageKind, Vec<u8>), Error> {
    let row = sqlx::query(section.read_sql())
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(Error::NotFound)?;
    let data: Option<Vec<u8>> = row.try_get("image_data")?;
    let mime: Option<String> = row.try_get("image_mime_type")?;
    match (data, mime) {
        (Some(data), Some(mime)) => {
            Ok((ImageKind::parse(&mime).map_err(|_| Error::Internal)?, data))
        }
        (None, None) => Err(Error::NotFound),
        _ => Err(Error::Internal),
    }
}
