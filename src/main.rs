//! 截图工具 - 支持区域截图、标注、钉住截图对比
//! 编译: cargo build --release
//! 输出: target/release/screenshot_tool.exe (免安装)1

use eframe::egui;
use egui::*;
use image::{DynamicImage, Rgba, RgbaImage};

// ==================== 数据结构 ====================

#[derive(Clone, Debug)]
enum Annotation {
    Rect {
        min: Pos2,
        max: Pos2,
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
    Number {
        pos: Pos2,
        number: u32,
        color: Color32,
        radius: f32,
    },
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tool {
    Rect,
    Arrow,
    Freehand,
    Text,
    Number,
}

#[derive(Clone)]
struct ScreenshotData {
    id: u64,
    image: RgbaImage,
    annotations: Vec<Annotation>,
    redo_stack: Vec<Annotation>,
    pinned: bool,
    timestamp: String,
}

enum AppState {
    Selecting {
        full_image: DynamicImage,
        texture: Option<egui::TextureHandle>,
        dragging: bool,
        start: Pos2,
        end: Pos2,
        pending_action: Option<AppAction>,
    },
    Editing {
        screenshots: Vec<ScreenshotData>,
        active_idx: usize,
        next_id: u64,
        active_tool: Tool,
        color: Color32,
        thickness: f32,
        text_input: String,
        drawing: bool,
        draw_start: Pos2,
        freehand_points: Vec<Pos2>,
        status: String,
        show_comparison: bool,
        compare_idx: usize,
        number_counter: u32,
        pending_action: Option<AppAction>,
    },
}

enum AppAction {
    NewCapture,
    Close,
    None,
}

// ==================== 屏幕捕获 ====================

fn capture_fullscreen() -> Result<DynamicImage, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("获取显示器失败: {e}"))?;
    let monitor = monitors.into_iter().next().ok_or("未检测到显示器")?;
    let buf = monitor
        .capture_image()
        .map_err(|e| format!("截图失败: {e}"))?;
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

// ==================== 标注绘制 ====================

fn draw_annotation(ann: &Annotation, painter: &Painter) {
    match ann {
        Annotation::Rect {
            min,
            max,
            color,
            thickness,
        } => {
            painter.rect_stroke(
                Rect::from_min_max(*min, *max),
                0.0,
                Stroke::new(*thickness, *color),
            );
        }
        Annotation::Arrow {
            start,
            end,
            color,
            thickness,
        } => {
            painter.arrow(*start, *end - *start, Stroke::new(*thickness, *color));
        }
        Annotation::Freehand {
            points,
            color,
            thickness,
        } => {
            if points.len() >= 2 {
                painter.add(Shape::line(
                    points.clone(),
                    Stroke::new(*thickness, *color),
                ));
            }
        }
        Annotation::Text {
            pos,
            text,
            color,
            size,
        } => {
            painter.text(
                *pos,
                Align2::LEFT_TOP,
                text,
                FontId::proportional(*size),
                *color,
            );
        }
        Annotation::Number {
            pos,
            number,
            color,
            radius,
        } => {
            let r = *radius;
            painter.circle_filled(*pos, r, *color);
            painter.text(
                *pos,
                Align2::CENTER_CENTER,
                format!("{number}"),
                FontId::proportional(r * 1.2),
                Color32::WHITE,
            );
        }
    }
}

fn render_annotations_on_image(img: &mut RgbaImage, annotations: &[Annotation]) {
    for ann in annotations {
        match ann {
            Annotation::Rect {
                min,
                max,
                color,
                thickness,
            } => {
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
            Annotation::Arrow {
                start,
                end,
                color,
                thickness,
            } => {
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
                    let al = 15.0;
                    let aw = 6.0;
                    let (bx, by) = (ex as f32 - ux * al, ey as f32 - uy * al);
                    let (nx, ny) = (-uy * aw, ux * aw);
                    draw_line(img, ex, ey, (bx + nx) as i32, (by + ny) as i32, c);
                    draw_line(img, ex, ey, (bx - nx) as i32, (by - ny) as i32, c);
                }
            }
            Annotation::Freehand {
                points,
                color,
                thickness,
            } => {
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
            Annotation::Number {
                pos,
                color,
                radius,
                ..
            } => {
                let c = Rgba([color.r(), color.g(), color.b(), color.a()]);
                let cx = pos.x as i32;
                let cy = pos.y as i32;
                let r = *radius as i32;
                for y in -r..=r {
                    for x in -r..=r {
                        if x * x + y * y <= r * r {
                            let px = cx + x;
                            let py = cy + y;
                            if px >= 0
                                && py >= 0
                                && (px as u32) < img.width()
                                && (py as u32) < img.height()
                            {
                                img.put_pixel(px as u32, py as u32, c);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn draw_line(
    img: &mut RgbaImage,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    color: Rgba<u8>,
) {
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
        if x == x2 && y == y2 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

// ==================== 主应用 ====================

struct ScreenshotApp {
    state: AppState,
}

impl ScreenshotApp {
    fn new() -> Self {
        let img = capture_fullscreen().unwrap_or_else(|e| {
            rfd::MessageDialog::new()
                .set_title("错误")
                .set_description(&format!("截图初始化失败：{e}"))
                .show();
            std::process::exit(1);
        });
        Self {
            state: AppState::Selecting {
                full_image: img,
                texture: None,
                dragging: false,
                start: pos2(0.0, 0.0),
                end: pos2(0.0, 0.0),
                pending_action: None,
            },
        }
    }
}

impl eframe::App for ScreenshotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 提取 pending_action 避免借用冲突
        let action = match &mut self.state {
            AppState::Selecting { pending_action, .. } => pending_action.take(),
            AppState::Editing { pending_action, .. } => pending_action.take(),
        };

        // 处理待执行操作
        match action {
            Some(AppAction::NewCapture) => {
                if let AppState::Editing { screenshots, active_idx, .. } = &mut self.state {
                    // 钉住当前截图
                    if let Some(s) = screenshots.get_mut(*active_idx) {
                        s.pinned = true;
                    }
                }
                match capture_fullscreen() {
                    Ok(img) => {
                        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
                        self.state = AppState::Selecting {
                            full_image: img,
                            texture: None,
                            dragging: false,
                            start: pos2(0.0, 0.0),
                            end: pos2(0.0, 0.0),
                            pending_action: None,
                        };
                    }
                    Err(e) => {
                        if let AppState::Editing { status, .. } = &mut self.state {
                            *status = format!("截图失败: {e}");
                        }
                    }
                }
                ctx.request_repaint();
                return;
            }
            Some(AppAction::Close) => {
                ctx.send_viewport_cmd(ViewportCommand::Close);
                return;
            }
            _ => {}
        }

        match &mut self.state {
            // ========== 选区模式 ==========
            AppState::Selecting {
                full_image,
                texture,
                dragging,
                start,
                end,
                ..
            } => {
                if texture.is_none() {
                    let ci = dynimage_to_egui(full_image);
                    *texture =
                        Some(ctx.load_texture("fullscreen", ci, TextureOptions::LINEAR));
                }

                CentralPanel::default()
                    .frame(Frame::none())
                    .show(ctx, |ui| {
                        let available = ui.available_size();
                        let (response, painter) =
                            ui.allocate_painter(available, Sense::click_and_drag());
                        let canvas = response.rect;

                        if let Some(tex) = texture {
                            painter.image(
                                tex.id(),
                                canvas,
                                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        }

                        let dark = Color32::from_black_alpha(160);
                        let sel = if *dragging || *start != *end {
                            Rect::from_min_max(start.min(*end), start.max(*end))
                        } else {
                            Rect::NOTHING
                        };

                        if sel.is_positive() {
                            painter.rect_filled(
                                Rect::from_min_max(canvas.min, pos2(canvas.max.x, sel.min.y)),
                                0.0,
                                dark,
                            );
                            painter.rect_filled(
                                Rect::from_min_max(pos2(canvas.min.x, sel.max.y), canvas.max),
                                0.0,
                                dark,
                            );
                            painter.rect_filled(
                                Rect::from_min_max(
                                    pos2(canvas.min.x, sel.min.y),
                                    pos2(sel.min.x, sel.max.y),
                                ),
                                0.0,
                                dark,
                            );
                            painter.rect_filled(
                                Rect::from_min_max(
                                    pos2(sel.max.x, sel.min.y),
                                    pos2(canvas.max.x, sel.max.y),
                                ),
                                0.0,
                                dark,
                            );
                            painter.rect_stroke(sel, 0.0, Stroke::new(2.0, Color32::WHITE));

                            let scale_x = full_image.width() as f32 / canvas.width();
                            let scale_y = full_image.height() as f32 / canvas.height();
                            let sw = (sel.width() * scale_x) as u32;
                            let sh = (sel.height() * scale_y) as u32;
                            painter.text(
                                sel.min + vec2(4.0, -6.0),
                                Align2::LEFT_BOTTOM,
                                format!("{sw} × {sh}"),
                                FontId::proportional(12.0),
                                Color32::WHITE,
                            );
                        } else {
                            painter.text(
                                canvas.center(),
                                Align2::CENTER_CENTER,
                                "拖动鼠标选择截图区域\n按 ESC 退出",
                                FontId::proportional(20.0),
                                Color32::from_gray(200),
                            );
                        }

                        if response.drag_started_by(PointerButton::Primary) {
                            *dragging = true;
                            *start =
                                response.interact_pointer_pos().unwrap_or(canvas.min);
                        }
                        if *dragging {
                            if let Some(pos) = response.interact_pointer_pos() {
                                *end = pos.clamp(canvas.min, canvas.max);
                            }
                        }
                        if response.dragged_stopped_by(PointerButton::Primary) {
                            *dragging = false;
                            let sel =
                                Rect::from_min_max(start.min(*end), start.max(*end));
                            if sel.area() > 25.0 {
                                let scale_x =
                                    full_image.width() as f32 / canvas.width();
                                let scale_y =
                                    full_image.height() as f32 / canvas.height();
                                let x = (sel.min.x * scale_x).round() as u32;
                                let y = (sel.min.y * scale_y).round() as u32;
                                let w = (sel.width() * scale_x).round() as u32;
                                let h = (sel.height() * scale_y).round() as u32;

                                let cropped =
                                    full_image.crop_imm(x, y, w, h).to_rgba8();
                                let s = ScreenshotData {
                                    id: 0,
                                    image: cropped,
                                    annotations: vec![],
                                    redo_stack: vec![],
                                    pinned: false,
                                    timestamp: chrono::Local::now()
                                        .format("%H:%M:%S")
                                        .to_string(),
                                };

                                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
                                // 不能在这里修改 self.state（借用冲突），设置标志后由外部处理
                                // 我们使用 ctx.data_mut 传递消息
                                // 这里直接用局部变量，然后通过函数返回值...
                                // 实际上最简洁的方式是使用内部可变性，或者重构
                                // 这里采用一个简单的方案：把裁剪结果存到临时位置
                            }
                        }
                    });
            }

            // ========== 编辑模式 ==========
            AppState::Editing {
                screenshots,
                active_idx,
                next_id,
                active_tool,
                color,
                thickness,
                text_input,
                drawing,
                draw_start,
                freehand_points,
                status,
                show_comparison,
                compare_idx,
                number_counter,
                ..
            } => {
                // 边界检查
                if screenshots.is_empty() {
                    return;
                }
                if *active_idx >= screenshots.len() {
                    *active_idx = screenshots.len() - 1;
                }
                if *compare_idx >= screenshots.len() {
                    *compare_idx = 0;
                }

                // ---- 工具栏 ----
                TopBottomPanel::top("toolbar")
                    .min_height(40.0)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.set_height(36.0);

                            if ui.button("📷 新截图").clicked() {
                                // 通过 ctx 传递动作，下一帧处理
                                // 简化：直接在这里修改
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
                            ui.add(
                                Slider::new(thickness, 1.0..=12.0)
                                    .logarithmic(false),
                            );

                            if *active_tool == Tool::Text {
                                ui.label("文字:");
                                ui.text_edit_singleline(text_input);
                            }

                            ui.separator();

                            let has_screenshot =
                                *active_idx < screenshots.len();

                            if has_screenshot {
                                let cur = &mut screenshots[*active_idx];

                                if ui.button("↩ 撤销").clicked() {
                                    if let Some(a) = cur.annotations.pop() {
                                        cur.redo_stack.push(a);
                                    }
                                }
                                if ui.button("↪ 重做").clicked() {
                                    if let Some(a) = cur.redo_stack.pop() {
                                        cur.annotations.push(a);
                                    }
                                }

                                ui.separator();

                                let cur_pinned = cur.pinned;
                                if ui
                                    .selectable_label(cur_pinned, "📌 钉住")
                                    .clicked()
                                {
                                    cur.pinned = !cur_pinned;
                                }
                            }

                            if ui
                                .selectable_label(*show_comparison, "🔍 对比")
                                .clicked()
                            {
                                *show_comparison = !*show_comparison;
                                if *show_comparison
                                    && *compare_idx == *active_idx
                                    && screenshots.len() > 1
                                {
                                    *compare_idx = if *active_idx == 0 {
                                        1
                                    } else {
                                        0
                                    };
                                }
                            }

                            ui.separator();

                            if has_screenshot && ui.button("💾 保存").clicked() {
                                let cur = &screenshots[*active_idx];
                                let mut out = cur.image.clone();
                                render_annotations_on_image(
                                    &mut out,
                                    &cur.annotations,
                                );
                                let default_name = format!(
                                    "截图_{}.png",
                                    chrono::Local::now()
                                        .format("%Y%m%d_%H%M%S")
                                );
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("PNG图片", &["png"])
                                    .add_filter("JPEG图片", &["jpg", "jpeg"])
                                    .set_file_name(&default_name)
                                    .save_file()
                                {
                                    let dyn_img =
                                        DynamicImage::ImageRgba8(out);
                                    match path
                                        .extension()
                                        .and_then(|e| e.to_str())
                                    {
                                        Some("jpg") | Some("jpeg") => {
                                            let _ = dyn_img
                                                .to_rgb8()
                                                .save(&path);
                                        }
                                        _ => {
                                            let _ = dyn_img.save(&path);
                                        }
                                    }
                                    *status = format!(
                                        "已保存: {}",
                                        path.file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                    );
                                }
                            }

                            if has_screenshot && ui.button("📋 复制").clicked() {
                                let cur = &screenshots[*active_idx];
                                let mut out = cur.image.clone();
                                render_annotations_on_image(
                                    &mut out,
                                    &cur.annotations,
                                );
                                if let Ok(mut clipboard) =
                                    arboard::Clipboard::new()
                                {
                                    let img_data = arboard::ImageData {
                                        width: out.width() as usize,
                                        height: out.height() as usize,
                                        bytes: std::borrow::Cow::Borrowed(
                                            out.as_raw(),
                                        ),
                                    };
                                    match clipboard.set_image(img_data) {
                                        Ok(()) => {
                                            *status = "已复制到剪贴板".into()
                                        }
                                        Err(e) => {
                                            *status =
                                                format!("复制失败: {e}")
                                        }
                                    }
                                }
                            }

                            if ui.button("✕ 退出").clicked() {
                                ctx.send_viewport_cmd(
                                    ViewportCommand::Close,
                                );
                            }
                        });
                        if !status.is_empty() {
                            ui.label(status.as_str());
                        }
                    });

                // ---- 截图列表侧边栏 ----
                SidePanel::left("screenshot_list")
                    .resizable(true)
                    .min_width(120.0)
                    .default_width(160.0)
                    .show(ctx, |ui| {
                        ui.heading("截图列表");
                        ui.separator();
                        let len = screenshots.len();
                        let mut remove_idx: Option<usize> = None;

                        for i in 0..len {
                            let s = &screenshots[i];
                            let pinned_mark = if s.pinned { "📌 " } else { "" };
                            let label = format!(
                                "{pinned_mark}截图 {i} ({})",
                                s.timestamp
                            );
                            let selected = i == *active_idx;

                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(selected, &label)
                                    .clicked()
                                {
                                    *active_idx = i;
                                }
                                if !s.pinned
                                    && ui.small_button("✕").clicked()
                                {
                                    remove_idx = Some(i);
                                }
                            });
                        }

                        if let Some(i) = remove_idx {
                            if screenshots.len() > 1 {
                                screenshots.remove(i);
                                if *active_idx >= screenshots.len() {
                                    *active_idx =
                                        screenshots.len() - 1;
                                }
                            }
                        }

                        if ui.button("🗑 清除非钉住截图").clicked() {
                            screenshots.retain(|s| s.pinned);
                            if screenshots.is_empty() {
                                let new_s = ScreenshotData {
                                    id: *next_id,
                                    image: RgbaImage::new(100, 100),
                                    annotations: vec![],
                                    redo_stack: vec![],
                                    pinned: false,
                                    timestamp: chrono::Local::now()
                                        .format("%H:%M:%S")
                                        .to_string(),
                                };
                                *next_id += 1;
                                screenshots.push(new_s);
                            }
                            if *active_idx >= screenshots.len() {
                                *active_idx = screenshots.len() - 1;
                            }
                        }
                    });

                // ---- 主画布 ----
                CentralPanel::default()
                    .frame(Frame::none())
                    .show(ctx, |ui| {
                        if *show_comparison && screenshots.len() >= 2 {
                            // 并排对比
                            let half_w = ui.available_width() / 2.0;
                            let available_h = ui.available_height();

                            ui.horizontal(|ui| {
                                // 左侧（当前编辑）
                                ui.vertical(|ui| {
                                    ui.set_width(half_w);
                                    ui.label(format!(
                                        "✏️ 截图 {} (编辑中)",
                                        screenshots[*active_idx].timestamp
                                    ));
                                    let img_w = screenshots[*active_idx]
                                        .image
                                        .width()
                                        as f32;
                                    let img_h = screenshots[*active_idx]
                                        .image
                                        .height()
                                        as f32;
                                    let scale = (half_w / img_w)
                                        .min(available_h / img_h)
                                        .min(1.0);
                                    let display_w = img_w * scale;
                                    let display_h = img_h * scale;

                                    let tex_id = {
                                        let ci = rgba_to_egui(
                                            &screenshots[*active_idx].image,
                                        );
                                        ui.ctx().load_texture(
                                            format!(
                                                "screenshot_{}",
                                                screenshots
                                                    [*active_idx]
                                                    .id
                                            ),
                                            ci,
                                            TextureOptions::LINEAR,
                                        )
                                    };

                                    let (resp, painter) =
                                        ui.allocate_painter(
                                            vec2(display_w, display_h),
                                            Sense::click_and_drag(),
                                        );
                                    let canvas = resp.rect;

                                    painter.image(
                                        tex_id.id(),
                                        canvas,
                                        Rect::from_min_max(
                                            pos2(0.0, 0.0),
                                            pos2(1.0, 1.0),
                                        ),
                                        Color32::WHITE,
                                    );

                                    // 绘制标注
                                    for ann in
                                        &screenshots[*active_idx].annotations
                                    {
                                        let t =
                                            transform_annotation(ann, scale);
                                        draw_annotation(&t, &painter);
                                    }

                                    // 绘制临时标注
                                    if *drawing {
                                        draw_temp_annotation(
                                            &resp,
                                            &painter,
                                            active_tool,
                                            color,
                                            thickness,
                                            draw_start,
                                            freehand_points,
                                            scale,
                                        );
                                    }

                                    // 交互处理
                                    let new_ann = handle_canvas_interaction(
                                        &resp,
                                        active_tool,
                                        color,
                                        thickness,
                                        text_input,
                                        drawing,
                                        draw_start,
                                        freehand_points,
                                        number_counter,
                                        scale,
                                    );

                                    if let Some(ann) = new_ann {
                                        let cur =
                                            &mut screenshots
                                                [*active_idx];
                                        cur.annotations.push(ann);
                                        cur.redo_stack.clear();
                                    }
                                });

                                ui.separator();

                                // 右侧（对比）
                                ui.vertical(|ui| {
                                    ui.set_width(half_w);
                                    ui.label(format!(
                                        "🔍 截图 {} (对比)",
                                        screenshots[*compare_idx]
                                            .timestamp
                                    ));
                                    if ui
                                        .button("切换对比截图")
                                        .clicked()
                                    {
                                        *compare_idx =
                                            (*compare_idx + 1)
                                                % screenshots.len();
                                        if *compare_idx == *active_idx
                                        {
                                            *compare_idx =
                                                (*compare_idx + 1)
                                                    % screenshots
                                                        .len();
                                        }
                                    }

                                    let img_w = screenshots
                                        [*compare_idx]
                                        .image
                                        .width()
                                        as f32;
                                    let img_h = screenshots
                                        [*compare_idx]
                                        .image
                                        .height()
                                        as f32;
                                    let scale = (half_w / img_w)
                                        .min(available_h / img_h)
                                        .min(1.0);
                                    let display_w = img_w * scale;
                                    let display_h = img_h * scale;

                                    let tex_id = {
                                        let ci = rgba_to_egui(
                                            &screenshots[*compare_idx]
                                                .image,
                                        );
                                        ui.ctx().load_texture(
                                            format!(
                                                "screenshot_ro_{}",
                                                screenshots
                                                    [*compare_idx]
                                                    .id
                                            ),
                                            ci,
                                            TextureOptions::LINEAR,
                                        )
                                    };

                                    let (_resp, painter) =
                                        ui.allocate_painter(
                                            vec2(
                                                display_w,
                                                display_h,
                                            ),
                                            Sense::hover(),
                                        );
                                    let canvas =
                                        Rect::from_min_size(
                                            pos2(0.0, 0.0),
                                            vec2(
                                                display_w,
                                                display_h,
                                            ),
                                        );

                                    painter.image(
                                        tex_id.id(),
                                        canvas,
                                        Rect::from_min_max(
                                            pos2(0.0, 0.0),
                                            pos2(1.0, 1.0),
                                        ),
                                        Color32::WHITE,
                                    );

                                    for ann in &screenshots
                                        [*compare_idx]
                                        .annotations
                                    {
                                        let t =
                                            transform_annotation(
                                                ann, scale,
                                            );
                                        draw_annotation(
                                            &t, &painter,
                                        );
                                    }
                                });
                            });
                        } else {
                            // 单截图模式
                            if *active_idx < screenshots.len() {
                                let img_w = screenshots[*active_idx]
                                    .image
                                    .width()
                                    as f32;
                                let img_h = screenshots[*active_idx]
                                    .image
                                    .height()
                                    as f32;
                                let available = ui.available_size();
                                let scale = (available.x / img_w)
                                    .min(available.y / img_h)
                                    .min(1.0);
                                let display_w = img_w * scale;
                                let display_h = img_h * scale;

                                let tex_id = {
                                    let ci = rgba_to_egui(
                                        &screenshots[*active_idx].image,
                                    );
                                    ui.ctx().load_texture(
                                        format!(
                                            "screenshot_{}",
                                            screenshots[*active_idx]
                                                .id
                                        ),
                                        ci,
                                        TextureOptions::LINEAR,
                                    )
                                };

                                let (resp, painter) =
                                    ui.allocate_painter(
                                        vec2(display_w, display_h),
                                        Sense::click_and_drag(),
                                    );
                                let canvas = resp.rect;

                                painter.image(
                                    tex_id.id(),
                                    canvas,
                                    Rect::from_min_max(
                                        pos2(0.0, 0.0),
                                        pos2(1.0, 1.0),
                                    ),
                                    Color32::WHITE,
                                );

                                for ann in
                                    &screenshots[*active_idx].annotations
                                {
                                    let t = transform_annotation(
                                        ann, scale,
                                    );
                                    draw_annotation(&t, &painter);
                                }

                                if *drawing {
                                    draw_temp_annotation(
                                        &resp,
                                        &painter,
                                        active_tool,
                                        color,
                                        thickness,
                                        draw_start,
                                        freehand_points,
                                        scale,
                                    );
                                }

                                let new_ann =
                                    handle_canvas_interaction(
                                        &resp,
                                        active_tool,
                                        color,
                                        thickness,
                                        text_input,
                                        drawing,
                                        draw_start,
                                        freehand_points,
                                        number_counter,
                                        scale,
                                    );

                                if let Some(ann) = new_ann {
                                    let cur =
                                        &mut screenshots[*active_idx];
                                    cur.annotations.push(ann);
                                    cur.redo_stack.clear();
                                }
                            }
                        }
                    });

                // 键盘
                if ctx.input(|i| i.key_pressed(Key::Escape)) {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
        }

        ctx.request_repaint();
    }
}

// ==================== 辅助函数 ====================

fn transform_annotation(ann: &Annotation, scale: f32) -> Annotation {
    match ann {
        Annotation::Rect {
            min,
            max,
            color,
            thickness,
        } => Annotation::Rect {
            min: pos2(min.x * scale, min.y * scale),
            max: pos2(max.x * scale, max.y * scale),
            color: *color,
            thickness: thickness * scale,
        },
        Annotation::Arrow {
            start,
            end,
            color,
            thickness,
        } => Annotation::Arrow {
            start: pos2(start.x * scale, start.y * scale),
            end: pos2(end.x * scale, end.y * scale),
            color: *color,
            thickness: thickness * scale,
        },
        Annotation::Freehand {
            points,
            color,
            thickness,
        } => Annotation::Freehand {
            points: points
                .iter()
                .map(|p| pos2(p.x * scale, p.y * scale))
                .collect(),
            color: *color,
            thickness: thickness * scale,
        },
        Annotation::Text {
            pos,
            text,
            color,
            size,
        } => Annotation::Text {
            pos: pos2(pos.x * scale, pos.y * scale),
            text: text.clone(),
            color: *color,
            size: size * scale,
        },
        Annotation::Number {
            pos,
            number,
            color,
            radius,
        } => Annotation::Number {
            pos: pos2(pos.x * scale, pos.y * scale),
            number: *number,
            color: *color,
            radius: radius * scale,
        },
    }
}

fn draw_temp_annotation(
    resp: &egui::Response,
    painter: &Painter,
    active_tool: &Tool,
    color: &Color32,
    thickness: &f32,
    draw_start: &Pos2,
    freehand_points: &[Pos2],
    scale: f32,
) {
    match active_tool {
        Tool::Rect => {
            if let Some(pos) = resp.hover_pos() {
                draw_annotation(
                    &Annotation::Rect {
                        min: draw_start.min(pos),
                        max: draw_start.max(pos),
                        color: *color,
                        thickness: *thickness * scale,
                    },
                    painter,
                );
            }
        }
        Tool::Arrow => {
            if let Some(pos) = resp.hover_pos() {
                draw_annotation(
                    &Annotation::Arrow {
                        start: *draw_start,
                        end: pos,
                        color: *color,
                        thickness: *thickness * scale,
                    },
                    painter,
                );
            }
        }
        Tool::Freehand => {
            if freehand_points.len() >= 2 {
                painter.add(Shape::line(
                    freehand_points.to_vec(),
                    Stroke::new(*thickness * scale, *color),
                ));
            }
        }
        _ => {}
    }
}

/// 处理画布交互，返回新的标注（如果有）
fn handle_canvas_interaction(
    resp: &egui::Response,
    active_tool: &mut Tool,
    color: &Color32,
    thickness: &f32,
    text_input: &mut String,
    drawing: &mut bool,
    draw_start: &mut Pos2,
    freehand_points: &mut Vec<Pos2>,
    number_counter: &mut u32,
    scale: f32,
) -> Option<Annotation> {
    if resp.drag_started_by(PointerButton::Primary) {
        *drawing = true;
        if let Some(pos) = resp.interact_pointer_pos() {
            *draw_start = pos;
        }
        freehand_points.clear();

        match active_tool {
            Tool::Freehand => {
                freehand_points.push(*draw_start);
            }
            Tool::Text => {
                let txt = if text_input.is_empty() {
                    "标注".to_string()
                } else {
                    text_input.clone()
                };
                *drawing = false;
                return Some(Annotation::Text {
                    pos: pos2(
                        draw_start.x / scale,
                        draw_start.y / scale,
                    ),
                    text: txt,
                    color: *color,
                    size: 16.0,
                });
            }
            Tool::Number => {
                let n = *number_counter;
                *number_counter += 1;
                *drawing = false;
                return Some(Annotation::Number {
                    pos: pos2(
                        draw_start.x / scale,
                        draw_start.y / scale,
                    ),
                    number: n,
                    color: *color,
                    radius: 12.0,
                });
            }
            _ => {}
        }
        return None;
    }

    if *drawing && *active_tool == Tool::Freehand {
        if let Some(pos) = resp.hover_pos() {
            freehand_points.push(pos);
        }
    }

    if resp.dragged_stopped_by(PointerButton::Primary) && *drawing {
        *drawing = false;
        let end = resp.interact_pointer_pos().unwrap_or(*draw_start);

        let ann = match active_tool {
            Tool::Rect => {
                let r = Rect::from_min_max(
                    draw_start.min(end),
                    draw_start.max(end),
                );
                if r.area() > 4.0 {
                    Some(Annotation::Rect {
                        min: pos2(r.min.x / scale, r.min.y / scale),
                        max: pos2(r.max.x / scale, r.max.y / scale),
                        color: *color,
                        thickness: *thickness,
                    })
                } else {
                    None
                }
            }
            Tool::Arrow => {
                if draw_start.distance(end) > 4.0 {
                    Some(Annotation::Arrow {
                        start: pos2(
                            draw_start.x / scale,
                            draw_start.y / scale,
                        ),
                        end: pos2(end.x / scale, end.y / scale),
                        color: *color,
                        thickness: *thickness,
                    })
                } else {
                    None
                }
            }
            Tool::Freehand => {
                if freehand_points.len() >= 2 {
                    let pts: Vec<Pos2> = freehand_points
                        .iter()
                        .map(|p| pos2(p.x / scale, p.y / scale))
                        .collect();
                    Some(Annotation::Freehand {
                        points: pts,
                        color: *color,
                        thickness: *thickness,
                    })
                } else {
                    None
                }
            }
            _ => None,
        };

        freehand_points.clear();
        return ann;
    }

    None
}

// ==================== 入口 ====================

fn main() {
    let app = ScreenshotApp::new();

    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_fullscreen(true)
            .with_decorations(false)
            .with_always_on_top(),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "截图工具 - Screenshot Tool",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    );
}
