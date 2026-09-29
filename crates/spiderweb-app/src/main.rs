//! Spiderweb (Rust port experiment): main entry point.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

rust_i18n::i18n!("locales", fallback = "en");

mod app;
mod convert_ui;
mod drawer;
mod drawer_tools;
mod errors;
mod help;
mod help_texts;
mod history;
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
mod tumour_window;

/// `SPIDERWEB_PRESENT=mailbox|fifo|immediate|autovsync|autonovsync` overrides
/// the surface present mode (Mailbox exists on Windows, not on macOS).
fn present_mode_from_env() -> Option<eframe::egui_wgpu::wgpu::PresentMode> {
    use eframe::egui_wgpu::wgpu::PresentMode;
    match std::env::var("SPIDERWEB_PRESENT")
        .ok()?
        .to_ascii_lowercase()
        .as_str()
    {
        "mailbox" => Some(PresentMode::Mailbox),
        "fifo" | "vsync" => Some(PresentMode::Fifo),
        "immediate" => Some(PresentMode::Immediate),
        "autovsync" => Some(PresentMode::AutoVsync),
        "autonovsync" => Some(PresentMode::AutoNoVsync),
        _ => None,
    }
}

fn main() {
    // Only English strings exist for now (the original is English); when more languages are added, switch to the system language here
    rust_i18n::set_locale("en");
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1400.0, 820.0])
        .with_min_inner_size([1000.0, 600.0])
        .with_title("Spiderweb");
    // Window / taskbar icon (upstream scripts/icons/icon-256.png)
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    // Raise `max_buffer_size` to what the adapter supports so projects with tens of millions of
    // notes are not capped at wgpu's 256 MiB default; the note renderer chunks its instance
    // buffers regardless, so a lower limit still works.
    let mut wgpu_setup = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    wgpu_setup.device_descriptor = std::sync::Arc::new(|adapter| {
        use eframe::egui_wgpu::wgpu;
        let base_limits = if adapter.get_info().backend == wgpu::Backend::Gl {
            wgpu::Limits::downlevel_webgl2_defaults()
        } else {
            wgpu::Limits::default()
        };
        wgpu::DeviceDescriptor {
            label: Some("egui wgpu device"),
            required_limits: wgpu::Limits {
                // When using a depth buffer, we have to be able to create a texture large
                // enough for the entire surface, and we want to support 4k+ displays.
                max_texture_dimension_2d: 8192,
                max_buffer_size: adapter.limits().max_buffer_size,
                ..base_limits
            },
            ..Default::default()
        }
    });
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: {
            let mut cfg = eframe::egui_wgpu::WgpuConfiguration {
                wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(wgpu_setup),
                ..Default::default()
            };
            if let Some(mode) = present_mode_from_env() {
                cfg.surface.present_mode = mode;
            }
            if let Ok(latency) = std::env::var("SPIDERWEB_LATENCY")
                && let Ok(n) = latency.parse::<u32>()
            {
                cfg.surface.desired_maximum_frame_latency = Some(n.clamp(1, 3));
            }
            if std::env::var("SPIDERWEB_PERF").is_ok() {
                eprintln!(
                    "[perf] requested present mode {:?}, frame latency {:?}",
                    cfg.surface.present_mode, cfg.surface.desired_maximum_frame_latency
                );
            }
            cfg
        },
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
