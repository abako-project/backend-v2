use super::store;
use crate::{
    auth::Session,
    state::{App, Error, parse},
};
use axum::{
    body::Bytes,
    extract::{Extension, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use generated_contracts::PrincipalId;
use std::sync::Arc;

pub(crate) const MAX_IMAGE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) enum ProfileSection {
    Client,
    Worker,
}

impl ProfileSection {
    fn parse(value: &str) -> Result<Self, Error> {
        match value {
            "client" => Ok(Self::Client),
            "worker" => Ok(Self::Worker),
            _ => Err(Error::NotFound),
        }
    }

    pub(super) fn update_sql(self) -> &'static str {
        match self {
            Self::Client => {
                "UPDATE client_profiles SET image_data = $1, image_mime_type = $2 WHERE principal_id = $3"
            }
            Self::Worker => {
                "UPDATE worker_profiles SET image_data = $1, image_mime_type = $2 WHERE principal_id = $3"
            }
        }
    }

    pub(super) fn read_sql(self) -> &'static str {
        match self {
            Self::Client => {
                "SELECT image_data, image_mime_type FROM client_profiles WHERE principal_id = $1"
            }
            Self::Worker => {
                "SELECT image_data, image_mime_type FROM worker_profiles WHERE principal_id = $1"
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum ImageKind {
    Png,
    Jpeg,
    Webp,
}

impl ImageKind {
    pub(super) fn parse(value: &str) -> Result<Self, Error> {
        match value {
            "image/png" => Ok(Self::Png),
            "image/jpeg" => Ok(Self::Jpeg),
            "image/webp" => Ok(Self::Webp),
            _ => Err(Error::Invalid),
        }
    }

    pub(super) const fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }

    fn matches(self, bytes: &[u8]) -> bool {
        match self {
            Self::Png => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            Self::Jpeg => bytes.starts_with(b"\xff\xd8\xff") && bytes.ends_with(b"\xff\xd9"),
            Self::Webp => {
                bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP"
            }
        }
    }
}

pub(crate) async fn put_image(
    State(app): State<Arc<App>>,
    Extension(session): Extension<Session>,
    Path(section): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<StatusCode, Error> {
    let section = ProfileSection::parse(&section)?;
    let mime = headers
        .get(header::CONTENT_TYPE)
        .ok_or(Error::Invalid)?
        .to_str()
        .map_err(|_| Error::Invalid)?;
    let kind = ImageKind::parse(mime)?;
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES || !kind.matches(&bytes) {
        return Err(Error::Invalid);
    }
    store::write_image(&app.db, session.view.principal_id, section, kind, &bytes).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn get_image(
    State(app): State<Arc<App>>,
    Path((principal_id, section)): Path<(String, String)>,
) -> Result<Response, Error> {
    let id = parse::<PrincipalId>(&principal_id)?;
    let section = ProfileSection::parse(&section)?;
    let (kind, bytes) = store::read_image(&app.db, id, section).await?;
    let mut response = bytes.into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(kind.mime()));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}
