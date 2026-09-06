//! Undoing a library edit (ADR 0129).
//!
//! Develop has had `Ctrl+Z` since it existed, because a revision graph is an
//! undo history that was already there. The library had none: rating a
//! hundred photographs, tagging one, putting it in a collection or writing a
//! caption were all one-way.
//!
//! What makes them undoable is that each is a *replacement* of catalog state
//! that can be read back first. So an edit here is two snapshots of the same
//! shape — what the catalog held, and what it holds now — and undo and redo
//! are the same operation applied to different halves. Nothing in this
//! module knows about Slint, and nothing about it is asynchronous: the
//! snapshot is taken by the caller before it acts, on values it already has
//! in hand.

use leyline_sdk::{
    AssetDescription, AssetId, CollectionId, ColorLabel, KeywordId, Library, PickState, VersionId,
};

/// How many edits are kept. Beyond this the oldest is dropped.
///
/// Fifty because an undo stack is for the mistake one notices, and the
/// mistake one notices is the last handful of gestures; a stack deep enough
/// to hold an afternoon invites the belief that it holds the morning too.
const DEPTH: usize = 50;

/// A piece of catalog state, as it was or as it became.
///
/// Every variant is *idempotent to apply*: putting it back a second time
/// changes nothing, which is what lets undo and redo share one code path.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Snapshot {
    /// Rating, colour label and flag of a set of versions — the three move
    /// together because one keystroke can change any of them and the
    /// snapshot is taken from the same grid rows either way.
    Classement(Vec<(VersionId, Option<u8>, Option<ColorLabel>, PickState)>),
    /// Whether these assets carry this keyword.
    Keyword {
        assets: Vec<AssetId>,
        keyword: KeywordId,
        tagged: bool,
    },
    /// Whether these versions belong to this collection.
    Collection {
        collection: CollectionId,
        versions: Vec<VersionId>,
        member: bool,
    },
    /// What someone wrote about one photograph (ADR 0099).
    Description {
        asset: AssetId,
        description: Box<AssetDescription>,
    },
}

impl Snapshot {
    /// Puts this state back.
    pub(crate) fn apply(&self, library: &Library) -> Result<(), String> {
        match self {
            Self::Classement(rows) => {
                // One call per distinct value rather than one per row: a
                // hundred photographs rated in one gesture were rated the
                // same, so undoing them is three calls, not three hundred.
                for (version, rating, label, pick) in rows {
                    library
                        .set_rating(&[*version], *rating)
                        .and_then(|()| library.set_color_label(&[*version], *label))
                        .and_then(|()| library.set_pick(&[*version], *pick))
                        .map_err(|e| e.to_string())?;
                }
                Ok(())
            }
            Self::Keyword {
                assets,
                keyword,
                tagged,
            } => {
                if *tagged {
                    library.add_keyword(assets, *keyword)
                } else {
                    library.remove_keyword(assets, *keyword)
                }
            }
            .map_err(|e| e.to_string()),
            Self::Collection {
                collection,
                versions,
                member,
            } => {
                if *member {
                    library.add_to_collection(*collection, versions)
                } else {
                    library.remove_from_collection(*collection, versions)
                }
            }
            .map_err(|e| e.to_string()),
            Self::Description { asset, description } => library
                .set_description(*asset, description)
                .map_err(|e| e.to_string()),
        }
    }
}

/// One reversible edit: what it was called, and the state on either side.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Edit {
    /// One of a closed set of words the interface turns into a sentence —
    /// `"rating"`, `"label"`, `"flag"`, `"keyword"`, `"collection"`,
    /// `"description"`. A word and not a phrase because every string a user
    /// reads lives behind `@tr(...)` in a `.slint` and none is ever
    /// assembled in Rust (ADR 0078 §3).
    pub(crate) kind: &'static str,
    pub(crate) before: Snapshot,
    pub(crate) after: Snapshot,
}

/// The library's undo stack.
///
/// A cursor into a list rather than two stacks: `cursor` is how many of
/// `edits` are currently applied, so undo steps it back and redo steps it
/// forward, and a new edit made after an undo drops what was ahead of it —
/// the branch nobody can reach any more.
#[derive(Debug, Default)]
pub(crate) struct History {
    edits: Vec<Edit>,
    cursor: usize,
}

impl History {
    /// Records an edit that has just been performed.
    pub(crate) fn push(&mut self, edit: Edit) {
        self.edits.truncate(self.cursor);
        self.edits.push(edit);
        if self.edits.len() > DEPTH {
            self.edits.remove(0);
        }
        self.cursor = self.edits.len();
    }

    /// What `undo` would undo, and what `redo` would redo.
    pub(crate) fn undoable(&self) -> Option<&'static str> {
        self.cursor
            .checked_sub(1)
            .and_then(|i| self.edits.get(i))
            .map(|edit| edit.kind)
    }

    /// See [`History::undoable`].
    pub(crate) fn redoable(&self) -> Option<&'static str> {
        self.edits.get(self.cursor).map(|edit| edit.kind)
    }

    /// Steps back one edit, putting the catalog back as it was.
    ///
    /// The cursor moves only if the catalog write succeeded: a failure that
    /// left the stack pointing elsewhere would make the *next* undo restore
    /// a state that never existed.
    pub(crate) fn undo(&mut self, library: &Library) -> Result<bool, String> {
        let Some(index) = self.cursor.checked_sub(1) else {
            return Ok(false);
        };
        let Some(edit) = self.edits.get(index) else {
            return Ok(false);
        };
        edit.before.apply(library)?;
        self.cursor = index;
        Ok(true)
    }

    /// Steps forward one edit. See [`History::undo`].
    pub(crate) fn redo(&mut self, library: &Library) -> Result<bool, String> {
        let Some(edit) = self.edits.get(self.cursor) else {
            return Ok(false);
        };
        edit.after.apply(library)?;
        self.cursor += 1;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(kind: &'static str, tagged: bool) -> Edit {
        let snapshot = |tagged| Snapshot::Keyword {
            assets: Vec::new(),
            keyword: KeywordId::new(1),
            tagged,
        };
        Edit {
            kind,
            before: snapshot(!tagged),
            after: snapshot(tagged),
        }
    }

    #[test]
    fn nothing_to_undo_on_an_empty_history() {
        let history = History::default();
        assert_eq!(history.undoable(), None);
        assert_eq!(history.redoable(), None);
    }

    #[test]
    fn pushing_names_what_can_be_undone() {
        let mut history = History::default();
        history.push(edit("keyword", true));
        assert_eq!(history.undoable(), Some("keyword"));
        // Nothing has been undone, so there is nothing ahead to redo.
        assert_eq!(history.redoable(), None);
    }

    #[test]
    fn a_new_edit_drops_what_was_ahead() {
        let mut history = History::default();
        history.push(edit("rating", true));
        history.push(edit("label", true));
        history.cursor = 0;
        history.push(edit("flag", true));
        assert_eq!(history.edits.len(), 1);
        assert_eq!(history.undoable(), Some("flag"));
        assert_eq!(history.redoable(), None);
    }

    #[test]
    fn the_stack_is_bounded_and_keeps_the_newest() {
        let mut history = History::default();
        for _ in 0..DEPTH {
            history.push(edit("rating", true));
        }
        history.push(edit("description", true));
        assert_eq!(history.edits.len(), DEPTH);
        assert_eq!(history.undoable(), Some("description"));
    }
}
