//! Unicode and ASCII glyph sets for HADAL's terminal display.

// ---------------------------------------------------------------------
// Unicode glyphs (default)
// ---------------------------------------------------------------------

pub const UNICODE: Glyphs = Glyphs {
    h_line: '─',
    v_line: '│',
    l_cross: '├',
    r_cross: '┤',
    bar: '█',
    bar_empty: '░',
    arrow_up: '↑',
    arrow_down: '↓',
    chevron_right: '›',
};

// ---------------------------------------------------------------------
// ASCII glyphs (fallback)
// ---------------------------------------------------------------------

pub const ASCII: Glyphs = Glyphs {
    h_line: '-',
    v_line: '|',
    l_cross: '+',
    r_cross: '+',
    bar: '#',
    bar_empty: ':',
    arrow_up: '^',
    arrow_down: 'v',
    chevron_right: '>',
};

// ---------------------------------------------------------------------
// Glyph struct
// ---------------------------------------------------------------------

/// A set of display glyphs.
///
/// Only glyphs the renderer actually draws are present, so an added glyph
/// never silently diverges from the active set.
#[derive(Clone, Copy, Debug)]
pub struct Glyphs {
    pub h_line: char,
    pub v_line: char,
    pub l_cross: char,
    pub r_cross: char,
    pub bar: char,
    pub bar_empty: char,
    pub arrow_up: char,
    pub arrow_down: char,
    pub chevron_right: char,
}

/// Return the glyph set for the current mode.
pub fn set(ascii: bool) -> &'static Glyphs {
    if ascii { &ASCII } else { &UNICODE }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_set_non_ascii() {
        let g = set(false);
        assert_ne!(g.h_line, '-');
        assert_ne!(g.v_line, '|');
    }

    #[test]
    fn ascii_set_is_ascii() {
        let g = set(true);
        assert_eq!(g.h_line, '-');
        assert_eq!(g.v_line, '|');
        assert_eq!(g.l_cross, '+');
        assert_eq!(g.r_cross, '+');
    }

    #[test]
    fn glyph_widths_are_single_cell() {
        use unicode_width::UnicodeWidthChar;

        let g = set(false);
        for glyph in [
            g.h_line,
            g.v_line,
            g.l_cross,
            g.r_cross,
            g.bar,
            g.bar_empty,
            g.arrow_up,
            g.arrow_down,
            g.chevron_right,
        ] {
            assert_eq!(
                glyph.width(),
                Some(1),
                "glyph {glyph:?} is not single-width"
            );
        }
    }
}
