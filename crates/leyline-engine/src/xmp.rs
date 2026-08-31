//! XMP sidecars, both directions (`docs/catalog.md` §29).
//!
//! **Writing** is the *On Demand* synchronization: the caller decides when,
//! and the written tags are the interoperable core — rating and label of the
//! current version, keywords (flat and hierarchical), artist and copyright.
//!
//! **Reading** (ADR 0047) is not its symmetric counterpart, and the
//! asymmetry is the decision: a sidecar *seeds* a catalog row that has none,
//! it never synchronizes one that has. So there is no read equivalent of the
//! *Always* mode, no file watching, no reconciliation — and the catalog stays
//! the single source of truth (`docs/catalog.md` §2.4). What reading buys is
//! the one thing the write side could not: a photographer arriving from
//! another program with years of rating and keywording sitting next to their
//! RAW files.
//!
//! The field set is exactly the same in both directions, which makes the
//! round trip an invariant a test can hold (ADR 0047 §4). The *file name* is
//! not symmetric either: we write one convention and read two, because the
//! other programs do not agree on one (ADR 0047 §2.1).

use std::path::{Path, PathBuf};

use leyline_catalog::Catalog;
use leyline_core::{AssetId, ColorLabel, Result};

/// XMP namespace URIs. Prefixes are only a convention in XML — Lightroom's
/// `xmp:`, darktable's `xmp:`, someone else's `ns2:` all denote the same
/// namespace — so everything below matches on the URI (ADR 0047 §6).
const NS_XMP: &str = "http://ns.adobe.com/xap/1.0/";
const NS_DC: &str = "http://purl.org/dc/elements/1.1/";
const NS_LR: &str = "http://ns.adobe.com/lightroom/1.0/";

/// Writes (or rewrites) the XMP sidecar of an asset, next to its file with
/// the `.xmp` extension, and returns its path.
///
/// Sidecars are derived output: rewriting is the §29 synchronization, so an
/// existing file is replaced.
pub fn write_xmp_sidecar(
    catalog: &Catalog,
    library_root: &Path,
    asset: AssetId,
) -> Result<PathBuf> {
    let details = catalog.asset_details(asset)?;
    let current = details
        .versions
        .iter()
        .find(|v| v.version == details.current_version)
        .expect("the current version is always one of the asset's versions");

    // Resolve keyword ids to paths through the tree.
    let mut flat = Vec::new();
    let mut hierarchical = Vec::new();
    let mut stack = catalog.keyword_tree()?;
    while let Some(node) = stack.pop() {
        if details.keywords.contains(&node.keyword) {
            flat.push(node.name.clone());
            hierarchical.push(node.path.replace('/', "|"));
        }
        stack.extend(node.children);
    }
    flat.sort();
    hierarchical.sort();

    let mut description_attrs = String::new();
    if let Some(rating) = current.rating {
        description_attrs.push_str(&format!("\n   xmp:Rating=\"{rating}\""));
    }
    if let Some(label) = current.color_label {
        description_attrs.push_str(&format!("\n   xmp:Label=\"{}\"", label_name(label)));
    }

    let mut body = String::new();
    if !flat.is_empty() {
        body.push_str("   <dc:subject><rdf:Bag>\n");
        for name in &flat {
            body.push_str(&format!("    <rdf:li>{}</rdf:li>\n", escape(name)));
        }
        body.push_str("   </rdf:Bag></dc:subject>\n");
        body.push_str("   <lr:hierarchicalSubject><rdf:Bag>\n");
        for path in &hierarchical {
            body.push_str(&format!("    <rdf:li>{}</rdf:li>\n", escape(path)));
        }
        body.push_str("   </rdf:Bag></lr:hierarchicalSubject>\n");
    }
    // Authored description first, what the file said second (ADR 0099 §2):
    // a creator someone typed is what the sidecar should carry, and the
    // EXIF artist is the fallback rather than the rival.
    let written = catalog.description(asset)?.unwrap_or_default();
    let metadata = details.metadata.as_ref();
    let creator = written
        .creator
        .as_deref()
        .or_else(|| metadata.and_then(|m| m.artist.as_deref()));
    let rights = written
        .copyright
        .as_deref()
        .or_else(|| metadata.and_then(|m| m.copyright.as_deref()));
    if let Some(title) = &written.title {
        body.push_str(&format!(
            "   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:title>\n",
            escape(title)
        ));
    }
    if let Some(caption) = &written.caption {
        body.push_str(&format!(
            "   <dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:description>\n",
            escape(caption)
        ));
    }
    if let Some(creator) = creator {
        body.push_str(&format!(
            "   <dc:creator><rdf:Seq><rdf:li>{}</rdf:li></rdf:Seq></dc:creator>\n",
            escape(creator)
        ));
    }
    if let Some(rights) = rights {
        body.push_str(&format!(
            "   <dc:rights><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:rights>\n",
            escape(rights)
        ));
    }
    for (tag, value) in [
        ("Credit", &written.credit),
        ("City", &written.city),
        ("State", &written.state),
        ("Country", &written.country),
    ] {
        if let Some(value) = value {
            body.push_str(&format!(
                "   <photoshop:{tag}>{}</photoshop:{tag}>\n",
                escape(value)
            ));
        }
    }

    let document = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Leyline\">\n\
         \x20<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         \x20 <rdf:Description rdf:about=\"\"\n\
         \x20  xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\n\
         \x20  xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n\
         \x20  xmlns:lr=\"http://ns.adobe.com/lightroom/1.0/\"\n\
         \x20  xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\"{description_attrs}>\n\
         {body}\
         \x20 </rdf:Description>\n\
         \x20</rdf:RDF>\n\
         </x:xmpmeta>\n\
         <?xpacket end=\"w\"?>\n"
    );

    let file = library_root.join(
        details
            .relative_path
            .replace('/', std::path::MAIN_SEPARATOR_STR),
    );
    let sidecar = file.with_extension("xmp");
    std::fs::write(&sidecar, document)?;
    Ok(sidecar)
}

/// What a sidecar carries, once parsed — the interoperable core of
/// [`write_xmp_sidecar`], read back (ADR 0047 §4).
///
/// Every field is optional in the same sense: absent from the file means
/// "says nothing", never "clear this". That is what makes the fill-only
/// policy of [`apply_xmp_sidecar`] expressible at all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct XmpSidecar {
    /// `xmp:Rating`, kept only when it is a star rating this catalog accepts
    /// (1 to 5). Adobe also writes `-1` for "rejected" and `0` for
    /// "unrated"; neither is a rating here, so both are dropped.
    pub rating: Option<u8>,
    /// `xmp:Label`, matched against the five labels the catalog knows.
    pub color_label: Option<ColorLabel>,
    /// Keyword paths, slash-separated, deepest form available: from
    /// `lr:hierarchicalSubject` when present, otherwise the flat
    /// `dc:subject` (ADR 0047 §4).
    pub keywords: Vec<String>,
    /// `dc:creator`, first entry of the sequence.
    pub artist: Option<String>,
    /// `dc:rights`, the `x-default` alternative.
    pub copyright: Option<String>,
}

impl XmpSidecar {
    /// Whether this sidecar would set anything at all.
    fn is_empty(&self) -> bool {
        self.rating.is_none()
            && self.color_label.is_none()
            && self.keywords.is_empty()
            && self.artist.is_none()
            && self.copyright.is_none()
    }
}

/// Where Leyline *writes* the sidecar of a photo: the file's name with its
/// extension replaced by `.xmp` — Adobe's convention, the one Lightroom and
/// Bridge look for (ADR 0047 §2.1).
pub fn sidecar_path(file: &Path) -> PathBuf {
    file.with_extension("xmp")
}

/// Where Leyline *looks* for the sidecar of a photo, in the order it tries
/// them (ADR 0047 §2.1). Two conventions are in use in the wild and reading
/// only one of them loses the other program's work in silence:
///
/// 1. `photo.CR2.xmp` — the whole file name plus `.xmp`, written by darktable
///    and by exiftool. Tried first because it names *one* photo and cannot be
///    confused with a sibling's;
/// 2. `photo.xmp` — the extension replaced, written by Lightroom and by
///    Leyline itself. Shared by every file of that stem in the directory, so
///    it only answers once the unambiguous form has said nothing.
///
/// The two coincide for a file with no extension; the duplicate is harmless,
/// the first hit wins.
pub fn sidecar_candidates(file: &Path) -> [PathBuf; 2] {
    let mut appended = file.as_os_str().to_owned();
    appended.push(".xmp");
    [PathBuf::from(appended), sidecar_path(file)]
}

/// Parses the sidecar next to `file`, if there is one, under either naming
/// convention of [`sidecar_candidates`].
///
/// `None` covers everything that is not a usable sidecar — no file, an
/// unreadable one, malformed XML, or a well-formed document carrying none of
/// the fields above. None of those is an error the caller has to handle:
/// ADR 0047 §5 makes a bad sidecar the sidecar's problem, never the photo's.
/// A candidate that exists but says nothing does not stop the search either:
/// the next one still gets its turn.
pub fn read_xmp_sidecar(file: &Path) -> Option<XmpSidecar> {
    sidecar_candidates(file).into_iter().find_map(|candidate| {
        let text = std::fs::read_to_string(candidate).ok()?;
        let sidecar = parse_xmp(&text)?;
        (!sidecar.is_empty()).then_some(sidecar)
    })
}

/// Parses an XMP packet. `None` when the XML itself does not parse; an empty
/// [`XmpSidecar`] when it parses but says nothing we read.
fn parse_xmp(text: &str) -> Option<XmpSidecar> {
    let document = roxmltree::Document::parse(text).ok()?;
    let mut sidecar = XmpSidecar::default();

    for node in document.descendants().filter(|n| n.is_element()) {
        // Both spellings of a simple property: an attribute on some
        // rdf:Description, or an element with the value as its text. Real
        // files in the wild use either.
        if let Some(value) = node.attribute((NS_XMP, "Rating")) {
            sidecar.rating = sidecar.rating.or_else(|| parse_rating(value));
        }
        if let Some(value) = node.attribute((NS_XMP, "Label")) {
            sidecar.color_label = sidecar.color_label.or_else(|| parse_label(value));
        }
        let (namespace, name) = (node.tag_name().namespace(), node.tag_name().name());
        match (namespace, name) {
            (Some(NS_XMP), "Rating") => {
                sidecar.rating = sidecar.rating.or_else(|| parse_rating(&text_of(node)));
            }
            (Some(NS_XMP), "Label") => {
                sidecar.color_label = sidecar.color_label.or_else(|| parse_label(&text_of(node)));
            }
            // Hierarchical keywords win over the flat list when a file has
            // both, which Lightroom's own export does (ADR 0047 §4).
            (Some(NS_LR), "hierarchicalSubject") => {
                let paths: Vec<String> = list_items(node)
                    .map(|item| item.replace('|', "/"))
                    .collect();
                if !paths.is_empty() {
                    sidecar.keywords = paths;
                }
            }
            (Some(NS_DC), "subject") => {
                if sidecar.keywords.is_empty() {
                    sidecar.keywords = list_items(node).collect();
                }
            }
            (Some(NS_DC), "creator") => {
                sidecar.artist = sidecar.artist.clone().or_else(|| list_items(node).next());
            }
            (Some(NS_DC), "rights") => {
                sidecar.copyright = sidecar
                    .copyright
                    .clone()
                    .or_else(|| list_items(node).next());
            }
            _ => {}
        }
    }
    Some(sidecar)
}

/// The trimmed text of an element, empty when it has none.
fn text_of(node: roxmltree::Node<'_, '_>) -> String {
    node.text().unwrap_or_default().trim().to_owned()
}

/// The non-empty `rdf:li` values under an element, whichever RDF container
/// (`Bag`, `Seq`, `Alt`) wraps them — the distinction is about ordering and
/// language, neither of which changes what we read out.
fn list_items<'a>(node: roxmltree::Node<'a, 'a>) -> impl Iterator<Item = String> + 'a {
    node.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "li")
        .map(text_of)
        .filter(|value| !value.is_empty())
}

/// `xmp:Rating` as a star count this catalog accepts, or nothing.
fn parse_rating(value: &str) -> Option<u8> {
    let stars: i32 = value.trim().parse().ok()?;
    (1..=5).contains(&stars).then_some(stars as u8)
}

/// The inverse of [`label_name`], case-insensitive: the label is a free-text
/// XMP field, and other programs are not obliged to capitalize it the way we
/// write it.
fn parse_label(value: &str) -> Option<ColorLabel> {
    match value.trim().to_ascii_lowercase().as_str() {
        "red" => Some(ColorLabel::Red),
        "yellow" => Some(ColorLabel::Yellow),
        "green" => Some(ColorLabel::Green),
        "blue" => Some(ColorLabel::Blue),
        "purple" => Some(ColorLabel::Purple),
        _ => None,
    }
}

/// Applies a parsed sidecar to an asset under the fill-only policy of
/// ADR 0047 §3: rating, label, artist and copyright are written **only where
/// the catalog has nothing**, and keywords are a union. Nothing this function
/// does can remove or replace catalog data.
///
/// Returns whether anything was written.
pub fn apply_xmp_sidecar(
    catalog: &mut Catalog,
    asset: AssetId,
    sidecar: &XmpSidecar,
) -> Result<bool> {
    let details = catalog.asset_details(asset)?;
    let current = details
        .versions
        .iter()
        .find(|v| v.version == details.current_version)
        .expect("the current version is always one of the asset's versions");
    let mut wrote = false;

    if let Some(rating) = sidecar.rating
        && current.rating.is_none()
    {
        catalog.set_rating(&[details.current_version], Some(rating))?;
        wrote = true;
    }
    if let Some(label) = sidecar.color_label
        && current.color_label.is_none()
    {
        catalog.set_color_label(&[details.current_version], Some(label))?;
        wrote = true;
    }

    for path in &sidecar.keywords {
        let keyword = ensure_keyword_path(catalog, path)?;
        // `add_keyword` is idempotent, so the guard is not about correctness:
        // it is what lets the return value mean "something was actually
        // written", which a second read of the same sidecar must not claim.
        if !details.keywords.contains(&keyword) {
            catalog.add_keyword(&[asset], keyword)?;
            wrote = true;
        }
    }

    // Metadata is one row: read it, fill the two empty fields, write it back.
    // A sidecar must not be able to clear an EXIF value by not mentioning it.
    let mut metadata = details.metadata.unwrap_or_default();
    let mut metadata_changed = false;
    if let Some(artist) = &sidecar.artist
        && metadata.artist.is_none()
    {
        metadata.artist = Some(artist.clone());
        metadata_changed = true;
    }
    if let Some(copyright) = &sidecar.copyright
        && metadata.copyright.is_none()
    {
        metadata.copyright = Some(copyright.clone());
        metadata_changed = true;
    }
    if metadata_changed {
        catalog.set_metadata(asset, &metadata)?;
        wrote = true;
    }

    Ok(wrote)
}

/// Resolves a slash-separated keyword path to its id, creating the levels
/// that do not exist yet under the ones that do.
fn ensure_keyword_path(catalog: &mut Catalog, path: &str) -> Result<leyline_core::KeywordId> {
    let mut parent = None;
    let mut siblings = catalog.keyword_tree()?;
    for name in path.split('/').filter(|s| !s.trim().is_empty()) {
        let name = name.trim();
        match siblings.into_iter().find(|node| node.name == name) {
            Some(node) => {
                parent = Some(node.keyword);
                siblings = node.children;
            }
            None => {
                parent = Some(catalog.create_keyword(parent, name)?);
                siblings = Vec::new();
            }
        }
    }
    parent.ok_or_else(|| {
        leyline_core::LeylineError::InvalidSettings(format!("empty keyword path {path:?}"))
    })
}

/// Lightroom-compatible label names.
fn label_name(label: ColorLabel) -> &'static str {
    match label {
        ColorLabel::Red => "Red",
        ColorLabel::Yellow => "Yellow",
        ColorLabel::Green => "Green",
        ColorLabel::Blue => "Blue",
        ColorLabel::Purple => "Purple",
    }
}

/// Minimal XML text escaping.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
