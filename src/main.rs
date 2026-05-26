mod capture;
mod annotation;
mod tools;

use std::sync::Arc;
use egui::{pos2, vec2, Color32, ColorImage, Rect, TextureOptions};
use egui_wgpu::wgpu;
use egui_winit::{egui, EventResponse};
use winit::{
    event::*,
    event_loop::{ControlFlow, EventLoop},
    window::{Fullscreen, WindowBuilder},
};

use annotation::Annotation;
use capture::{capture_screen, capture_window, CaptureResult};
use tools::{Tool, ToolBar};

/// 全局应用状态
enum AppState {
    /// 启动后截取全屏，进入选区模式
    Selecting {
        full_image: CaptureResult,  // 全屏图
        texture_handle: Option<egui::TextureHandle>,
        dark_texture: Option<egui::TextureHandle>,
        selecting: bool,           // 是否正在拖动选区
        start: egui::Pos2,
        end: egui::Pos2,
    },
    /// 已裁剪选区，进入标注模式
    Annotating {
        image: CaptureResult,      // 裁剪后的图
        texture_handle: Option<egui::TextureHandle>,
        annotations: Vec<Annotation>,
        redo_stack: Vec<Annotation>,
        active_tool: Tool,
        color: Color32,
        thickness: f32,
        drawing: Option<Box<dyn FnMut(&egui::Painter, &mut Vec<Annotation>, Rect, &egui::InputState)>>,
        temp_annotation: Option<Annotation>,
        status_message: String,
    },
}

impl AppState {
    fn new() -> Self {
        // 启动时直接截取全屏
        let full = capture_screen().expect("无法截取屏幕");
        AppState::Selecting {
            full_image: full,
            texture_handle: None,
            dark_texture: None,
            selecting: false,
            start: pos2(0.0, 0.0),
            end: pos2(0.0, 0.0),
        }
    }
}

fn main() {
    env_logger::init();
    let event_loop = EventLoop::new().unwrap();
    let window = Arc::new(
        WindowBuilder::new()
            .with_decorations(false)
            .with_always_on_top(true)
            .with_visible(true)
            .with_transparent(false) // 我们通过绘制黑色遮罩实现
            .build(&event_loop)
            .unwrap(),
    );
    // 全屏窗口
    window.set_fullscreen(Some(Fullscreen::Borderless(None)));

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    let surface = instance.create_surface(window.clone()).unwrap();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    })).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format = surface_caps.formats[0];
    let mut surface_config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: window.inner_size().width,
        height: window.inner_size().height,
        present_mode: wgpu::PresentMode::AutoVsync,
        alpha_mode: surface_caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &surface_config);
    let egui_ctx = egui::Context::default();
    let mut egui_winit_state = egui_winit::State::new(
        egui_ctx.clone(),
        egui::viewport::viewport_id(&Arc::clone(&window)),
        &window,
        None,
        None,
        None,
    );
    let mut renderer = egui_wgpu::Renderer::new(&device, surface_format, None, 1);
    let mut app_state = AppState::new();
    let mut paint_jobs = Vec::new();
    let mut screen_descriptor = egui_wgpu::ScreenDescriptor::default();

    event_loop.run(move |event, elwt| {
        elwt.set_control_flow(ControlFlow::Poll);

        let window = Arc::clone(&window);
        let event_response = egui_winit_state.on_window_event(&window, &event);

        if event_response.repaint {
            window.request_redraw();
        }

        match event {
            Event::WindowEvent { event, .. } => {
                match event {
                    WindowEvent::RedrawRequested => {
                        // 重新配置表面大小
                        let size = window.inner_size();
                        if size.width > 0 && size.height > 0 {
                            surface_config.width = size.width;
                            surface_config.height = size.height;
                            surface.configure(&device, &surface_config);
                        }

                        // 获取输入
                        let raw_input = egui_winit_state.take_egui_input(&window);
                        let full_output = egui_ctx.run(raw_input, |ctx| {
                            update_ui(ctx, &mut app_state, &window, &device, &queue);
                        });
                        egui_winit_state.handle_platform_output(&window, full_output.platform_output);
                        let paint_jobs_temp = egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
                        paint_jobs = paint_jobs_temp;

                        // 渲染
                        let mut encoder = device.create_command_encoder(&Default::default());
                        screen_descriptor = egui_wgpu::ScreenDescriptor {
                            size_in_pixels: [surface_config.width, surface_config.height],
                            pixels_per_point: window.scale_factor() as f32,
                        };
                        let out_frame = match surface.get_current_texture() {
                            Ok(frame) => frame,
                            Err(wgpu::SurfaceError::Outdated) => return,
                            Err(e) => { eprintln!("{:?}", e); return; }
                        };
                        let view = out_frame.texture.create_view(&Default::default());
                        renderer.update_buffers(&device, &queue, &mut encoder, &paint_jobs, &screen_descriptor);
                        {
                            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: None,
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: &view,
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            renderer.render(&mut render_pass, &paint_jobs, &screen_descriptor);
                        }
                        queue.submit(std::iter::once(encoder.finish()));
                        out_frame.present();
                    }
                    WindowEvent::CloseRequested => {
                        elwt.exit();
                    }
                    _ => {}
                }
            }
            Event::AboutToWait => {
                window.request_redraw();
            }
            _ => {}
        }
    }).unwrap();
}

fn update_ui(
    ctx: &egui::Context,
    app_state: &mut AppState,
    window: &winit::window::Window,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) {
    match app_state {
        AppState::Selecting { full_image, texture_handle, dark_texture, selecting, start, end } => {
            // 上传纹理（首次）
            if texture_handle.is_none() {
                let rgb = full_image.to_egui_color_image();
                let handle = ctx.load_texture("fullscreen", rgb.clone(), TextureOptions::LINEAR);
                // 创建暗化版本
                let mut dark = rgb.clone();
                for p in dark.pixels.iter_mut() {
                    let c = p.to_array();
                    *p = Color32::from_rgba_premultiplied(
                        (c[0] as f32 * 0.4) as u8,
                        (c[1] as f32 * 0.4) as u8,
                        (c[2] as f32 * 0.4) as u8,
                        c[3],
                    );
                }
                let dark_handle = ctx.load_texture("fullscreen_dark", dark, TextureOptions::LINEAR);
                *texture_handle = Some(handle);
                *dark_texture = Some(dark_handle);
            }

            egui::CentralPanel::default()
                .frame(egui::Frame::none())
                .show(ctx, |ui| {
                    let (response, painter) = ui.allocate_painter(
                        ui.available_size(),
                        egui::Sense::click_and_drag(),
                    );
                    let rect = response.rect;
                    let pointer_pos = response.hover_pos();

                    // 绘制暗化全屏
                    if let Some(dark) = &dark_texture {
                        painter.image(dark.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }

                    // 选区高亮
                    if *selecting && let Some(pos) = pointer_pos {
                        *end = pos;
                    }
                    if *selecting || start != end {
                        let sel = Rect::from_min_max(start.min(*end), start.max(*end));
                        if let Some(tex) = &texture_handle {
                            // 原图选区
                            let uv = Rect::from_min_max(
                                pos2(sel.min.x / rect.width(), sel.min.y / rect.height()),
                                pos2(sel.max.x / rect.width(), sel.max.y / rect.height()),
                            );
                            painter.image(tex.id(), sel, uv, Color32::WHITE);
                        }
                        // 选区边框
                        painter.rect_stroke(sel, 0.0, (1.5, Color32::WHITE));
                    }

                    // 鼠标事件
                    let resp = response.interact(egui::Sense::click_and_drag());
                    if resp.drag_started() {
                        *selecting = true;
                        *start = resp.interact_pointer_pos().unwrap();
                    } else if resp.drag_released() {
                        *selecting = false;
                        // 裁剪
                        let sel = Rect::from_min_max(start.min(*end), start.max(*end));
                        if sel.area() > 10.0 {
                            let scale = full_image.width as f32 / rect.width();
                            let crop_rect = image::math::Rect {
                                x: (sel.min.x * scale).round() as u32,
                                y: (sel.min.y * scale).round() as u32,
                                width: (sel.width() * scale).round() as u32,
                                height: (sel.height() * scale).round() as u32,
                            };
                            if let Some(cropped) = full_image.crop(crop_rect) {
                                // 转换为 CaptureResult
                                let cropped_capture = CaptureResult {
                                    width: cropped.width(),
                                    height: cropped.height(),
                                    data: cropped.into_raw(), // RGBA
                                };
                                *app_state = AppState::Annotating {
                                    image: cropped_capture,
                                    texture_handle: None,
                                    annotations: vec![],
                                    redo_stack: vec![],
                                    active_tool: Tool::Select,
                                    color: Color32::RED,
                                    thickness: 2.0,
                                    drawing: None,
                                    temp_annotation: None,
                                    status_message: String::new(),
                                };
                            }
                        }
                    }
                });
        }
        AppState::Annotating { image, texture_handle, annotations, redo_stack, active_tool, color, thickness, drawing, temp_annotation, status_message } => {
            // 上传纹理
            if texture_handle.is_none() {
                let rgba = image.to_egui_color_image();
                let handle = ctx.load_texture("annotate", rgba, TextureOptions::LINEAR);
                *texture_handle = Some(handle);
            }

            // 如果启动时没有窗口尺寸，重新设置窗口大小
            let img_w = image.width as f32;
            let img_h = image.height as f32;
            let toolbar_height = 40.0;
            let win_size = vec2(img_w, img_h + toolbar_height);
            let cur_size = window.outer_size();
            if cur_size.width != win_size.x as u32 || cur_size.height != win_size.y as u32 {
                let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(
                    win_size.x as u32,
                    win_size.y as u32,
                ));
                window.set_fullscreen(None);
                window.set_decorations(true);
                window.set_always_on_top(false);
                // 等待下一帧
                return;
            }

            egui::TopBottomPanel::top("toolbar").min_height(toolbar_height).show(ctx, |ui| {
                tools::toolbar_ui(ui, active_tool, color, thickness, annotations, redo_stack, status_message);
            });
            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
                let (response, painter) = ui.allocate_painter(
                    vec2(img_w, img_h),
                    egui::Sense::click_and_drag(),
                );
                let rect = response.rect;
                // 绘制背景图
                if let Some(tex) = &texture_handle {
                    painter.image(tex.id(), rect, Rect::from_min_max(pos2(0.0,0.0), pos2(1.0,1.0)), Color32::WHITE);
                }
                // 绘制所有标注
                for ann in annotations.iter() {
                    ann.draw(&painter, rect);
                }
                // 绘制临时标注
                if let Some(temp) = temp_annotation {
                    temp.draw(&painter, rect);
                }
                // 处理工具交互
                tools::handle_tool(
                    &response,
                    &painter,
                    active_tool,
                    color,
                    thickness,
                    annotations,
                    redo_stack,
                    drawing,
                    temp_annotation,
                    status_message,
                );
            });
        }
    }
}
