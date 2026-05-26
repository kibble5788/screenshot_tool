use egui::{painter::Painter, pos2, vec2, Color32, Pos2, Rect, Stroke};

#[derive(Clone, Debug)]
pub enum Annotation {
    Rectangle {
        rect: Rect,
        color: Color32,
        thickness: f32,
    },
    Arrow {
        start: Pos2,
        end: Pos2,
        color: Color32,
        thickness: f32,
    },
    Freehand {
        points: Vec<Pos2>,
        color: Color32,
        thickness: f32,
    },
    Text {
        pos: Pos2,
        text: String,
        color: Color32,
        size: f32,
    },
}

impl Annotation {
    pub fn draw(&self, painter: &Painter, canvas_rect: Rect) {
        let scale_x = canvas_rect.width() / canvas_rect.width(); // 这里假设画布坐标与像素对齐
        match self {
            Self::Rectangle { rect, color, thickness } => {
                painter.rect_stroke(*rect, 0.0, (*thickness, *color));
            }
            Self::Arrow { start, end, color, thickness } => {
                painter.arrow(*start, *end - *start, *thickness, *color);
            }
            Self::Freehand { points, color, thickness } => {
                if points.len() >= 2 {
                    painter.add(egui::Shape::line(
                        points.clone(),
                        Stroke::new(*thickness, *color),
                    ));
                }
            }
            Self::Text { pos, text, color, size } => {
                painter.text(
                    *pos,
                    egui::Align2::LEFT_TOP,
                    text,
                    egui::FontId::proportional(*size),
                    *color,
                );
            }
        }
    }
}
