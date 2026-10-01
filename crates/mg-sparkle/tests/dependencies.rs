#[test]
fn enabled_rust_image_codecs_decode_and_other_codecs_remain_disabled() {
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
    // JPEG is deliberately enabled through the reviewed Rust decoder. Use a
    // uniform grayscale fixture so its lossy transform has an exact result.
    let jpeg_pixels = image::RgbImage::from_pixel(8, 8, image::Rgb([128, 128, 128]));
    let mut jpeg = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(jpeg_pixels.clone())
        .write_to(&mut jpeg, image::ImageFormat::Jpeg)
        .unwrap();
    assert_eq!(
        image::load_from_memory_with_format(jpeg.get_ref(), image::ImageFormat::Jpeg)
            .unwrap()
            .into_rgb8(),
        jpeg_pixels
    );
    assert!(matches!(
        image::load_from_memory_with_format(b"invalid JPEG", image::ImageFormat::Jpeg),
        Err(image::ImageError::Decoding(_))
    ));
    for format in [
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
