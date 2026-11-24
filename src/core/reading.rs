//! Reading modes for comics and webtoons: two pages side by side, or
//! images stacked into one long strip.

use eframe::egui::{Rect, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ReadingMode {
    #[default]
    Single,
    /// Two pages, first on the left (Western books).
    TwoLtr,
    /// Two pages, first on the right (Japanese manga).
    TwoRtl,
    /// One vertical strip, scrolled continuously.
    Webtoon,
}

impl ReadingMode {
    /// `D`: single → left-to-right → right-to-left → single.
    pub fn next_two_page(self) -> Self {
        match self {
            ReadingMode::Single | ReadingMode::Webtoon => ReadingMode::TwoLtr,
            ReadingMode::TwoLtr => ReadingMode::TwoRtl,
            ReadingMode::TwoRtl => ReadingMode::Single,
        }
    }

    pub fn is_two_page(self) -> bool {
        matches!(self, ReadingMode::TwoLtr | ReadingMode::TwoRtl)
    }

    pub fn label(self) -> &'static str {
        match self {
            ReadingMode::Single => "SINGLE PAGE",
            ReadingMode::TwoLtr => "TWO PAGES · LEFT TO RIGHT",
            ReadingMode::TwoRtl => "TWO PAGES · RIGHT TO LEFT",
            ReadingMode::Webtoon => "WEBTOON",
        }
    }
}

/// Where two pages go: same height, side by side, as large as fits,
/// centred. `first`/`second` are image sizes; returns (first, second) rects.
pub fn spread(canvas: Rect, first: Vec2, second: Vec2, rtl: bool) -> (Rect, Rect) {
    let ra = first.x / first.y.max(1.0);
    let rb = second.x / second.y.max(1.0);
    let h = canvas.height().min(canvas.width() / (ra + rb));
    let (wa, wb) = (h * ra, h * rb);
    let left = canvas.center().x - (wa + wb) / 2.0;
    let top = canvas.center().y - h / 2.0;
    let (lw, rw) = if rtl { (wb, wa) } else { (wa, wb) };
    let l = Rect::from_min_size(eframe::egui::pos2(left, top), Vec2::new(lw, h));
    let r = Rect::from_min_size(eframe::egui::pos2(left + lw, top), Vec2::new(rw, h));
    if rtl {
        (r, l)
    } else {
        (l, r)
    }
}

/// Scrolls a webtoon strip by `dy` points. `offset` is how far the current
/// image's top is above the canvas top; `heights(i)` gives image `i`'s drawn
/// height. Returns the new (index, offset), moving to the next/previous
/// image when its edge is crossed and stopping at both ends.
pub fn scroll_strip(
    index: usize,
    offset: f32,
    dy: f32,
    len: usize,
    view_h: f32,
    heights: impl Fn(usize) -> f32,
) -> (usize, f32) {
    let (mut i, mut off) = (index, offset + dy);
    while off < 0.0 && i > 0 {
        i -= 1;
        off += heights(i);
    }
    while i + 1 < len && off >= heights(i) {
        off -= heights(i);
        i += 1;
    }
    if i == 0 {
        off = off.max(0.0);
    }
    // At the end, stop when the last image's bottom reaches the canvas bottom.
    if i + 1 == len {
        off = off.min((heights(i) - view_h).max(0.0));
    }
    (i, off)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    #[test]
    fn modes_cycle() {
        let m = ReadingMode::Single;
        assert_eq!(m.next_two_page().next_two_page().next_two_page(), m);
        assert!(ReadingMode::TwoRtl.is_two_page());
        assert_eq!(ReadingMode::Webtoon.next_two_page(), ReadingMode::TwoLtr);
    }

    #[test]
    fn pages_share_height_and_fit() {
        let canvas = Rect::from_min_size(pos2(0.0, 0.0), Vec2::new(1000.0, 800.0));
        let page = Vec2::new(600.0, 900.0);
        let (a, b) = spread(canvas, page, page, false);
        assert_eq!(a.height(), b.height());
        assert!(a.right() <= b.left() + 0.01);
        assert!(a.width() + b.width() <= 1000.01);
        assert!((a.height() - 750.0).abs() < 0.01, "{a:?}"); // width-limited
                                                             // Right to left: the first page is on the right.
        let (a2, b2) = spread(canvas, page, page, true);
        assert!(a2.left() > b2.left());
    }

    #[test]
    fn strip_scrolls_across_images_and_stops_at_ends() {
        let h = |i: usize| [1000.0, 500.0, 2000.0][i];
        // Down past the first image into the second.
        assert_eq!(scroll_strip(0, 900.0, 200.0, 3, 600.0, h), (1, 100.0));
        // Up back into the first.
        assert_eq!(scroll_strip(1, 100.0, -300.0, 3, 600.0, h), (0, 800.0));
        // Cannot go above the top.
        assert_eq!(scroll_strip(0, 10.0, -100.0, 3, 600.0, h), (0, 0.0));
        // Stops when the last image's bottom meets the canvas bottom.
        assert_eq!(scroll_strip(2, 1000.0, 5000.0, 3, 600.0, h), (2, 1400.0));
        // A big jump crosses several images.
        assert_eq!(scroll_strip(0, 0.0, 1600.0, 3, 600.0, h), (2, 100.0));
    }
}
