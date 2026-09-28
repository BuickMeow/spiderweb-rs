//! Spiderweb（Rust 试验田版）：主程序入口。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod drawer;
mod drawer_tools;
mod errors;
mod help;
mod help_texts;
mod panels;
mod playback;
mod roll;
mod roll_curve;
mod roll_custom;
mod roll_funnel;
mod roll_live;
mod roll_menu;
mod roll_text;
mod roll_velocity;
#[cfg(test)]
mod test_support;
mod text_dialog;

fn main() {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1400.0, 820.0])
        .with_min_inner_size([1000.0, 600.0])
        .with_title("Spiderweb");
    // 窗口 / 任务栏图标（原版 scripts/icons/icon-256.png）
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "Spiderweb",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    ) {
        eprintln!("Spiderweb 启动失败：{e}");
    }
}
