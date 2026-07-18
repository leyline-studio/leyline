//! XMP sidecar export (`docs/catalog.md` §29).
//!
//! The catalog is always the source of truth; sidecars exist purely for
//! interoperability with other tools and are never read back. This is the
//! *On Demand* mode: the caller decides when to synchronize. The written
//! tags are the interoperable core — rating and label of the current
//! version, keywords (flat and hierarchical), artist and copyright.

use std::path::{Path, PathBuf};

use leyline_catalog::Catalog;
use leyline_core::{AssetId, ColorLabel, Result};

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
    if let Some(metadata) = &details.metadata {
        if let Some(artist) = &metadata.artist {
            body.push_str(&format!(
                "   <dc:creator><rdf:Seq><rdf:li>{}</rdf:li></rdf:Seq></dc:creator>\n",
                escape(artist)
            ));
        }
        if let Some(rights) = &metadata.copyright {
            body.push_str(&format!(
                "   <dc:rights><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:rights>\n",
                escape(rights)
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
         \x20  xmlns:lr=\"http://ns.adobe.com/lightroom/1.0/\"{description_attrs}>\n\
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
