//! Spiderweb（Rust 试验田版）：主程序入口。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

rust_i18n::i18n!("locales", fallback = "en");

mod app;
mod convert_ui;
mod drawer;
mod drawer_tools;
mod errors;
mod help;
mod help_texts;
#[cfg(test)]
mod i18n_check;
mod join_split;
mod note_gpu;
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
mod snap_picker;
#[cfg(test)]
mod test_support;
mod text_dialog;

fn main() {
    // 目前只有英语一种文案（原版即英语）；以后加语言时在这里换成系统语言
    rust_i18n::set_locale("en");
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
        eprintln!("{}", rust_i18n::t!("app.startup_failed", e = e));
    }
}
