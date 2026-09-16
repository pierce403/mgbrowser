#[test]
fn png_and_gif_roundtrip_and_disabled_codecs() {
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
    // GIF is now deliberately enabled for the real HN transparent spacer.
    // Verify the allowed codec rather than weakening the unsupported-codec gate.
    let mut gif = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels.clone())
        .write_to(&mut gif, image::ImageFormat::Gif)
        .unwrap();
    assert_eq!(
        image::load_from_memory(gif.get_ref()).unwrap().into_rgba8(),
        pixels
    );
    for format in [
        image::ImageFormat::Jpeg,
        image::ImageFormat::WebP,
        image::ImageFormat::Tiff,
        image::ImageFormat::Avif,
        image::ImageFormat::Bmp,
        image::ImageFormat::Ico,
    ] {
        let error = image::load_from_memory_with_format(b"disabled codec", format).unwrap_err();
        assert!(
            matches!(error, image::ImageError::Unsupported(_)),
            "{format:?}"
        );
    }
}

#[test]
fn rust_font_apis_reject_invalid_fonts() {
    assert!(mg_sparkle::paint::Fonts::from_bytes(b"invalid".to_vec()).is_err());
    assert!(fontdue::Font::from_bytes(&b"invalid"[..], fontdue::FontSettings::default()).is_err());
    assert!(rustybuzz::Face::from_slice(b"invalid", 0).is_none());
}
