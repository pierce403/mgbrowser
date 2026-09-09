#[test]
fn png_roundtrip_and_disabled_codec() {
    let pixels = image::RgbaImage::from_pixel(2, 1, image::Rgba([12, 34, 56, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels.clone())
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    assert_eq!(
        image::load_from_memory(bytes.get_ref())
            .unwrap()
            .into_rgba8(),
        pixels
    );
    let error =
        image::load_from_memory_with_format(b"GIF89a", image::ImageFormat::Gif).unwrap_err();
    assert!(matches!(error, image::ImageError::Unsupported(_)));
}

#[test]
fn rust_font_apis_reject_invalid_fonts() {
    assert!(mg_sparkle::paint::Fonts::from_bytes(b"invalid".to_vec()).is_err());
    assert!(fontdue::Font::from_bytes(&b"invalid"[..], fontdue::FontSettings::default()).is_err());
    assert!(rustybuzz::Face::from_slice(b"invalid", 0).is_none());
}
