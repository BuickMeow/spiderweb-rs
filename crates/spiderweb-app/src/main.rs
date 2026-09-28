//! Spiderweb（Rust 试验田版）：主程序入口。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod panels;
mod playback;
mod roll;
mod roll_curve;
mod roll_velocity;

fn main() {
    let viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1400.0, 820.0])
        .with_min_inner_size([1000.0, 600.0])
        .with_title("Spiderweb");
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
