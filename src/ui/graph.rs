use egui::{Color32, Mesh, Pos2, Sense, Shape, Stroke, Ui, Vec2};

use crate::chess::review::MoveClass;

pub const GRAPH_HEIGHT: f32 = 64.0;

/// Darker variants keep the marks readable on a light panel.
pub fn class_color(class: MoveClass, dark: bool) -> Option<Color32> {
    let (bright, deep) = match class {
        MoveClass::Inaccuracy => ((225, 185, 50), (150, 105, 0)),
        MoveClass::Mistake => ((235, 135, 45), (185, 85, 0)),
        MoveClass::Blunder => ((225, 70, 70), (190, 35, 35)),
        MoveClass::Best | MoveClass::Good => return None,
    };
    let (r, g, b) = if dark { bright } else { deep };
    Some(Color32::from_rgb(r, g, b))
}

pub fn index_at(x: f32, width: f32, points: usize) -> usize {
    if points < 2 || width <= 0.0 {
        return 0;
    }
    let frac = (x / width).clamp(0.0, 1.0);
    (frac * (points - 1) as f32).round() as usize
}

pub fn show_eval_graph(
    ui: &mut Ui,
    white_win: &[f32],
    classes: &[MoveClass],
    cursor: usize,
) -> Option<usize> {
    let width = ui.available_width();
    let (response, painter) =
        ui.allocate_painter(Vec2::new(width, GRAPH_HEIGHT), Sense::click_and_drag());
    let rect = response.rect;
    let n = white_win.len();

    painter.rect_filled(rect, 2.0, Color32::from_rgb(52, 52, 52));

    let x_of = |i: usize| {
        if n < 2 {
            rect.center().x
        } else {
            rect.left() + rect.width() * i as f32 / (n - 1) as f32
        }
    };
    let y_of = |win: f32| rect.bottom() - rect.height() * (win.clamp(0.0, 100.0) / 100.0);

    if n >= 2 {
        let fill = Color32::from_rgb(232, 232, 232);
        let mut mesh = Mesh::default();
        for (i, &win) in white_win.iter().enumerate() {
            let x = x_of(i);
            mesh.colored_vertex(Pos2::new(x, rect.bottom()), fill);
            mesh.colored_vertex(Pos2::new(x, y_of(win)), fill);
            if i > 0 {
                let base = (i as u32 - 1) * 2;
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base + 1, base + 3, base + 2);
            }
        }
        painter.add(Shape::mesh(mesh));
    }

    painter.line_segment(
        [
            Pos2::new(rect.left(), rect.center().y),
            Pos2::new(rect.right(), rect.center().y),
        ],
        Stroke::new(1.0_f32, Color32::from_rgb(130, 130, 130)),
    );

    for (i, class) in classes.iter().enumerate() {
        if let (Some(color), Some(&win)) = (class_color(*class, true), white_win.get(i + 1)) {
            painter.circle_filled(Pos2::new(x_of(i + 1), y_of(win)), 3.0, color);
        }
    }

    if cursor < n {
        let x = x_of(cursor);
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.5_f32, Color32::from_rgb(90, 150, 240)),
        );
    }

    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        return Some(index_at(pointer.x - rect.left(), rect.width(), n));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_at_maps_edges_and_middle() {
        assert_eq!(index_at(0.0, 400.0, 5), 0);
        assert_eq!(index_at(400.0, 400.0, 5), 4);
        assert_eq!(index_at(200.0, 400.0, 5), 2);
        assert_eq!(index_at(149.0, 400.0, 5), 1);
        assert_eq!(index_at(151.0, 400.0, 5), 2);
    }

    #[test]
    fn index_at_clamps_and_handles_degenerate_graphs() {
        assert_eq!(index_at(-50.0, 400.0, 5), 0);
        assert_eq!(index_at(900.0, 400.0, 5), 4);
        assert_eq!(index_at(100.0, 400.0, 1), 0);
        assert_eq!(index_at(100.0, 400.0, 0), 0);
        assert_eq!(index_at(100.0, 0.0, 5), 0);
    }

    #[test]
    fn only_costly_moves_get_a_colour() {
        assert!(class_color(MoveClass::Best, true).is_none());
        assert!(class_color(MoveClass::Good, true).is_none());
        assert!(class_color(MoveClass::Inaccuracy, true).is_some());
        assert!(class_color(MoveClass::Mistake, true).is_some());
        assert!(class_color(MoveClass::Blunder, true).is_some());
    }
}
