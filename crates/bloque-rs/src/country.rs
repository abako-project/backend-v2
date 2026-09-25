use serde::Serialize;

use crate::error::Error;

/// An ISO 3166-1 **alpha-3** country code (e.g. `"USA"`, `"COL"`) — the form
/// Bloque requires everywhere it asks for a country code. Sending the
/// 2-letter alpha-2 form (`"US"` instead of `"USA"`) is a well-documented,
/// easy-to-make integration mistake; this type makes it a construction-time
/// error instead of a rejected API call.
///
/// This validates *shape* only (exactly 3 ASCII letters, normalized to
/// uppercase), not that the code is a real, currently-assigned ISO entry —
/// embedding the full ISO-3166 table felt like the wrong tradeoff for a
/// small crate. Reject against a real table first if you need that
/// guarantee.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct CountryCode(String);

impl CountryCode {
    pub fn new(code: impl AsRef<str>) -> Result<Self, Error> {
        let code = code.as_ref();
        if code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic()) {
            Ok(Self(code.to_ascii_uppercase()))
        } else {
            Err(Error::Config(format!(
                "\"{code}\" is not a 3-letter ISO 3166-1 alpha-3 country code (e.g. \"USA\", \"COL\") — \
                 did you mean a 2-letter code?"
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CountryCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<&str> for CountryCode {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Error> {
        Self::new(value)
    }
}

impl std::str::FromStr for CountryCode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Self::new(s)
    }
}
