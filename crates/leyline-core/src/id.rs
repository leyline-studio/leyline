//! Typed identifiers.
//!
//! Identifiers are dedicated types, never bare integers (`docs/engine-api.md` §2).
//! The raw value is private: crossing the API boundary always goes through
//! [`new`](AssetId::new) / [`get`](AssetId::get).

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident($raw:ty)) => {
        $(#[$doc])*
        #[derive(
            Copy,
            Clone,
            Eq,
            PartialEq,
            Ord,
            PartialOrd,
            Hash,
            Debug,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name($raw);

        impl $name {
            /// Wraps a raw identifier value.
            pub const fn new(raw: $raw) -> Self {
                Self(raw)
            }

            /// Returns the raw identifier value.
            pub const fn get(self) -> $raw {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_type!(
    /// Identifier of an asset (a physical file managed by Leyline).
    AssetId(i64)
);
id_type!(
    /// Identifier of a develop version (a named branch of revisions).
    VersionId(i64)
);
id_type!(
    /// Identifier of a develop revision (an immutable settings state).
    RevisionId(i64)
);
id_type!(
    /// Identifier of a folder in the library tree.
    FolderId(i64)
);
id_type!(
    /// Identifier of a collection (manual or smart).
    CollectionId(i64)
);
id_type!(
    /// Identifier of a hierarchical keyword.
    KeywordId(i64)
);
id_type!(
    /// Identifier of an export preset.
    ExportPresetId(i64)
);
id_type!(
    /// Identifier of a develop preset.
    PresetId(i64)
);
id_type!(
    /// Identifier of an asynchronous engine job.
    JobId(u64)
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_shows_raw_value() {
        assert_eq!(AssetId::new(42).to_string(), "42");
        assert_eq!(JobId::new(7).to_string(), "7");
    }

    #[test]
    fn round_trips_raw_value() {
        assert_eq!(RevisionId::new(-3).get(), -3);
    }

    #[test]
    fn serializes_transparently() {
        assert_eq!(serde_json::to_string(&VersionId::new(5)).unwrap(), "5");
        let id: VersionId = serde_json::from_str("5").unwrap();
        assert_eq!(id, VersionId::new(5));
    }
}
