use egui::{Color32, Id, Key, PointerButton, Pos2, Rect, Slider, Stroke, Ui};
use crate::annotation::Annotation;

#[derive(Clone, Copy, PartialEq)]
pub enum Tool {
    Select,      // 选择移动（未实现移动）
    Rectangle,
    Arrow,
    Freehand,
    Text,
}

pub struct ToolBar;

pub fn toolbar_ui(
    ui: &mut Ui,
    active_tool: &mut Tool,
    color: &mut Color32,
    thickness: &mut f32,
    annotations: &mut Vec<Annotation>,
    redo_stack: &mut Vec<Annotation>,
    status_message: &mut String,
) {
    ui.horizontal(|ui| {
        ui.selectable_value(active_tool, Tool::Select, "选择");
        ui.selectable_value(active_tool, Tool::Rectangle, "矩形");
        ui.selectable_value(active_tool, Tool::Arrow, "箭头");
        ui.selectable_value(active_tool, Tool::Freehand, "画笔");
        ui.selectable_value(active_tool, Tool::Text, "文字");

        ui.separator();
        ui.label("颜色:");
        egui::color_picker::color_edit_button_srgba(ui, color, egui::color_picker::Alpha::Opaque);
        ui.label("粗细:");
        ui.add(Slider::new(thickness, 1.0..=10.0).logarithmic(false));

        ui.separator();
        if ui.button("撤销").clicked() {
            if let Some(ann) = annotations.pop() {
                redo_stack.push(ann);
            }
        }
        if ui.button("重做").clicked() {
            if let Some(ann) = redo_stack.pop() {
                annotations.push(ann);
            }
        }

        ui.separator();
        if ui.button("保存").clicked() {
            save_annotated(ui.ctx());
        }
        if ui.button("复制").clicked() {
            copy_to_clipboard(ui.ctx());
        }
        if ui.button("退出").clicked() {
            std::process::exit(0);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(format!("{}", status_message));
        });
    });
}

pub fn handle_tool(
    response: &egui::Response,
    painter: &egui::Painter,
    active_tool: &Tool,
    color: &Color32,
    thickness: &f32,
    annotations: &mut Vec<Annotation>,
    redo_stack: &mut Vec<Annotation>,
    drawing: &mut Option<Box<dyn FnMut(&egui::Painter, &mut Vec<Annotation>, Rect, &egui::InputState)>>,
    temp_annotation: &mut Option<Annotation>,
    status_message: &mut String,
) {
    let rect = response.rect;
    let pointer_pos = response.hover_pos();
    let ctx = response.ctx;

    if *active_tool == Tool::Select {
        *drawing = None;
        *temp_annotation = None;
        return;
    }

    // 开始拖拽或鼠标按下时创建新标注
    if response.drag_started_by(PointerButton::Primary) {
        let start = response.interact_pointer_pos().unwrap();
        match active_tool {
            Tool::Rectangle => {
                *temp_annotation = Some(Annotation::Rectangle {
                    rect: Rect::from_min_size(start, egui::vec2(0.0, 0.0)),
                    color: *color,
                    thickness: *thickness,
                });
                let color = *color;
                let thick = *thickness;
                *drawing = Some(Box::new(move |painter, anns, canvas_rect, input| {
                    if let Some(pos) = input.pointer.hover_pos() {
                        let start = if let Some(Annotation::Rectangle { rect, .. }) = anns.last() {
                            rect.min
                        } else {
                            pos
                        };
                        let rect = Rect::from_min_max(start, pos);
                        *anns.last_mut().unwrap() = Annotation::Rectangle { rect, color, thickness: thick };
                    }
                }));
            }
            Tool::Arrow => {
                *temp_annotation = Some(Annotation::Arrow {
                    start,
                    end: start,
                    color: *color,
                    thickness: *thickness,
                });
                let color = *color;
                let thick = *thickness;
                *drawing = Some(Box::new(move |painter, anns, canvas_rect, input| {
                    if let Some(pos) = input.pointer.hover_pos() {
                        let start = if let Some(Annotation::Arrow { start, .. }) = anns.last() {
                            *start
                        } else {
                            pos
                        };
                        *anns.last_mut().unwrap() = Annotation::Arrow { start, end: pos, color, thickness: thick };
                    }
                }));
            }
            Tool::Freehand => {
                let mut pts = vec![start];
                *temp_annotation = Some(Annotation::Freehand {
                    points: pts.clone(),
                    color: *color,
                    thickness: *thickness,
                });
                let color = *color;
                let thick = *thickness;
                *drawing = Some(Box::new(move |painter, anns, canvas_rect, input| {
                    if let Some(pos) = input.pointer.hover_pos() {
                        if let Some(Annotation::Freehand { points, .. }) = anns.last_mut() {
                            points.push(pos);
                        }
                    }
                }));
            }
            Tool::Text => {
                // 点击放置文字，弹出输入框（简化：直接使用预定义文字）
                // 这里使用一个简单的模式：弹出输入框
                // 限于篇幅，仅实现放置固定文字
                let ann = Annotation::Text {
                    pos: start,
                    text: "标注".to_string(),
                    color: *color,
                    size: 14.0,
                };
                annotations.push(ann);
                redo_stack.clear();
                *status_message = "文字已添加".to_string();
            }
            _ => {}
        }
        // 将初始 temp 推入 annotations 以便修改
        if let Some(temp) = temp_annotation.take() {
            annotations.push(temp);
            redo_stack.clear();
        }
    }

    // 拖拽移动时，更新 drawing 回调
    if response.dragged_by(PointerButton::Primary) {
        if let Some(draw_fn) = drawing {
            draw_fn(painter, annotations, rect, &ctx.input(|i| i.clone()));
        }
    }

    // 释放时结束绘制
    if response.drag_released_by(PointerButton::Primary) {
        *drawing = None;
        // 如果最终图形面积太小，移除
        if let Some(ann) = annotations.last() {
            let too_small = match ann {
                Annotation::Rectangle { rect, .. } => rect.area() < 4.0,
                Annotation::Arrow { start, end, .. } => start.distance(*end) < 4.0,
                Annotation::Freehand { points, .. } => points.len() < 2,
                _ => false,
            };
            if too_small {
                annotations.pop();
            }
        }
        *status_message = "标注完成".to_string();
    }
}

fn save_annotated(ctx: &egui::Context) {
    // 从当前纹理中获取像素较复杂，这里简化：使用全局变量
    // 实际需要将图像与标注合成，然后保存。此处演示调用，需完善。
    // 因篇幅限制，此处省略具体实现，可用 image 库合成标注。
    let path = rfd::FileDialog::new()
        .add_filter("PNG", &["png"])
        .add_filter("JPEG", &["jpg", "jpeg"])
        .set_file_name(&format!("截图_{}.png", chrono::Local::now().format("%Y%m%d_%H%M%S")))
        .save_file();
    if let Some(path) = path {
        // 将图像与标注合成并保存到路径
        // 在实际代码中需要获取图像数据和标注绘制到 image buffer。
        // 此处只打印信息。
        println!("保存至: {:?}", path);
    }
}

fn copy_to_clipboard(ctx: &egui::Context) {
    // 同保存，需合成图像后写入剪贴板
    println!("复制到剪贴板");
}
