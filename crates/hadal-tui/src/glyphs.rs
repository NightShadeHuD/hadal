//! Unicode and ASCII glyph sets for HADAL's terminal display.

// ---------------------------------------------------------------------
// Unicode glyphs (default)
// ---------------------------------------------------------------------

pub const UNICODE: Glyphs = Glyphs {
    h_line: '─',
    v_line: '│',
    lt_corner: '┌',
    rt_corner: '┐',
    lb_corner: '└',
    rb_corner: '┘',
    t_cross: '┬',
    b_cross: '┴',
    l_cross: '├',
    r_cross: '┤',
    dot: '·',
    dash: '‐',
    bar: '█',
    bar_empty: '░',
    scope_dot: '•',
    scope_ring: '◦',
    scope_cross: '×',
    arrow_right: '→',
    arrow_left: '←',
    arrow_up: '↑',
    arrow_down: '↓',
    chevron_right: '›',
    chevron_left: '‹',
    star: '★',
    star_empty: '☆',
    check: "✓",
    x_mark: "✗",
    separator: '┄',
    capacitor_full: '█',
    capacitor_empty: '░',
    carrier_tuned: '◆',
    carrier_muted: '◇',
    depth_gauge: '▼',
};

// ---------------------------------------------------------------------
// ASCII glyphs (fallback)
// ---------------------------------------------------------------------

pub const ASCII: Glyphs = Glyphs {
    h_line: '-',
    v_line: '|',
    lt_corner: '+',
    rt_corner: '+',
    lb_corner: '+',
    rb_corner: '+',
    t_cross: '+',
    b_cross: '+',
    l_cross: '+',
    r_cross: '+',
    dot: '.',
    dash: '-',
    bar: '#',
    bar_empty: ':',
    scope_dot: '*',
    scope_ring: 'o',
    scope_cross: 'x',
    arrow_right: '>',
    arrow_left: '<',
    arrow_up: '^',
    arrow_down: 'v',
    chevron_right: '>',
    chevron_left: '<',
    star: '*',
    star_empty: ' ',
    check: "[+]",
    x_mark: "[-]",
    separator: '.',
    capacitor_full: '#',
    capacitor_empty: ':',
    carrier_tuned: '*',
    carrier_muted: 'o',
    depth_gauge: 'v',
};

// ---------------------------------------------------------------------
// Glyph struct
// ---------------------------------------------------------------------

/// A set of display glyphs.
///
/// The set is intentionally complete (scope dots, depth gauge, carrier strip,
/// stars) so later phases never emit a glyph absent from the active set.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct Glyphs {
    pub h_line: char,
    pub v_line: char,
    pub lt_corner: char,
    pub rt_corner: char,
    pub lb_corner: char,
    pub rb_corner: char,
    pub t_cross: char,
    pub b_cross: char,
    pub l_cross: char,
    pub r_cross: char,
    pub dot: char,
    pub dash: char,
    pub bar: char,
    pub bar_empty: char,
    pub scope_dot: char,
    pub scope_ring: char,
    pub scope_cross: char,
    pub arrow_right: char,
    pub arrow_left: char,
    pub arrow_up: char,
    pub arrow_down: char,
    pub chevron_right: char,
    pub chevron_left: char,
    pub star: char,
    pub star_empty: char,
    pub check: &'static str,
    pub x_mark: &'static str,
    pub separator: char,
    pub capacitor_full: char,
    pub capacitor_empty: char,
    pub carrier_tuned: char,
    pub carrier_muted: char,
    pub depth_gauge: char,
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
        assert_eq!(g.lt_corner, '+');
        assert_eq!(g.rb_corner, '+');
    }

    #[test]
    fn glyph_widths_are_single_cell() {
        use unicode_width::UnicodeWidthChar;

        let g = set(false);
        for glyph in [
            g.h_line,
            g.v_line,
            g.lt_corner,
            g.rt_corner,
            g.lb_corner,
            g.rb_corner,
            g.t_cross,
            g.b_cross,
            g.l_cross,
            g.r_cross,
            g.dot,
            g.bar,
            g.bar_empty,
            g.scope_dot,
            g.scope_ring,
            g.scope_cross,
            g.carrier_tuned,
            g.carrier_muted,
            g.depth_gauge,
            g.separator,
        ] {
            assert_eq!(
                glyph.width(),
                Some(1),
                "glyph {glyph:?} is not single-width"
            );
        }

        // check/x_mark are single-character in Unicode, multi-char in ASCII.
        assert_eq!(set(false).check.chars().count(), 1);
        assert_eq!(set(false).x_mark.chars().count(), 1);
        assert_eq!(set(true).check, "[+]");
        assert_eq!(set(true).x_mark, "[-]");
    }
}
