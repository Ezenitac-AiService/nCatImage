//! View-only overlay state. Never parses image data or interprets markup.
use ncat_protocol::MetadataEntry;

#[derive(Default)]
pub struct MetadataOverlay {
    pub enabled: bool,
    pub text: String,
}
impl MetadataOverlay {
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
    }
    pub fn visible(&self) -> bool {
        self.enabled && !self.text.is_empty()
    }
    pub fn clear(&mut self) {
        self.text = String::new();
    }
    pub fn update(&mut self, fields: &[MetadataEntry]) -> bool {
        let text = fields
            .iter()
            .map(|entry| format!("{}: {}", entry.name, entry.value))
            .collect::<Vec<_>>()
            .join("\n\n");
        if text == self.text {
            return false;
        }
        self.text = text;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn off_by_default_and_empty_pages_never_show_stale_text() {
        let mut state = MetadataOverlay::default();
        assert!(!state.visible());
        state.update(&[MetadataEntry {
            name: "Title".into(),
            value: "<b>plain</b>".into(),
        }]);
        assert!(!state.visible());
        state.toggle();
        assert!(state.visible());
        assert_eq!(state.text, "Title: <b>plain</b>");
        state.update(&[]);
        assert!(!state.visible());
        assert!(state.enabled);
        state.toggle();
        assert!(!state.enabled);
    }
}
