//! The panels this instrument draws on, and the grid they share.
//!
//! Two boards carry the same instrument. The E213 is 250×122; the E290 is
//! 296×128 — eighteen per cent wider and five per cent taller, at close enough
//! to the same pixel pitch that a glyph is the same size in the hand on both.
//! So the larger panel is not a scale factor and nothing is drawn bigger on it.
//! What it buys is room: sentences that used to reach the truncation mark say
//! the rest of themselves, and the band a glance lands on gets the extra rows.
//!
//! Every position either renderer uses comes from [`PanelGrid`], derived from
//! the panel's own dimensions. The rail is anchored to the right edge and the
//! tape to the bottom edge, because those are the edges they belong to — an
//! instrument whose furniture is nailed to coordinates that only suit one panel
//! is two layouts wearing one name, and they drift.

use serde::{Deserialize, Serialize};

/// Width of the registration rail, which is the same on both panels.
///
/// The rail holds a two-character code, a ladder, and a hatch — content of a
/// fixed size drawn at a fixed size. Widening it on the larger panel would
/// stretch a ruler and take the width from the words that wanted it.
pub(crate) const RAIL_WIDTH: u32 = 18;
/// Height of the bottom tape, likewise fixed: it carries one line of the label
/// face on both panels.
pub(crate) const TAPE_HEIGHT: u32 = 14;
/// Left margin for everything that is not a full-bleed band.
pub(crate) const MARGIN_LEFT: i32 = 4;
/// Gap between the content area and the rail.
const RAIL_GUTTER: i32 = 4;

/// A physical e-paper panel this instrument knows how to draw.
///
/// The board reports which one it is; nobody is asked to choose. Adding a third
/// means adding a variant here and the geometry it draws at, not a second
/// layout.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelModel {
    /// Heltec Vision Master E213, 250×122.
    #[default]
    E213,
    /// Heltec Vision Master E290, 296×128.
    E290,
}

impl PanelModel {
    /// Every panel the host can render for, in the order a listing should show.
    pub const ALL: [Self; 2] = [Self::E213, Self::E290];

    /// Panel width in pixels.
    pub const fn width(self) -> u16 {
        match self {
            Self::E213 => 250,
            Self::E290 => 296,
        }
    }

    /// Panel height in pixels.
    pub const fn height(self) -> u16 {
        match self {
            Self::E213 => 122,
            Self::E290 => 128,
        }
    }

    /// Packed bytes per row, including the white padding bits that finish the
    /// last byte.
    pub const fn stride(self) -> usize {
        (self.width() as usize).div_ceil(8)
    }

    /// Size of one packed framebuffer for this panel.
    pub const fn payload_size(self) -> usize {
        self.stride() * self.height() as usize
    }

    /// Short name as the firmware banner and the interface both spell it.
    pub const fn label(self) -> &'static str {
        match self {
            Self::E213 => "E213",
            Self::E290 => "E290",
        }
    }

    /// The panel with these dimensions, if it is one this host can draw.
    ///
    /// This is how a board's reported geometry becomes a panel: the firmware
    /// announces `250x122` or `296x128` and the host renders for whichever came
    /// back. Dimensions that match nothing are refused rather than rounded to
    /// the nearest panel, because a frame drawn for the wrong geometry is not a
    /// slightly wrong picture — it is a diagonal smear.
    pub fn from_dimensions(width: u16, height: u16) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|panel| panel.width() == width && panel.height() == height)
    }

    /// The panel a board announcing this label is carrying.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|panel| panel.label().eq_ignore_ascii_case(label.trim()))
    }

    /// The grid this panel is drawn on.
    pub(crate) const fn grid(self) -> PanelGrid {
        PanelGrid::of(self)
    }
}

/// Where the furniture sits on a given panel.
///
/// One instrument, two panels: the values below are the same measurements on
/// both, taken from the edges that own them. `E213` numbers are what the panel
/// was hand-tuned to before there was a second one, so a derivation that does
/// not reproduce them exactly is a derivation that moved the shipped panel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PanelGrid {
    pub(crate) model: PanelModel,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

impl PanelGrid {
    pub(crate) const fn of(model: PanelModel) -> Self {
        Self {
            model,
            width: model.width(),
            height: model.height(),
        }
    }

    /// Left edge of the rail, and so the right edge of everything else.
    pub(crate) const fn rail_left(self) -> i32 {
        self.width as i32 - RAIL_WIDTH as i32
    }

    /// Drawing width available to panel content, which is where a full-bleed
    /// band stops.
    pub(crate) const fn content_width(self) -> u32 {
        self.rail_left() as u32
    }

    /// Right edge of content that must stay clear of the rail.
    pub(crate) const fn content_right(self) -> i32 {
        self.rail_left() - RAIL_GUTTER
    }

    /// Top of the bottom tape, anchored to the panel's bottom edge.
    pub(crate) const fn tape_top(self) -> i32 {
        self.height as i32 - TAPE_HEIGHT as i32
    }

    /// Rows the panel has beyond the smaller one.
    ///
    /// Both renderers spend this in one place rather than sprinkling it: the
    /// band a glance lands on first. Six rows shared out between five rows of
    /// text is invisible padding; six rows given to the state field is a field
    /// that reads as taller from across a room.
    pub(crate) const fn extra_rows(self) -> i32 {
        self.height as i32 - PanelModel::E213.height() as i32
    }

    /// Characters of a face with `glyph_width` advance that fit between two x
    /// positions, leaving the four pixels of air that keep text off a rule.
    pub(crate) fn characters_between(self, left: i32, right: i32, glyph_width: i32) -> usize {
        usize::try_from(((right - left - 4).max(glyph_width)) / glyph_width).unwrap_or(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The E213 is in service. Its grid must come out of the derivation exactly
    /// as it was hand-tuned, or this refactor silently moved a shipped panel.
    #[test]
    fn the_e213_grid_is_the_one_that_was_hand_tuned() {
        let grid = PanelModel::E213.grid();
        assert_eq!(grid.rail_left(), 232);
        assert_eq!(grid.content_width(), 232);
        assert_eq!(grid.content_right(), 228);
        assert_eq!(grid.tape_top(), 108);
        assert_eq!(grid.extra_rows(), 0);
        assert_eq!(
            grid.characters_between(MARGIN_LEFT, grid.content_right(), 6),
            36
        );
    }

    /// The larger panel spends its width on words: the same furniture, further
    /// apart, with eight more characters on the title line.
    #[test]
    fn the_e290_grid_moves_the_furniture_to_its_own_edges() {
        let grid = PanelModel::E290.grid();
        assert_eq!(grid.rail_left(), 278);
        assert_eq!(grid.content_right(), 274);
        assert_eq!(grid.tape_top(), 114);
        assert_eq!(grid.extra_rows(), 6);
        assert_eq!(
            grid.characters_between(MARGIN_LEFT, grid.content_right(), 6),
            44
        );
    }

    /// Geometry is how a board is identified, so the two panels must not be
    /// confusable and an unknown geometry must stay unknown.
    #[test]
    fn dimensions_name_exactly_one_panel() {
        assert_eq!(
            PanelModel::from_dimensions(250, 122),
            Some(PanelModel::E213)
        );
        assert_eq!(
            PanelModel::from_dimensions(296, 128),
            Some(PanelModel::E290)
        );
        assert_eq!(PanelModel::from_dimensions(250, 128), None);
        assert_eq!(PanelModel::from_dimensions(0, 0), None);
    }

    #[test]
    fn a_panel_is_recognised_by_the_name_the_banner_speaks() {
        assert_eq!(PanelModel::from_label("E290"), Some(PanelModel::E290));
        assert_eq!(PanelModel::from_label(" e213 "), Some(PanelModel::E213));
        assert_eq!(PanelModel::from_label("E999"), None);
    }

    #[test]
    fn packed_sizes_match_the_wire_contract() {
        assert_eq!(PanelModel::E213.stride(), 32);
        assert_eq!(PanelModel::E213.payload_size(), 3904);
        assert_eq!(PanelModel::E290.stride(), 37);
        assert_eq!(PanelModel::E290.payload_size(), 4736);
    }
}
