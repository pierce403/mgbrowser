//! Bounded bookmark values and host-owned persistence requests.
pub const MAX_BOOKMARKS: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
}

impl Bookmark {
    pub fn new(url: &str, title: &str) -> Result<Self, String> {
        if url.len() > 8191 || url.chars().any(char::is_control) {
            return Err("Bookmark URL is too long or contains control characters".into());
        }
        let url = url::Url::parse(url).map_err(|_| "Invalid bookmark URL")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.as_str().len() > 8191
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("Bookmarks require an HTTP(S) URL without credentials".into());
        }
        let title: String = title
            .chars()
            .filter(|c| !c.is_control())
            .take(128)
            .collect();
        Ok(Self {
            title: if title.trim().is_empty() {
                url.to_string()
            } else {
                title
            },
            url: url.into(),
        })
    }
}

#[derive(Clone, Debug)]
pub enum BookmarkChange {
    Add(Bookmark),
    Remove(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_web_urls_only() {
        assert_eq!(
            Bookmark::new("HTTPS://EXAMPLE.COM", "").unwrap().url,
            "https://example.com/"
        );
        for url in [
            "about:blank",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://me:secret@example.com/",
            "https://example.com/\n",
        ] {
            assert!(Bookmark::new(url, "test").is_err());
        }
        assert_eq!(
            Bookmark::new("http://example.com/", &"é".repeat(200))
                .unwrap()
                .title
                .chars()
                .count(),
            128
        );
        assert_eq!(
            Bookmark::new("http://example.com/", "a\nb").unwrap().title,
            "ab"
        );
    }
}
