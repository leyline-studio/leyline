//! Authored descriptions (`docs/catalog.md` §2.4, ADR 0099).
//!
//! What a photographer *writes* about a photograph — a title, a caption, a
//! copyright line — as opposed to what its file says, which lives in
//! `metadata` and is replaced wholesale every time the file is read again.
//!
//! Keeping the two apart is the whole decision (ADR 0099 §1): nothing that
//! reads a file writes this table, so a re-import, an EXIF re-read or a
//! reprocessing cannot destroy authored text. That is a property of the
//! schema rather than of anyone's care.

use leyline_core::{AssetId, LeylineError, Result};
use rusqlite::OptionalExtension;

use crate::{Catalog, db_err};

/// What someone wrote about a photograph (ADR 0099 §1).
///
/// Every field optional, and an all-empty description is the absence of a
/// row rather than a row of nulls — the same "neutral means absent"
/// convention `Settings` follows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetDescription {
    /// Short name of the photograph (`dc:title`).
    pub title: Option<String>,
    /// Longer description (`dc:description`).
    pub caption: Option<String>,
    /// Who made it (`dc:creator`) — authored, unlike `metadata.artist`,
    /// which is what the file's EXIF said.
    pub creator: Option<String>,
    /// Rights statement (`dc:rights`), authored like `creator`.
    pub copyright: Option<String>,
    /// Who should be credited when it is published (`photoshop:Credit`).
    pub credit: Option<String>,
    /// Where it was taken: city (`photoshop:City`).
    pub city: Option<String>,
    /// Where it was taken: state or province (`photoshop:State`).
    pub state: Option<String>,
    /// Where it was taken: country (`photoshop:Country`).
    pub country: Option<String>,
}

impl AssetDescription {
    /// Whether this description says anything at all — what decides
    /// between writing a row and deleting one.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields().all(|(_, value)| value.is_none())
    }

    /// The eight fields, paired with their column names, in one place so a
    /// reader, a writer and a merge cannot disagree about the list.
    fn fields(&self) -> impl Iterator<Item = (&'static str, Option<&str>)> {
        [
            ("title", self.title.as_deref()),
            ("caption", self.caption.as_deref()),
            ("creator", self.creator.as_deref()),
            ("copyright", self.copyright.as_deref()),
            ("credit", self.credit.as_deref()),
            ("city", self.city.as_deref()),
            ("state", self.state.as_deref()),
            ("country", self.country.as_deref()),
        ]
        .into_iter()
    }

    /// `self`, with every field `other` sets overriding it — how an import
    /// template lands on an asset (ADR 0099 §4) without clearing what it
    /// says nothing about.
    #[must_use]
    pub fn overlaid_with(&self, other: &AssetDescription) -> AssetDescription {
        let pick = |mine: &Option<String>, theirs: &Option<String>| {
            theirs.clone().or_else(|| mine.clone())
        };
        AssetDescription {
            title: pick(&self.title, &other.title),
            caption: pick(&self.caption, &other.caption),
            creator: pick(&self.creator, &other.creator),
            copyright: pick(&self.copyright, &other.copyright),
            credit: pick(&self.credit, &other.credit),
            city: pick(&self.city, &other.city),
            state: pick(&self.state, &other.state),
            country: pick(&self.country, &other.country),
        }
    }
}

impl Catalog {
    /// Records what someone wrote about an asset, replacing any previous
    /// description. An empty one deletes the row.
    ///
    /// Never called by anything that reads a file: that is the invariant
    /// ADR 0099 §1 rests on.
    pub fn set_description(
        &mut self,
        asset: AssetId,
        description: &AssetDescription,
    ) -> Result<()> {
        self.ensure_writable()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        tx.query_row("SELECT 1 FROM assets WHERE id = ?1", [asset.get()], |_| {
            Ok(())
        })
        .optional()
        .map_err(db_err)?
        .ok_or(LeylineError::AssetMissing(asset))?;

        if description.is_empty() {
            tx.execute(
                "DELETE FROM asset_descriptions WHERE asset_id = ?1",
                [asset.get()],
            )
            .map_err(db_err)?;
        } else {
            tx.execute(
                "INSERT INTO asset_descriptions
                     (asset_id, title, caption, creator, copyright, credit, city, state, country)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(asset_id) DO UPDATE SET
                     title = excluded.title, caption = excluded.caption,
                     creator = excluded.creator, copyright = excluded.copyright,
                     credit = excluded.credit, city = excluded.city,
                     state = excluded.state, country = excluded.country",
                rusqlite::params![
                    asset.get(),
                    description.title,
                    description.caption,
                    description.creator,
                    description.copyright,
                    description.credit,
                    description.city,
                    description.state,
                    description.country,
                ],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        // Authored creator and copyright take precedence in search
        // (ADR 0099 §2), so this asset's index row follows the write.
        crate::search::refresh_asset_authorship(&self.conn, asset)?;
        Ok(())
    }

    /// What someone wrote about an asset, if anyone has.
    pub fn description(&self, asset: AssetId) -> Result<Option<AssetDescription>> {
        self.conn
            .query_row(
                "SELECT title, caption, creator, copyright, credit, city, state, country
                 FROM asset_descriptions WHERE asset_id = ?1",
                [asset.get()],
                |row| {
                    Ok(AssetDescription {
                        title: row.get(0)?,
                        caption: row.get(1)?,
                        creator: row.get(2)?,
                        copyright: row.get(3)?,
                        credit: row.get(4)?,
                        city: row.get(5)?,
                        state: row.get(6)?,
                        country: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(db_err)
    }
}
