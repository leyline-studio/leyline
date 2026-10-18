//! The XMP packet: every field of ADR 0157 §2, so a reader that only reads
//! XMP misses nothing the EXIF block says.
//!
//! Fields come in a fixed order and nothing in the packet depends on when it
//! was written: no `xmp:MetadataDate`, no `ModifyDate`, no instance or
//! document id (ADR 0157 §3).

use super::exif_block::offset_text;
use super::{
    CaptureTime, GpsPosition, PhotoMetadata, Ratio, SOFTWARE, altitude_centimetres,
    bias_hundredths, usable, usable_position,
};

/// The namespace JPEG's XMP APP1 segment opens with.
pub(crate) const JPEG_NAMESPACE: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

/// The PNG `iTXt` keyword the XMP specification assigns.
pub(crate) const PNG_KEYWORD: &str = "XML:com.adobe.xmp";

/// Escapes text for an XML element or attribute, and drops the control
/// characters XML 1.0 cannot carry at all — a caption pasted from elsewhere
/// can hold one, and a packet no reader can parse loses every other field.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// `YYYY-MM-DDTHH:MM:SS`, with the offset when it is known.
fn xmp_date(time: &CaptureTime) -> String {
    let offset = time.offset_minutes.map(offset_text).unwrap_or_default();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{offset}",
        time.year, time.month, time.day, time.hour, time.minute, time.second
    )
}

fn ratio(r: Ratio) -> String {
    format!("{}/{}", r.numerator, r.denominator)
}

/// XMP's GPS coordinate form, `DD,MM.mmmmmmR`. Counted in whole
/// millionths of a minute first, so a rounding cannot write 60 minutes.
fn coordinate(degrees: f64, positive: char, negative: char) -> String {
    let total = (degrees.abs() * 60.0 * 1e6).round() as u64;
    let direction = if degrees < 0.0 { negative } else { positive };
    format!(
        "{},{:02}.{:06}{direction}",
        total / 60_000_000,
        total / 1_000_000 % 60,
        total % 1_000_000
    )
}

fn simple(body: &mut String, name: &str, value: &str) {
    body.push_str(&format!("   <{name}>{}</{name}>\n", escape(value)));
}

fn alternative(body: &mut String, name: &str, value: &str) {
    body.push_str(&format!(
        "   <{name}><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></{name}>\n",
        escape(value)
    ));
}

fn list(body: &mut String, name: &str, kind: &str, items: &[String]) {
    body.push_str(&format!("   <{name}><rdf:{kind}>\n"));
    for item in items {
        body.push_str(&format!("    <rdf:li>{}</rdf:li>\n", escape(item)));
    }
    body.push_str(&format!("   </rdf:{kind}></{name}>\n"));
}

fn gps(body: &mut String, position: &GpsPosition) {
    simple(body, "exif:GPSVersionID", "2.3.0.0");
    simple(
        body,
        "exif:GPSLatitude",
        &coordinate(position.latitude, 'N', 'S'),
    );
    simple(
        body,
        "exif:GPSLongitude",
        &coordinate(position.longitude, 'E', 'W'),
    );
    if let Some((below, centimetres)) = altitude_centimetres(position.altitude) {
        simple(body, "exif:GPSAltitudeRef", if below { "1" } else { "0" });
        simple(body, "exif:GPSAltitude", &format!("{centimetres}/100"));
    }
}

/// The whole packet for `photo`.
pub(crate) fn xmp_packet(photo: &PhotoMetadata) -> String {
    let mut body = String::new();

    simple(&mut body, "xmp:CreatorTool", SOFTWARE);
    simple(&mut body, "tiff:Orientation", "1");
    if let Some(make) = &photo.make {
        simple(&mut body, "tiff:Make", make);
    }
    if let Some(model) = &photo.model {
        simple(&mut body, "tiff:Model", model);
    }
    if let Some(lens) = &photo.lens {
        simple(&mut body, "exifEX:LensModel", lens);
    }
    if let Some(time) = &photo.captured {
        let date = xmp_date(time);
        simple(&mut body, "exif:DateTimeOriginal", &date);
        simple(&mut body, "photoshop:DateCreated", &date);
    }
    if let Some(iso) = photo.iso {
        list(&mut body, "exif:ISOSpeedRatings", "Seq", &[iso.to_string()]);
    }
    if let Some(shutter) = usable(photo.shutter) {
        simple(&mut body, "exif:ExposureTime", &ratio(shutter));
    }
    if let Some(aperture) = usable(photo.aperture) {
        simple(&mut body, "exif:FNumber", &ratio(aperture));
    }
    if let Some(focal) = usable(photo.focal_length) {
        simple(&mut body, "exif:FocalLength", &ratio(focal));
    }
    if let Some(bias) = bias_hundredths(photo.exposure_bias) {
        simple(&mut body, "exif:ExposureBiasValue", &format!("{bias}/100"));
    }
    if let Some(fired) = photo.flash {
        let fired = if fired { "True" } else { "False" };
        body.push_str(&format!(
            "   <exif:Flash rdf:parseType=\"Resource\"><exif:Fired>{fired}</exif:Fired></exif:Flash>\n"
        ));
    }
    if let Some(position) = usable_position(photo.position) {
        gps(&mut body, &position);
    }

    if let Some(title) = &photo.title {
        alternative(&mut body, "dc:title", title);
    }
    if let Some(caption) = &photo.caption {
        alternative(&mut body, "dc:description", caption);
    }
    if let Some(creator) = &photo.creator {
        list(
            &mut body,
            "dc:creator",
            "Seq",
            std::slice::from_ref(creator),
        );
    }
    if let Some(copyright) = &photo.copyright {
        alternative(&mut body, "dc:rights", copyright);
    }
    if !photo.keywords.is_empty() {
        list(&mut body, "dc:subject", "Bag", &photo.keywords);
    }
    if !photo.keyword_paths.is_empty() {
        list(
            &mut body,
            "lr:hierarchicalSubject",
            "Bag",
            &photo.keyword_paths,
        );
    }
    for (tag, value) in [
        ("photoshop:Credit", &photo.credit),
        ("photoshop:City", &photo.city),
        ("photoshop:State", &photo.state),
        ("photoshop:Country", &photo.country),
    ] {
        if let Some(value) = value {
            simple(&mut body, tag, value);
        }
    }

    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Leyline\">\n\
         \x20<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         \x20 <rdf:Description rdf:about=\"\"\n\
         \x20  xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\n\
         \x20  xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\"\n\
         \x20  xmlns:exif=\"http://ns.adobe.com/exif/1.0/\"\n\
         \x20  xmlns:exifEX=\"http://cipa.jp/exif/1.0/\"\n\
         \x20  xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n\
         \x20  xmlns:lr=\"http://ns.adobe.com/lightroom/1.0/\"\n\
         \x20  xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\">\n\
         {body}\
         \x20 </rdf:Description>\n\
         \x20</rdf:RDF>\n\
         </x:xmpmeta>\n\
         <?xpacket end=\"r\"?>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_never_round_up_to_sixty_minutes() {
        assert_eq!(coordinate(-0.999_999_999_9, 'N', 'S'), "1,00.000000S");
        assert_eq!(coordinate(48.856_6, 'N', 'S'), "48,51.396000N");
    }

    #[test]
    fn control_characters_are_dropped_and_markup_escaped() {
        assert_eq!(
            escape("a\u{1}<b>&\"c\"\n"),
            "a&lt;b&gt;&amp;&quot;c&quot;\n"
        );
    }
}
