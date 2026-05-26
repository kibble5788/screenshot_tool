use image::{RgbaImage, DynamicImage};
use xcap::Monitor;

pub struct CaptureResult {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>, // RGBA bytes
}

impl CaptureResult {
    /// 转换为 egui 的 ColorImage
    pub fn to_egui_color_image(&self) -> egui::ColorImage {
        egui::ColorImage::from_rgba_unmultiplied(
            [self.width as usize, self.height as usize],
            &self.data,
        )
    }

    /// 裁剪返回 DynamicImage
    pub fn crop(&self, rect: image::math::Rect) -> Option<DynamicImage> {
        let img = RgbaImage::from_raw(self.width, self.height, self.data.clone())?;
        let cropped = DynamicImage::ImageRgba8(img).crop_imm(rect.x, rect.y, rect.width, rect.height);
        Some(cropped)
    }
}

/// 全屏截图
pub fn capture_screen() -> Result<CaptureResult, Box<dyn std::error::Error>> {
    let monitors = Monitor::all()?;
    let primary = monitors.into_iter().next().ok_or("无法获取主显示器")?;
    let image = primary.capture_image()?;
    let (w, h) = (image.width(), image.height());
    let rgba = image.to_rgba8();
    let data = rgba.into_vec();
    Ok(CaptureResult { width: w, height: h, data })
}

/// 窗口截图（通过 xcap 暂时不支持窗口选择，可抓取特定区域）
/// 这里直接使用 Monitor 捕获，窗口选择在 UI 中完成。
pub fn capture_window() -> Result<CaptureResult, Box<dyn std::error::Error>> {
    // 实际上同样可以全屏捕获，然后由用户在选区模式下选择窗口区域
    capture_screen()
}
