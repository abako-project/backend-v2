/// Define a string-backed enum for an API field that has known values today
/// but may grow new ones server-side before this crate is updated. Adds an
/// `Other(String)` catch-all so deserializing an unrecognized value is a
/// usable variant instead of a hard failure, and so a caller can still send
/// a value this crate doesn't have a name for yet.
///
/// Generates `as_str`, `Display`, `From<&str>`, and manual `Serialize`/
/// `Deserialize` (as the plain wire string, not `{"Variant": ...}`).
///
/// Not used for fields that are genuinely closed (`"src" | "dst"`, sort
/// direction, ...) — those are hand-written plain enums instead, so the
/// compiler enforces exhaustiveness instead of silently accepting typos.
macro_rules! wire_string_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $wire:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )+
            /// Any value this crate doesn't have a named variant for yet —
            /// keeps unknown-but-valid API values usable instead of erroring.
            Other(String),
        }

        impl $name {
            pub fn as_str(&self) -> &str {
                match self {
                    $( $name::$variant => $wire, )+
                    $name::Other(s) => s.as_str(),
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                match s {
                    $( $wire => $name::$variant, )+
                    other => $name::Other(other.to_string()),
                }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> ::std::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(
                deserializer: D,
            ) -> ::std::result::Result<Self, D::Error> {
                let s = <String as serde::Deserialize>::deserialize(deserializer)?;
                Ok(Self::from(s.as_str()))
            }
        }
    };
}

pub(crate) use wire_string_enum;

/// Percent-encode and join `key=value` pairs for a query string.
pub(crate) fn build_query(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                url::form_urlencoded::byte_serialize(k.as_bytes()).collect::<String>(),
                url::form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}
