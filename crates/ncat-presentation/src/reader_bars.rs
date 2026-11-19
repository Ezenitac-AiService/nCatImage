//! Shared edge reveal and pin policy. Bars never participate in image layout.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bar {
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReaderBars {
    pinned: [bool; 2],
    hovered: Option<Bar>,
}

impl Default for ReaderBars {
    fn default() -> Self {
        Self {
            pinned: [true; 2],
            hovered: None,
        }
    }
}

impl ReaderBars {
    pub fn pinned(self, bar: Bar) -> bool {
        self.pinned[bar as usize]
    }

    pub fn visible(self, bar: Bar, editing: Option<Bar>) -> bool {
        self.pinned(bar) || self.hovered == Some(bar) || editing == Some(bar)
    }

    pub fn toggle_pin(&mut self, bar: Bar) {
        self.pinned[bar as usize] = !self.pinned(bar);
    }

    pub fn toggle_both(&mut self) {
        self.pinned = [!self.pinned.iter().any(|&pin| pin); 2];
    }

    pub fn point(&mut self, y: Option<i32>, height: i32, edge: i32, top: i32, bottom: i32) {
        self.hovered = y.filter(|&y| (0..height).contains(&y)).and_then(|y| {
            let (bar, distance, band) = if y < height / 2 {
                (Bar::Top, y, top)
            } else {
                (Bar::Bottom, height - 1 - y, bottom)
            };
            (distance < edge
                || ((self.pinned(bar) || self.hovered == Some(bar)) && distance < band))
                .then_some(bar)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_edge_reveal_and_editing_are_independent() {
        let mut bars = ReaderBars::default();
        bars.toggle_both();
        bars.point(Some(25), 300, 12, 75, 115);
        assert!(!bars.visible(Bar::Top, None));
        bars.point(Some(4), 300, 12, 75, 115);
        bars.point(Some(65), 300, 12, 75, 115);
        assert!(bars.visible(Bar::Top, None));
        assert!(!bars.visible(Bar::Bottom, None));
        bars.point(Some(295), 300, 12, 75, 115);
        assert!(!bars.visible(Bar::Top, None));
        assert!(bars.visible(Bar::Bottom, None));
        bars.toggle_pin(Bar::Top);
        bars.point(None, 300, 12, 75, 115);
        assert!(bars.visible(Bar::Top, None));
        assert!(!bars.visible(Bar::Bottom, None));
        assert!(bars.visible(Bar::Bottom, Some(Bar::Bottom)));
        bars.toggle_both();
        assert!(!bars.pinned(Bar::Top));
        bars.toggle_both();
        assert!(bars.pinned(Bar::Top) && bars.pinned(Bar::Bottom));
    }
}
