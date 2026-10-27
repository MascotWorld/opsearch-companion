#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(concat!("OPSearch Companion ", env!("CARGO_PKG_VERSION")))
            .with_inner_size([760.0, 680.0])
            .with_min_inner_size([640.0, 480.0])
            .with_icon(ui::window_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "OPSearch Companion",
        options,
        Box::new(|cc| Ok(Box::new(ui::CompanionApp::new(cc)))),
    )
}
