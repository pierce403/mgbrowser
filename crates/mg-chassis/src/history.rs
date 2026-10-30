//! Bounded browsing history model and host-persistence requests.
//!
//! Chassis never touches the local filesystem; the platform host owns
//! `history.json` storage and synchronizes committed entries across tabs.

pub const MAX_HISTORY: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub timestamp_secs: u64,
    pub visits: u32,
}

impl HistoryEntry {
    pub fn new(url: &str, title: &str, timestamp_secs: u64, visits: u32) -> Result<Self, String> {
        if url.len() > 8191 || url.chars().any(char::is_control) {
            return Err("History URL is too long or contains control characters".into());
        }
        let parsed = url::Url::parse(url).map_err(|_| "Invalid history URL")?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.as_str().len() > 8191
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err("History requires a credential-free HTTP(S) URL".into());
        }
        let clean_title: String = title
            .chars()
            .filter(|c| !c.is_control())
            .take(128)
            .collect();
        let canonical = String::from(parsed);
        Ok(Self {
            title: if clean_title.trim().is_empty() {
                canonical.clone()
            } else {
                clean_title
            },
            url: canonical,
            timestamp_secs,
            visits: visits.max(1),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HistoryChange {
    Record {
        url: String,
        title: String,
        timestamp_secs: u64,
    },
    Remove(String),
    ClearAll,
}

/// Apply a `HistoryChange` to an in-memory most-recent-first history list.
pub fn apply_in_memory(entries: &mut Vec<HistoryEntry>, change: &HistoryChange) {
    match change {
        HistoryChange::Record {
            url,
            title,
            timestamp_secs,
        } => {
            let Ok(fresh) = HistoryEntry::new(url, title, *timestamp_secs, 1) else {
                return;
            };
            if let Some(pos) = entries.iter().position(|e| e.url == fresh.url) {
                let prev = entries.remove(pos);
                let updated = HistoryEntry {
                    url: fresh.url,
                    title: if title.trim().is_empty() {
                        prev.title
                    } else {
                        fresh.title
                    },
                    timestamp_secs: (*timestamp_secs).max(prev.timestamp_secs),
                    visits: prev.visits.saturating_add(1),
                };
                entries.insert(0, updated);
            } else {
                entries.insert(0, fresh);
                if entries.len() > MAX_HISTORY {
                    entries.truncate(MAX_HISTORY);
                }
            }
        }
        HistoryChange::Remove(url) => {
            entries.retain(|e| &e.url != url);
        }
        HistoryChange::ClearAll => {
            entries.clear();
        }
    }
}

impl crate::Browser {
    pub fn set_browsing_history(&mut self, entries: Vec<HistoryEntry>) {
        self.browsing_history = entries
            .into_iter()
            .take(MAX_HISTORY)
            .filter_map(|e| HistoryEntry::new(&e.url, &e.title, e.timestamp_secs, e.visits).ok())
            .collect();
        self.history_page = self
            .history_page
            .min(self.browsing_history.len().saturating_sub(1) / self.bookmark_rows());
        self.refresh_current_internal_page();
        self.dirty = true;
    }

    pub fn browsing_history(&self) -> &[HistoryEntry] {
        &self.browsing_history
    }

    pub fn take_history_changes(&mut self) -> Vec<HistoryChange> {
        std::mem::take(&mut self.history_changes)
    }

    pub fn take_clear_browsing_data(&mut self) -> bool {
        std::mem::take(&mut self.clear_browsing_data)
    }

    pub(crate) fn record_history_visit(&mut self, url: &str, title: &str) {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let change = HistoryChange::Record {
            url: url.to_owned(),
            title: title.to_owned(),
            timestamp_secs: now_secs,
        };
        if HistoryEntry::new(url, title, now_secs, 1).is_ok() {
            apply_in_memory(&mut self.browsing_history, &change);
            if self.history_changes.len() < MAX_HISTORY {
                self.history_changes.push(change);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_validates_credential_free_http_and_deduplicates_visits() {
        for bad in [
            "about:blank",
            "chrome://history",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://user:pw@example.com/",
        ] {
            assert!(HistoryEntry::new(bad, "Test", 100, 1).is_err());
        }
        let mut list = Vec::new();
        apply_in_memory(
            &mut list,
            &HistoryChange::Record {
                url: "https://example.com/a".into(),
                title: "Page A".into(),
                timestamp_secs: 1000,
            },
        );
        apply_in_memory(
            &mut list,
            &HistoryChange::Record {
                url: "https://example.com/b".into(),
                title: "Page B".into(),
                timestamp_secs: 1010,
            },
        );
        apply_in_memory(
            &mut list,
            &HistoryChange::Record {
                url: "https://example.com/a".into(),
                title: "Page A Updated".into(),
                timestamp_secs: 1020,
            },
        );
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].url, "https://example.com/a");
        assert_eq!(list[0].visits, 2);
        assert_eq!(list[0].title, "Page A Updated");
        assert_eq!(list[1].url, "https://example.com/b");

        apply_in_memory(
            &mut list,
            &HistoryChange::Remove("https://example.com/a".into()),
        );
        assert_eq!(list.len(), 1);
        apply_in_memory(&mut list, &HistoryChange::ClearAll);
        assert!(list.is_empty());
    }
}
