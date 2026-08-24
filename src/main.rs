// В release-сборке под Windows прячем чёрное консольное окно.
// В debug оставляем — иначе не увидеть панику и println!.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod document;

use std::path::PathBuf;

use app::MdViewApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    // args_os, а не args: имя файла может быть не-UTF-8, и args() на таком паникует.
    // nth(1) — нулевой аргумент это путь к самому бинарнику.
    let initial: Option<PathBuf> = std::env::args_os().nth(1).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("mdview")
            .with_inner_size([1000.0, 720.0])
            .with_min_inner_size([420.0, 320.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "mdview",
        options,
        // Замыкание, а не готовый объект: приложению нужен контекст egui,
        // который существует только после создания окна.
        // Box<dyn App> — динамическая диспетчеризация: eframe не знает нашего типа.
        Box::new(|cc| Ok(Box::new(MdViewApp::new(cc, initial)))),
    )
}
