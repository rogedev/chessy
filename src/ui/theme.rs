use egui::{
    Color32, Context, CornerRadius, FontId, Margin, Shadow, Stroke, TextStyle, Visuals, vec2,
};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    pub background: Color32,
    pub panel: Color32,
    /// Behind small icons so white pieces stay readable on the light theme.
    pub chip: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub accent: Color32,
}

impl Palette {
    pub const fn new(dark: bool) -> Self {
        if dark {
            Self {
                dark,
                background: Color32::from_rgb(28, 28, 30),
                panel: Color32::from_rgb(37, 37, 40),
                chip: Color32::from_rgb(60, 60, 65),
                text: Color32::from_rgb(226, 226, 228),
                text_weak: Color32::from_rgb(150, 150, 156),
                accent: Color32::from_rgb(100, 160, 240),
            }
        } else {
            Self {
                dark,
                background: Color32::from_rgb(236, 236, 238),
                panel: Color32::from_rgb(248, 248, 249),
                chip: Color32::from_rgb(208, 208, 214),
                text: Color32::from_rgb(28, 28, 32),
                text_weak: Color32::from_rgb(100, 100, 108),
                accent: Color32::from_rgb(30, 100, 200),
            }
        }
    }
}

#[cfg(test)]
fn luminance(c: Color32) -> f32 {
    let channel = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

/// WCAG contrast ratio, from 1 (identical) to 21 (black on white).
#[cfg(test)]
pub fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

pub fn apply(ctx: &Context, dark: bool) {
    let p = Palette::new(dark);
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    v.panel_fill = p.background;
    v.window_fill = p.panel;
    v.extreme_bg_color = if dark {
        Color32::from_rgb(22, 22, 24)
    } else {
        Color32::WHITE
    };
    v.selection.bg_fill = p.accent.gamma_multiply(0.4);
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.hyperlink_color = p.accent;

    let (idle, hover, press) = if dark {
        (
            Color32::from_rgb(52, 52, 57),
            Color32::from_rgb(66, 66, 72),
            Color32::from_rgb(78, 78, 86),
        )
    } else {
        (
            Color32::from_rgb(228, 228, 232),
            Color32::from_rgb(216, 216, 222),
            Color32::from_rgb(204, 204, 212),
        )
    };
    v.widgets.inactive.weak_bg_fill = idle;
    v.widgets.hovered.weak_bg_fill = hover;
    v.widgets.active.weak_bg_fill = press;
    v.widgets.open.weak_bg_fill = hover;

    let radius = CornerRadius::same(6);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = radius;
    }
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow {
        offset: [0, 6],
        blur: 22,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 45 }),
    };
    ctx.set_visuals(v);

    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = vec2(8.0, 6.0);
        style.spacing.button_padding = vec2(10.0, 5.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.window_margin = Margin::same(14);
        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(19.0)),
            (TextStyle::Body, FontId::proportional(14.0)),
            (TextStyle::Button, FontId::proportional(14.0)),
            (TextStyle::Small, FontId::proportional(12.0)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_ratio_extremes() {
        assert!((contrast_ratio(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(Color32::GRAY, Color32::GRAY) - 1.0).abs() < 0.001);
    }

    #[test]
    fn contrast_ratio_is_symmetric() {
        let a = Color32::from_rgb(30, 100, 200);
        let b = Color32::from_rgb(240, 240, 240);
        assert_eq!(contrast_ratio(a, b), contrast_ratio(b, a));
    }

    #[test]
    fn move_marks_are_readable_on_the_panel() {
        use crate::chess::review::MoveClass;
        use crate::ui::graph::class_color;

        for dark in [true, false] {
            let panel = Palette::new(dark).panel;
            for class in [
                MoveClass::Inaccuracy,
                MoveClass::Mistake,
                MoveClass::Blunder,
            ] {
                let color = class_color(class, dark).expect("marked class");
                let ratio = contrast_ratio(color, panel);
                assert!(ratio >= 3.0, "dark={dark} {class:?}: ratio {ratio}");
            }
        }
    }

    #[test]
    fn text_is_readable_on_every_surface() {
        for dark in [true, false] {
            let p = Palette::new(dark);
            for surface in [p.panel, p.background] {
                let ratio = contrast_ratio(p.text, surface);
                assert!(ratio >= 7.0, "dark={dark}: text ratio {ratio}");
                let ratio = contrast_ratio(p.text_weak, surface);
                assert!(ratio >= 3.5, "dark={dark}: weak text ratio {ratio}");
                let ratio = contrast_ratio(p.accent, surface);
                assert!(ratio >= 3.0, "dark={dark}: accent ratio {ratio}");
            }
        }
    }

    #[test]
    fn chip_stands_out_from_the_panel() {
        for dark in [true, false] {
            let p = Palette::new(dark);
            assert!(contrast_ratio(p.chip, p.panel) >= 1.15, "dark={dark}");
        }
    }
}
