#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod tray;
mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(concat!("OPSearch Companion ", env!("CARGO_PKG_VERSION")))
            .with_inner_size(ui::WINDOW_SIZE)
            .with_resizable(false)
            .with_decorations(false)
            .with_icon(ui::window_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "OPSearch Companion",
        options,
        Box::new(|cc| Ok(Box::new(ui::CompanionApp::new(cc)))),
    )
}
