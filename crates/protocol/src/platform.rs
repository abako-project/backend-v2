//! Public deployment branding and vocabulary, independent of provider state.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::ContractError;

/// Maximum UTF-8 bytes in one platform string or project-type value.
pub const MAX_PLATFORM_TEXT_BYTES: usize = 4_000;

macro_rules! profile_dto {
    ($(#[$attr:meta])* $name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        $(#[$attr])*
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name { $(
            #[doc = concat!("Public profile field `", stringify!($field), "`.")]
            pub $field: $ty,
        )* }
    };
}

profile_dto!(/// One DAO's public presentation profile for this deployment.
PlatformProfile {
    id: String, name: String, tagline: String, description: String,
    sector: String, currency: PlatformCurrency, project_types: Vec<String>,
    terminology: PlatformTerminology,
});
profile_dto!(/// Asset display metadata; this object does not convert amounts.
    PlatformCurrency { symbol: String, name: String, decimals: u8 });
profile_dto!(/// Plain-text labels and hints used by domain-independent frontends.
PlatformTerminology {
    worker_role: String, worker_role_plural: String, client_role: String,
    coordinator_role: String, worker_pitch: String, client_pitch: String,
    external_profile: ExternalProfileHints, skills_helper_text: String,
    background_helper_text: String, empty_state_worker_text: String,
    empty_state_client_text: String,
});
profile_dto!(/// Presentation hints, without creating an external-identity binding.
ExternalProfileHints {
    id: String, label: String, placeholder: String, icon: String, help_text: String,
});

impl PlatformProfile {
    /// Validate all public text and the bounded, unique project-type list.
    pub fn validate(&self) -> Result<(), ContractError> {
        let terms = &self.terminology;
        let external = &terms.external_profile;
        for value in [
            &self.id,
            &self.name,
            &self.tagline,
            &self.description,
            &self.sector,
            &self.currency.symbol,
            &self.currency.name,
            &terms.worker_role,
            &terms.worker_role_plural,
            &terms.client_role,
            &terms.coordinator_role,
            &terms.worker_pitch,
            &terms.client_pitch,
            &terms.skills_helper_text,
            &terms.background_helper_text,
            &terms.empty_state_worker_text,
            &terms.empty_state_client_text,
            &external.id,
            &external.label,
            &external.placeholder,
            &external.icon,
            &external.help_text,
        ]
        .into_iter()
        .chain(&self.project_types)
        {
            if value.trim().is_empty()
                || value.len() > MAX_PLATFORM_TEXT_BYTES
                || value.contains('\0')
            {
                return Err(ContractError::Invalid("platform text"));
            }
        }
        if self.project_types.is_empty()
            || self.project_types.len() > 100
            || self.project_types.iter().collect::<BTreeSet<_>>().len() != self.project_types.len()
        {
            return Err(ContractError::Invalid("platform project types"));
        }
        Ok(())
    }
}
