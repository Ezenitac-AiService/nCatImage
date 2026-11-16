//! Toolkit-independent reader state and policies. No widgets, event loop or OS services.
#![forbid(unsafe_code)]

use ncat_core::{Direction, fit_size, physical_next};
pub mod input;
pub mod playback;

pub fn interface_shown(enabled: bool, fullscreen: bool) -> bool {
    enabled && !fullscreen
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageKey {
    Next,
    Previous,
    Left,
    Right,
}

pub fn page_step(key: PageKey, direction: Direction) -> bool {
    match key {
        PageKey::Next => true,
        PageKey::Previous => false,
        PageKey::Left | PageKey::Right => physical_next(direction, key == PageKey::Right),
    }
}

pub fn adjacent(selected: usize, len: usize, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(if forward {
        selected.saturating_add(1).min(len - 1)
    } else {
        selected.saturating_sub(1).min(len - 1)
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReaderView {
    pub zoom: f64,
    pub quarter_turns: u8,
    pub original: (u32, u32),
}

impl Default for ReaderView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            quarter_turns: 0,
            original: (1, 1),
        }
    }
}

impl ReaderView {
    pub fn change_zoom(&mut self, factor: f64) {
        if !factor.is_finite() || factor < 0.0 {
            return;
        }
        self.zoom = if factor == 0.0 {
            1.0
        } else {
            (self.zoom * factor).clamp(1.0, 16.0)
        };
    }

    pub fn rotate(&mut self, backwards: bool) {
        self.quarter_turns = (self.quarter_turns + if backwards { 3 } else { 1 }) % 4;
    }

    pub fn render_size(&self, viewport: (i32, i32), scale: f64, limit: u32) -> (u32, u32) {
        let dimension = |value: i32, minimum| {
            (f64::from(value.max(minimum)) * scale * self.zoom).clamp(1.0, f64::from(limit.max(1)))
                as u32
        };
        (dimension(viewport.0, 320), dimension(viewport.1, 240))
    }

    pub fn picture_size(&self, viewport: (i32, i32)) -> Option<(i32, i32)> {
        if self.zoom <= 1.0 {
            return None;
        }
        let (w, h) = fit_size(
            self.original.0.max(1),
            self.original.1.max(1),
            viewport.0.max(1) as u32,
            viewport.1.max(1) as u32,
        )
        .unwrap_or((1, 1));
        Some((
            (f64::from(w) * self.zoom) as i32,
            (f64::from(h) * self.zoom) as i32,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_only_changes_horizontal_input() {
        for direction in [Direction::LeftToRight, Direction::RightToLeft] {
            assert!(page_step(PageKey::Next, direction));
            assert!(!page_step(PageKey::Previous, direction));
        }
        assert!(page_step(PageKey::Right, Direction::LeftToRight));
        assert!(!page_step(PageKey::Right, Direction::RightToLeft));
    }

    #[test]
    fn navigation_is_bounded() {
        assert_eq!(adjacent(0, 0, true), None);
        assert_eq!(adjacent(0, 3, false), Some(0));
        assert_eq!(adjacent(2, 3, true), Some(2));
        assert_eq!(adjacent(0, 3, true), Some(1));
    }

    #[test]
    fn view_is_bounded_and_resettable() {
        let mut view = ReaderView::default();
        view.change_zoom(100.0);
        assert_eq!(view.zoom, 16.0);
        assert_eq!(view.render_size((1000, 800), 2.0, 8192), (8192, 8192));
        view.change_zoom(f64::NAN);
        assert_eq!(view.zoom, 16.0);
        view.change_zoom(0.0);
        assert_eq!(view.picture_size((1000, 800)), None);
        view.rotate(true);
        assert_eq!(view.quarter_turns, 3);
        view.rotate(false);
        assert_eq!(view.quarter_turns, 0);
    }

    #[test]
    fn fullscreen_preserves_windowed_preference() {
        assert!(!interface_shown(true, true));
        assert!(!interface_shown(false, true));
        assert!(interface_shown(true, false));
        assert!(!interface_shown(false, false));
    }
}
