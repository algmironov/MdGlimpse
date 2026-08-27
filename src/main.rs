// В release-сборке под Windows прячем чёрное консольное окно.
// В debug оставляем — иначе не увидеть панику и println!.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod document;
mod icons;
mod outline;
mod platform;
mod rendered_probe;
mod search;
mod settings;

use std::path::PathBuf;

use app::MdGlimpseApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    // args_os, а не args: имя файла может быть не-UTF-8, и args() на таком паникует.
    let requested = requested_paths(std::env::args_os());

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("MdGlimpse")
        .with_inner_size([1000.0, 720.0])
        .with_min_inner_size([420.0, 320.0])
        // Настройка только для Windows, хотя по имени этого не скажешь:
        // egui-winit применяет её под `cfg(target_os = "windows")`, а нужна
        // она там из-за COM — winit регистрирует OLE-приёмник перетаскивания.
        // На X11 и Wayland строка молча отбрасывается, и включить или
        // выключить перетаскивание ею нельзя. Если под Linux drag & drop
        // не работает, искать причину надо не здесь: см. README, раздел
        // про Wayland.
        .with_drag_and_drop(true);
    // Иконка заголовка, панели задач и Alt+Tab. Это не та же иконка, что
    // видна у .exe в проводнике: ту встраивает build.rs ресурсом, а эту
    // окну выдаёт сам процесс, и без неё окно получит серую заглушку.
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        // Это не заголовок окна, а идентификатор приложения: от него
        // eframe считает каталог настроек — %APPDATA%\mdglimpse\data.
        // Он обязан отличаться от имени проекта-близнеца: раньше здесь
        // стояло «mdview», и оба приложения делили один каталог в AppData.
        "mdglimpse",
        options,
        // Замыкание, а не готовый объект: приложению нужен контекст egui,
        // который существует только после создания окна.
        // Box<dyn App> — динамическая диспетчеризация: eframe не знает нашего типа.
        Box::new(|cc| Ok(Box::new(MdGlimpseApp::new(cc, &requested)))),
    )
}

/// Пути к файлам из командной строки, в порядке появления.
///
/// Берём ВСЕ аргументы, а не первый: в проводнике можно выделить несколько
/// `.md` и нажать Enter — Windows передаёт их списком. Открыть мы пока
/// умеем один, но промолчать про остальные нельзя, человек решит,
/// что программа их потеряла.
///
/// Функция принимает итератор, а не читает окружение сама, — только ради
/// теста: подсунуть ей выдуманный `argv` иначе было бы нечем.
fn requested_paths<I>(args: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = std::ffi::OsString>,
{
    // skip(1) — нулевой аргумент это путь к самому бинарнику.
    args.into_iter().skip(1).map(PathBuf::from).collect()
}

/// Распаковывает иконку окна из PNG, зашитого в бинарник.
///
/// `include_bytes!` кладёт файл внутрь .exe на этапе компиляции — рядом
/// с программой ничего носить не надо. Ошибку декодирования проглатываем
/// молча: без иконки окно откроется, а падать из-за картинки глупо.
fn window_icon() -> Option<egui::IconData> {
    let png = include_bytes!("../assets/mdglimpse-256.png");
    let image = image::load_from_memory(png).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn argv(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    #[test]
    fn own_path_is_not_a_document() {
        assert!(requested_paths(argv(&["mdglimpse.exe"])).is_empty());
        assert!(requested_paths(argv(&[])).is_empty());
    }

    #[test]
    fn every_argument_survives_in_order() {
        let paths = requested_paths(argv(&["mdglimpse.exe", "a.md", "b.md", "c.md"]));
        let names: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        assert_eq!(names, vec!["a.md", "b.md", "c.md"]);
    }

    #[test]
    fn spaces_and_cyrillic_stay_one_argument() {
        // Проводник передаёт такой путь одним аргументом, и разрезать его
        // мы не имеем права — именно поэтому в команде реестра стоит "%1"
        // в кавычках.
        //
        // Разделитель платформенный, и это не педантизм: под Linux
        // обратная косая — обычный символ имени файла, поэтому
        // `file_name()` у windows-пути честно вернёт его целиком,
        // «C:\Мои файлы\моя заметка.md». Проверяем разбор argv,
        // а не то, как чужая ОС понимает чужие разделители.
        #[cfg(windows)]
        let path = r"C:\Мои файлы\моя заметка.md";
        #[cfg(not(windows))]
        let path = "/home/пользователь/мои файлы/моя заметка.md";

        let paths = requested_paths(argv(&["mdglimpse", path]));
        assert_eq!(paths.len(), 1);
        assert_eq!(
            paths[0].file_name().and_then(|n| n.to_str()),
            Some("моя заметка.md")
        );
    }
}
