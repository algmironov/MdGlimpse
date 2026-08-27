//! DIAG-DND-TEMP — временный диагностический слой. Не часть приложения.
//!
//! Отвечает ровно на два вопроса, ради которых собран:
//!  1. доходят ли до приложения события перетаскивания файлов;
//!  2. на каком графическом адаптере оно рисует.
//!
//! Удаляется целиком: `rm src/diag.rs` плюс три строки с маркером
//! `DIAG-DND-TEMP` — одна в `main.rs` и две в `app.rs`. Все три находит
//! `grep -rn DIAG-DND-TEMP src/`.
//!
//! Печать через `eprintln!`, а не через крейт логирования: слой живёт
//! несколько дней, тянуть ради него зависимость незачем. И он не спрятан
//! за `#[cfg(debug_assertions)]` — проверять надо именно release-сборку,
//! ту самую, что запускается на виртуальной машине.
//!
//! Модуль жёстко завязан на wgpu-бэкенд (`cc.wgpu_render_state`). Это
//! осознанно: eframe 0.36 берёт wgpu по умолчанию, а закладываться на
//! возможную смену бэкенда во временном коде — лишняя работа.

use std::cell::RefCell;

use eframe::egui;

thread_local! {
    /// Список наведённых файлов, о котором мы уже сообщили.
    ///
    /// Нужен, потому что `hovered_files` — не событие: `RawInput::take`
    /// в egui 0.36 этот список **копирует**, а не забирает (в отличие от
    /// `dropped_files`, см. `raw_input.rs:138-139`). Пока файл висит над
    /// окном, он приходит в каждом кадре, и egui при этом держит
    /// непрерывную перерисовку. Без этой памяти в лог сыпалось бы
    /// по шестьдесят одинаковых строк в секунду.
    ///
    /// Состояние снаружи структуры приложения — сознательное исключение
    /// из правила «всё состояние в структуре приложения»: патч должен
    /// сниматься одним движением, а не вычищаться ещё и из полей
    /// `MdGlimpseApp`.
    static REPORTED_HOVER: RefCell<Vec<egui::HoveredFile>> = const { RefCell::new(Vec::new()) };
}

/// Однократный отчёт при запуске: окружение и графический адаптер.
///
/// Зовётся из `MdGlimpseApp::new`, то есть уже после того, как eframe создал
/// окно и устройство wgpu. Если этих строк в выводе нет вовсе — значит,
/// инициализация до них не дошла: `run_native` вернул `Err`, и Rust напечатал
/// причину строкой «Error: ...».
pub fn startup(cc: &eframe::CreationContext<'_>) {
    // Эти четыре переменные решают, какой бэкенд возьмёт winit 0.30:
    // непустая WAYLAND_DISPLAY или WAYLAND_SOCKET — Wayland, иначе непустая
    // DISPLAY — X11. Печатаем их, чтобы в логе было видно, что именно
    // проверял winit, а не что мы предположили.
    eprintln!(
        "[DIAG-ENV] XDG_SESSION_TYPE = {}",
        env_value("XDG_SESSION_TYPE")
    );
    eprintln!(
        "[DIAG-ENV] WAYLAND_DISPLAY  = {}",
        env_value("WAYLAND_DISPLAY")
    );
    eprintln!(
        "[DIAG-ENV] WAYLAND_SOCKET   = {}",
        env_value("WAYLAND_SOCKET")
    );
    eprintln!("[DIAG-ENV] DISPLAY          = {}", env_value("DISPLAY"));

    match cc.wgpu_render_state.as_ref() {
        // Собрано с фичей wgpu, а состояния нет — значит, рисует не wgpu.
        None => eprintln!("[DIAG-GPU] wgpu_render_state отсутствует: рендерер не wgpu"),
        Some(state) => {
            let info = state.adapter.get_info();
            eprintln!("[DIAG-GPU] выбран: {info:?}");
            if info.device_type == eframe::wgpu::DeviceType::Cpu {
                eprintln!(
                    "[DIAG-GPU] это программный растеризатор (llvmpipe/lavapipe): \
                     аппаратного ускорения у машины нет"
                );
            }
            // Полный список — чтобы отличить «GPU не нашёлся вовсе»
            // от «нашёлся, но wgpu предпочёл ему программный».
            for (index, adapter) in state.available_adapters.iter().enumerate() {
                let info = adapter.get_info();
                eprintln!("[DIAG-GPU] доступен [{index}]: {info:?}");
            }
        }
    }
}

/// Отчёт о перетаскивании. Зовётся каждый кадр, печатает — только когда есть
/// о чём: список наведённых файлов изменился либо что-то бросили в окно.
pub fn drag_and_drop(ctx: &egui::Context) {
    let (hovered, dropped) = ctx.input(|input| {
        // Читаем именно `raw`, а не разобранное состояние: вопрос в том,
        // доехали ли события до egui вообще, а не как он их истолковал.
        let hovered = input.raw.hovered_files.clone();
        let dropped: Vec<String> = input
            .raw
            .dropped_files
            .iter()
            .map(|file| file.path().display().to_string())
            .collect();
        (hovered, dropped)
    });

    REPORTED_HOVER.with_borrow_mut(|reported| {
        if *reported == hovered {
            return;
        }
        if hovered.is_empty() {
            // Опустевший список — это либо HoveredFileCancelled, либо
            // состоявшийся drop. Строка одна на весь жест, потоком она
            // не станет, а без неё не отличить «курсор увели» от
            // «событие потерялось по дороге».
            eprintln!("[DIAG-DND] hovered_files опустел");
        } else {
            for (index, file) in hovered.iter().enumerate() {
                eprintln!("[DIAG-DND] hovered_files[{index}] = {file:?}");
            }
        }
        *reported = hovered;
    });

    for (index, path) in dropped.iter().enumerate() {
        eprintln!("[DIAG-DND] dropped_files[{index}] = {path}");
    }
}

/// Значение переменной окружения в печатном виде.
///
/// `var_os`, а не `var`: под Linux в переменной может лежать не-UTF-8,
/// и `var` вернула бы на этом ту же ошибку, что и для отсутствующей
/// переменной, — то есть соврала бы ровно там, где мы её и спрашиваем.
fn env_value(name: &str) -> String {
    match std::env::var_os(name) {
        // Кавычки не украшение: для winit пустая строка и отсутствие
        // переменной — один и тот же случай, а в голом выводе их не различить.
        Some(value) => format!("«{}»", value.to_string_lossy()),
        None => "не задана".to_owned(),
    }
}
