//! Dependency foundation for an experimental browser, not a browser implementation.
//! TLS uses the selected RustCrypto provider explicitly, without global fallback.

pub mod document;
pub mod net;
pub mod paint;

/// Build a research TLS client with the caller's trusted roots.
/// Certificate and hostname verification remain rustls's normal defaults.
pub fn tls_client_config(
    roots: rustls::RootCertStore,
) -> Result<rustls::ClientConfig, rustls::Error> {
    Ok(
        rustls::ClientConfig::builder_with_provider(rustls_rustcrypto::provider().into())
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    #[test]
    fn selected_provider_builds_a_client() {
        let config = super::tls_client_config(rustls::RootCertStore::empty()).unwrap();
        let name = "localhost".try_into().unwrap();
        let client = rustls::ClientConnection::new(config.into(), name).unwrap();
        assert!(client.wants_write());
    }

    #[test]
    fn png_roundtrip_and_disabled_codec() {
        let pixels = image::RgbaImage::from_pixel(2, 1, image::Rgba([12, 34, 56, 255]));
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels.clone())
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let decoded = image::load_from_memory(bytes.get_ref())
            .unwrap()
            .into_rgba8();
        assert_eq!(decoded, pixels);
        // A recognized format must not acquire an implicit native fallback.
        let error =
            image::load_from_memory_with_format(b"GIF89a", image::ImageFormat::Gif).unwrap_err();
        assert!(matches!(error, image::ImageError::Unsupported(_)));
    }

    #[test]
    fn rust_font_apis_reject_invalid_fonts() {
        assert!(
            fontdue::Font::from_bytes(&b"invalid"[..], fontdue::FontSettings::default()).is_err()
        );
        assert!(rustybuzz::Face::from_slice(b"invalid", 0).is_none());
    }
}
