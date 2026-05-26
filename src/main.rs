// 隐藏 Windows 控制台黑框 (必须在文件第一行)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use egui::*;
use image::{DynamicImage, Rgba, RgbaImage};

// ==================== 数据结构 ====================

#[derive(Clone, Debug)]
enum Annotation {
    Rect { min: Pos2, max: Pos2, color: Color32, thickness: f32 },
    Arrow { start: Pos2, end: Pos2, color: Color32, thickness: f32 },
    Freehand { points: Vec<Pos2>, color: Color32, thickness: f32 },
    Text { pos: Pos2, text: String, color: Color32, size: f32 },
    Number { pos: Pos2, number: u32, color: Color32, radius: f32 },
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tool { Rect, Arrow, Freehand, Text, Number }

#[derive(Clone)]
struct ScreenshotData {
    image: RgbaImage,
    annotations: Vec<Annotation>,
    redo_stack: Vec<Annotation>,
    texture: Option<TextureHandle>,
}

enum AppState {
    Selecting {
        full_image: DynamicImage,
        texture: Option<TextureHandle>,
        dragging: bool,
        start: Pos2,
        end: Pos2,
    },
    Editing {
        screenshots: Vec<ScreenshotData>,
        active_idx: usize,
        active_tool: Tool,
        color: Color32,
        thickness: f32,
        text_input: String,
        drawing: bool,
        draw_start: Pos2,
        freehand_points: Vec<Pos2>,
        status: String,
        number_counter: u32,
    },
}

enum AppAction {
    EnterSelecting,
    EnterEditing(RgbaImage),
    Close,
}

// ==================== 屏幕捕获 ====================

fn capture_fullscreen() -> Result<DynamicImage, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("获取显示器失败: {e}"))?;
    let monitor = monitors.into_iter().next().ok_or("未检测到显示器")?;
    let buf = monitor.capture_image().map_err(|e| format!("截图失败: {e}"))?;
    let (w, h) = (buf.width(), buf.height());
    let raw = buf.into_raw();
    let rgba = RgbaImage::from_raw(w, h, raw).ok_or("图像数据转换失败")?;
    Ok(DynamicImage::ImageRgba8(rgba))
}

fn dynimage_to_egui(img: &DynamicImage) -> ColorImage {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw())
}

fn rgba_to_egui(img: &RgbaImage) -> ColorImage {
    let (w, h) = img.dimensions();
    ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw())
}

// ==================== 标注绘制 (UI) ====================

fn draw_annotation_ui(ann: &Annotation, painter: &Painter, scale: f32, offset: Pos2) {
    match ann {
        Annotation::Rect { min, max, color, thickness } => {
            let r = Rect::from_min_max(*min * scale + offset.to_vec2(), *max * scale + offset.to_vec2());
            painter.rect_stroke(r, 0.0, Stroke::new(*thickness * scale, *color));
        }
        Annotation::Arrow { start, end, color, thickness } => {
            let s = *start * scale + offset.to_vec2();
            let e = *end * scale + offset.to_vec2();
            painter.arrow(s, e - s, Stroke::new(*thickness * scale, *color));
        }
        Annotation::Freehand { points, color, thickness } => {
            if points.len() >= 2 {
                let scaled_pts: Vec<Pos2> = points.iter().map(|p| *p * scale + offset.to_vec2()).collect();
                painter.add(Shape::line(scaled_pts, Stroke::new(*thickness * scale, *color)));
            }
        }
        Annotation::Text { pos, text, color, size } => {
            let p = *pos * scale + offset.to_vec2();
            painter.text(p, Align2::LEFT_TOP, text, FontId::proportional(*size * scale), *color);
        }
        Annotation::Number { pos, number, color, radius } => {
            let p = *pos * scale + offset.to_vec2();
            let r = *radius * scale;
            painter.circle_filled(p, r, *color);
            painter.text(p, Align2::CENTER_CENTER, format!("{number}"), FontId::proportional(r * 1.2), Color32::WHITE);
        }
    }
}

// ==================== 标注绘制 (导出到图片) ====================

fn render_annotations_on_image(img: &mut RgbaImage, annotations: &[Annotation]) {
    for ann in annotations {
        match ann {
            Annotation::Rect { min, max, color, thickness } => {
                let t = thickness.round() as i32;
                let c = Rgba([color.r(), color.g(), color.b(), color.a()]);
                let (x1, y1) = (min.x as i32, min.y as i32);
                let (x2, y2) = (max.x as i32, max.y as i32);
                for i in 0..t {
                    draw_line(img, x1 + i, y1, x2 - i, y1, c);
                    draw_line(img, x1 + i, y2 - i, x2 - i, y2 - i, c);
                    draw_line(img, x1, y1 + i, x1, y2 - i, c);
                    draw_line(img, x2 - i, y1 + i, x2 - i, y2 - i, c);
                }
            }
            Annotation::Arrow { start, end, color, thickness } => {
                let c = Rgba([color.r(), color.g(), color.b(), color.a()]);
                let t = thickness.round() as i32;
                let (sx, sy) = (start.x as i32, start.y as i32);
                let (ex, ey) = (end.x as i32, end.y as i32);
                for i in -t / 2..=t / 2 {
                    draw_line(img, sx + i, sy, ex + i, ey, c);
                    draw_line(img, sx, sy + i, ex, ey + i, c);
                }
                let dx = (ex - sx) as f32;
                let dy = (ey - sy) as f32;
                let len = (dx * dx + dy * dy).sqrt();
                if len > 0.0 {
                    let (ux, uy) = (dx / len, dy / len);
                    let (bx, by) = (ex as f32 - ux * 15.0, ey as f32 - uy * 15.0);
                    let (nx, ny) = (-uy * 6.0, ux * 6.0);
                    draw_line(img, ex, ey, (bx + nx) as i32, (by + ny) as i32, c);
                    draw_line(img, ex, ey, (bx - nx) as i32, (by - ny) as i32, c);
                }
            }
            Annotation::Freehand { points, color, thickness } => {
                let c = Rgba([color.r(), color.g(), color.b(), color.a()]);
                let t = thickness.round() as i32;
                for pair in points.windows(2) {
                    let (x1, y1) = (pair[0].x as i32, pair[0].y as i32);
                    let (x2, y2) = (pair[1].x as i32, pair[1].y as i32);
                    for i in -t / 2..=t / 2 {
                        draw_line(img, x1 + i, y1, x2 + i, y2, c);
                        draw_line(img, x1, y1 + i, x2, y2 + i, c);
                    }
                }
            }
            Annotation::Number { pos, color, radius, .. } => {
                let c = Rgba([color.r(), color.g(), color.b(), color.a()]);
                let cx = pos.x as i32;
                let cy = pos.y as i32;
                let r = *radius as i32;
                for y in -r..=r {
                    for x in -r..=r {
                        if x * x + y * y <= r * r {
                            let px = cx + x;
                            let py = cy + y;
                            if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                                img.put_pixel(px as u32, py as u32, c);
                            }
                        }
                    }
                }
            }
            _ => {} // 文字渲染到图片需要额外字体库，此处仅在 UI 显示
        }
    }
}

fn draw_line(img: &mut RgbaImage, x1: i32, y1: i32, x2: i32, y2: i32, color: Rgba<u8>) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let dx = (x2 - x1).abs();
    let dy = -(y2 - y1).abs();
    let sx = if x1 < x2 { 1 } else { -1 };
    let sy = if y1 < y2 { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut x, mut y) = (x1, y1);
    loop {
        if x >= 0 && x < w && y >= 0 && y < h {
            img.put_pixel(x as u32, y as u32, color);
        }
        if x == x2 && y == y2 { break; }
        let e2 = 2 * err;
        if e2 >= dy { err += dy; x += sx; }
        if e2 <= dx { err += dx; y += sy; }
    }
}

// ==================== 主应用 ====================

struct ScreenshotApp {
    state: AppState,
    pending_action: Option<AppAction>,
}

impl ScreenshotApp {
    fn new() -> Self {
        Self {
            state: AppState::Selecting {
                full_image: DynamicImage::new_rgba8(1, 1),
                texture: None,
                dragging: false,
                start: pos2(0.0, 0.0),
                end: pos2(0.0, 0.0),
            },
            pending_action: Some(AppAction::EnterSelecting), // 启动即截图
        }
    }
}

impl eframe::App for ScreenshotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. 处理状态切换
        if let Some(action) = self.pending_action.take() {
            match action {
                AppAction::EnterSelecting => {
                    ctx.send_viewport_cmd(ViewportCommand::Decorations(false));
                    ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
                    ctx.send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
                    
                    match capture_fullscreen() {
                        Ok(img) => {
                            self.state = AppState::Selecting {
                                full_image: img,
                                texture: None,
                                dragging: false,
                                start: pos2(0.0, 0.0),
                                end: pos2(0.0, 0.0),
                            };
                        }
                        Err(e) => {
                            rfd::MessageDialog::new().set_title("错误").set_description(&e).show();
                            self.pending_action = Some(AppAction::Close);
                        }
                    }
                }
                AppAction::EnterEditing(cropped_img) => {
                    ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
                    ctx.send_viewport_cmd(ViewportCommand::Decorations(true));
                    ctx.send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::Normal));
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(900.0, 650.0)));
                    
                    let new_shot = ScreenshotData {
                        image: cropped_img,
                        annotations: vec![],
                        redo_stack: vec![],
                        texture: None,
                    };

                    if let AppState::Editing { screenshots, active_idx, .. } = &mut self.state {
                        screenshots.push(new_shot);
                        *active_idx = screenshots.len() - 1;
                    } else {
                        self.state = AppState::Editing {
                            screenshots: vec![new_shot],
                            active_idx: 0,
                            active_tool: Tool::Rect,
                            color: Color32::RED,
                            thickness: 3.0,
                            text_input: "文本".into(),
                            drawing: false,
                            draw_start: pos2(0.0, 0.0),
                            freehand_points: vec![],
                            status: String::new(),
                            number_counter: 1,
                        };
                    }
                }
                AppAction::Close => {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
            ctx.request_repaint();
            return;
        }

        // 2. 渲染 UI
        match &mut self.state {
            // ========== 全屏选区模式 ==========
            AppState::Selecting { full_image, texture, dragging, start, end } => {
                if texture.is_none() {
                    let ci = dynimage_to_egui(full_image);
                    *texture = Some(ctx.load_texture("fullscreen", ci, TextureOptions::LINEAR));
                }

                CentralPanel::default().frame(Frame::none()).show(ctx, |ui| {
                    let available = ui.available_size();
                    let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
                    let canvas = response.rect;

                    if let Some(tex) = texture {
                        painter.image(tex.id(), canvas, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }

                    let dark = Color32::from_black_alpha(150);
                    let sel = if *dragging || *start != *end {
                        Rect::from_min_max(start.min(*end), start.max(*end))
                    } else {
                        Rect::NOTHING
                    };

                    if sel.is_positive() {
                        painter.rect_filled(Rect::from_min_max(canvas.min, pos2(canvas.max.x, sel.min.y)), 0.0, dark);
                        painter.rect_filled(Rect::from_min_max(pos2(canvas.min.x, sel.max.y), canvas.max), 0.0, dark);
                        painter.rect_filled(Rect::from_min_max(pos2(canvas.min.x, sel.min.y), pos2(sel.min.x, sel.max.y)), 0.0, dark);
                        painter.rect_filled(Rect::from_min_max(pos2(sel.max.x, sel.min.y), pos2(canvas.max.x, sel.max.y)), 0.0, dark);
                        painter.rect_stroke(sel, 0.0, Stroke::new(2.0, Color32::WHITE));

                        let scale_x = full_image.width() as f32 / canvas.width();
                        let scale_y = full_image.height() as f32 / canvas.height();
                        let sw = (sel.width() * scale_x) as u32;
                        let sh = (sel.height() * scale_y) as u32;
                        painter.text(sel.min + vec2(4.0, -6.0), Align2::LEFT_BOTTOM, format!("{sw} × {sh}"), FontId::proportional(14.0), Color32::WHITE);
                    } else {
                        painter.text(canvas.center(), Align2::CENTER_CENTER, "拖动鼠标选择截图区域\n按 ESC 取消", FontId::proportional(24.0), Color32::from_gray(220));
                    }

                    if response.drag_started_by(PointerButton::Primary) {
                        *dragging = true;
                        *start = response.interact_pointer_pos().unwrap_or(canvas.min);
                    }
                    if *dragging {
                        if let Some(pos) = response.interact_pointer_pos() {
                            *end = pos.clamp(canvas.min, canvas.max);
                        }
                    }
                    if response.drag_stopped_by(PointerButton::Primary) {
                        *dragging = false;
                        let sel = Rect::from_min_max(start.min(*end), start.max(*end));
                        if sel.area() > 25.0 {
                            let scale_x = full_image.width() as f32 / canvas.width();
                            let scale_y = full_image.height() as f32 / canvas.height();
                            let x = (sel.min.x * scale_x).round() as u32;
                            let y = (sel.min.y * scale_y).round() as u32;
                            let w = (sel.width() * scale_x).round() as u32;
                            let h = (sel.height() * scale_y).round() as u32;
                            let cropped = full_image.crop_imm(x, y, w, h).to_rgba8();
                            self.pending_action = Some(AppAction::EnterEditing(cropped));
                        }
                    }

                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        self.pending_action = Some(AppAction::Close);
                    }
                });
            }

            // ========== 编辑模式 ==========
            AppState::Editing {
                screenshots, active_idx, active_tool, color, thickness, text_input,
                drawing, draw_start, freehand_points, status, number_counter,
            } => {
                if screenshots.is_empty() {
                    self.pending_action = Some(AppAction::Close);
                    return;
                }
                *active_idx = (*active_idx).min(screenshots.len() - 1);

                // ---- 顶部工具栏 (自动换行防遮挡) ----
                TopBottomPanel::top("toolbar").show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.set_height(28.0);
                        if ui.button("📷 新截图").clicked() {
                            self.pending_action = Some(AppAction::EnterSelecting);
                        }
                        ui.separator();
                        ui.selectable_value(active_tool, Tool::Rect, "▢ 矩形");
                        ui.selectable_value(active_tool, Tool::Arrow, "➤ 箭头");
                        ui.selectable_value(active_tool, Tool::Freehand, "✎ 画笔");
                        ui.selectable_value(active_tool, Tool::Text, "T 文字");
                        ui.selectable_value(active_tool, Tool::Number, "① 序号");
                        ui.separator();
                        ui.label("颜色:");
                        ui.color_edit_button_srgba(color);
                        ui.label("粗细:");
                        ui.add(Slider::new(thickness, 1.0..=12.0).desired_width(80.0));
                        if *active_tool == Tool::Text {
                            ui.label("文字:");
                            ui.text_edit_singleline(text_input);
                        }
                        ui.separator();
                        
                        let cur = &mut screenshots[*active_idx];
                        if ui.button("↩ 撤销").clicked() { if let Some(a) = cur.annotations.pop() { cur.redo_stack.push(a); } }
                        if ui.button("↪ 重做").clicked() { if let Some(a) = cur.redo_stack.pop() { cur.annotations.push(a); } }
                        ui.separator();
                        
                        if ui.button("💾 保存").clicked() {
                            let mut out = cur.image.clone();
                            render_annotations_on_image(&mut out, &cur.annotations);
                            let default_name = format!("截图_{}.png", chrono::Local::now().format("%Y%m%d_%H%M%S"));
                            if let Some(path) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name(&default_name).save_file() {
                                let _ = DynamicImage::ImageRgba8(out).save(&path);
                                *status = format!("已保存: {}", path.file_name().unwrap_or_default().to_string_lossy());
                            }
                        }
                        if ui.button("📋 复制").clicked() {
                            let mut out = cur.image.clone();
                            render_annotations_on_image(&mut out, &cur.annotations);
                            if let Ok(mut clip) = arboard::Clipboard::new() {
                                let _ = clip.set_image(arboard::ImageData { width: out.width() as usize, height: out.height() as usize, bytes: std::borrow::Cow::Borrowed(out.as_raw()) });
                                *status = "已复制到剪贴板".into();
                            }
                        }
                        if ui.button("✕ 退出").clicked() { self.pending_action = Some(AppAction::Close); }
                    });
                    if !status.is_empty() { ui.label(status.as_str()); }
                });

                // ---- 中央画布 ----
                CentralPanel::default().frame(Frame::none()).show(ctx, |ui| {
                    let cur = &mut screenshots[*active_idx];
                    
                    if cur.texture.is_none() {
                        let ci = rgba_to_egui(&cur.image);
                        cur.texture = Some(ui.ctx().load_texture("edit_img", ci, TextureOptions::LINEAR));
                    }
                    let tex_id = cur.texture.as_ref().unwrap().id();

                    let available = ui.available_size();
                    let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
                    let canvas_rect = response.rect;

                    let img_size = vec2(cur.image.width() as f32, cur.image.height() as f32);
                    let scale = (canvas_rect.width() / img_size.x).min(canvas_rect.height() / img_size.y).min(1.0);
                    let display_size = img_size * scale;
                    let img_rect = Rect::from_center_size(canvas_rect.center(), display_size);

                    // 绘制背景和图片
                    painter.rect_filled(canvas_rect, 0.0, Color32::from_gray(40));
                    painter.image(tex_id, img_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);

                    // 绘制已有标注
                    for ann in &cur.annotations {
                        draw_annotation_ui(ann, &painter, scale, img_rect.min);
                    }

                    // 处理鼠标交互
                    if let Some(pos) = response.interact_pointer_pos() {
                        if img_rect.contains(pos) {
                            let img_pos = (pos - img_rect.min) / scale;
                            
                            if response.drag_started_by(PointerButton::Primary) {
                                *drawing = true;
                                *draw_start = img_pos;
                                if *active_tool == Tool::Freehand { freehand_points.clear(); freehand_points.push(img_pos); }
                                if *active_tool == Tool::Text {
                                    cur.annotations.push(Annotation::Text { pos: img_pos, text: text_input.clone(), color: *color, size: 16.0 });
                                    cur.texture = None; // 强制刷新
                                    *drawing = false;
                                }
                                if *active_tool == Tool::Number {
                                    cur.annotations.push(Annotation::Number { pos: img_pos, number: *number_counter, color: *color, radius: 15.0 });
                                    *number_counter += 1;
                                    cur.texture = None;
                                    *drawing = false;
                                }
                            }
                            
                            if *drawing && response.dragged_by(PointerButton::Primary) {
                                if *active_tool == Tool::Freehand { freehand_points.push(img_pos); }
                                
                                let preview_ann = match active_tool {
                                    Tool::Rect => Some(Annotation::Rect { min: *draw_start, max: img_pos, color: *color, thickness: *thickness }),
                                    Tool::Arrow => Some(Annotation::Arrow { start: *draw_start, end: img_pos, color: *color, thickness: *thickness }),
                                    Tool::Freehand => Some(Annotation::Freehand { points: freehand_points.clone(), color: *color, thickness: *thickness }),
                                    _ => None,
                                };
                                if let Some(ann) = preview_ann { draw_annotation_ui(&ann, &painter, scale, img_rect.min); }
                            }
                            
                            if response.drag_stopped_by(PointerButton::Primary) && *drawing {
                                *drawing = false;
                                let final_ann = match active_tool {
                                    Tool::Rect => Some(Annotation::Rect { min: *draw_start, max: img_pos, color: *color, thickness: *thickness }),
                                    Tool::Arrow => Some(Annotation::Arrow { start: *draw_start, end: img_pos, color: *color, thickness: *thickness }),
                                    Tool::Freehand => Some(Annotation::Freehand { points: freehand_points.clone(), color: *color, thickness: *thickness }),
                                    _ => None,
                                };
                                if let Some(ann) = final_ann { 
                                    cur.annotations.push(ann); 
                                    cur.redo_stack.clear();
                                    cur.texture = None;
                                }
                            }
                        }
                    }
                });
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 650.0]),
        ..Default::default()
    };
    eframe::run_native(
        "截图工具",
        native_options,
        Box::new(|_cc| Box::new(ScreenshotApp::new())),
    )
}
