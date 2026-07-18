//! Shared enumerations whose integer encodings are fixed by `docs/catalog.md`.

macro_rules! catalog_enum {
    (
        $(#[$doc:meta])* $name:ident {
            $($(#[$vdoc:meta])* $variant:ident = $value:literal),+ $(,)?
        }
    ) => {
        $(#[$doc])*
        #[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
        pub enum $name {
            $($(#[$vdoc])* $variant = $value),+
        }

        impl $name {
            /// Decodes the catalog integer encoding; `None` if unknown.
            pub const fn from_i64(value: i64) -> Option<Self> {
                match value {
                    $($value => Some(Self::$variant),)+
                    _ => None,
                }
            }

            /// Returns the catalog integer encoding.
            pub const fn as_i64(self) -> i64 {
                self as i64
            }
        }
    };
}

catalog_enum!(
    /// Kind of file an asset references (`docs/catalog.md` §10).
    MediaType {
        /// Camera RAW file.
        Raw = 0,
        /// JPEG file.
        Jpeg = 1,
        /// TIFF file.
        Tiff = 2,
        /// PNG file.
        Png = 3,
        /// DNG file.
        Dng = 4,
        /// HEIF file.
        Heif = 5,
        /// PSD file.
        Psd = 6,
        /// Any other supported file kind.
        Other = 7,
    }
);

catalog_enum!(
    /// Pick / reject flag carried by a develop version (`docs/catalog.md` §11).
    PickState {
        /// Not flagged.
        None = 0,
        /// Flagged as picked.
        Pick = 1,
        /// Flagged as rejected.
        Reject = 2,
    }
);

catalog_enum!(
    /// How a collection gets its members (`docs/catalog.md` §24).
    CollectionType {
        /// Explicit list of versions, ordered by the user.
        Manual = 0,
        /// Members generated from `rules_json`.
        Smart = 1,
    }
);

catalog_enum!(
    /// Color label carried by a develop version (`docs/catalog.md` §18).
    ColorLabel {
        /// Red label.
        Red = 0,
        /// Yellow label.
        Yellow = 1,
        /// Green label.
        Green = 2,
        /// Blue label.
        Blue = 3,
        /// Purple label.
        Purple = 4,
    }
);

catalog_enum!(
    /// Size class of a cached preview (`docs/catalog.md` §19).
    PreviewKind {
        /// Up to 256 px.
        Thumbnail = 0,
        /// Up to 1024 px.
        Small = 1,
        /// Up to 2048 px.
        Medium = 2,
        /// Up to 4096 px.
        Large = 3,
        /// Native resolution.
        Full = 4,
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_catalog_encoding() {
        for value in 0..8 {
            assert_eq!(MediaType::from_i64(value).unwrap().as_i64(), value);
        }
        assert_eq!(MediaType::from_i64(8), None);
        assert_eq!(PickState::from_i64(2), Some(PickState::Reject));
        assert_eq!(PreviewKind::Full.as_i64(), 4);
        assert_eq!(PreviewKind::from_i64(5), None);
    }
}
