use super::*;
use leyline_sdk::Rgb8;

/// A 4x2 image whose pixels are all different, so a wrong index is
/// visible rather than plausible.
fn image() -> Rgb8 {
    let mut data = Vec::new();
    for i in 0..8u8 {
        data.extend_from_slice(&[i * 10, i * 10 + 1, i * 10 + 2]);
    }
    Rgb8::new(4, 2, data).unwrap()
}

#[test]
fn the_readout_names_the_pixel_under_the_pointer() {
    let image = image();
    assert_eq!(
        pixel_readout(&image, 0.0, 0.0).as_deref(),
        Some("R 0  G 1  B 2")
    );
    // Second column of the second row: index 5.
    assert_eq!(
        pixel_readout(&image, 0.3, 0.6).as_deref(),
        Some("R 50  G 51  B 52")
    );
}

#[test]
fn the_far_edge_reads_the_last_pixel_rather_than_running_off_it() {
    assert_eq!(
        pixel_readout(&image(), 1.0, 1.0).as_deref(),
        Some("R 70  G 71  B 72")
    );
}

#[test]
fn off_the_photograph_is_a_readout_of_nothing() {
    let image = image();
    assert_eq!(pixel_readout(&image, -0.01, 0.5), None);
    assert_eq!(pixel_readout(&image, 0.5, 1.2), None);
}
