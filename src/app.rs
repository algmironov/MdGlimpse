//! Состояние приложения и отрисовка.
//!
//! egui — immediate mode GUI: нет дерева виджетов, которое живёт между кадрами.
//! Каждый кадр функция `update` заново описывает, что должно быть на экране.
//! Кнопка не "существует" — вызов `ui.button(..)` одновременно рисует её
//! и возвращает информацию о том, кликнули ли по ней в этом кадре.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

use crate::document::Document;

/// Ширина колонки текста. Длинные строки на весь монитор читать невозможно.
const MAX_CONTENT_WIDTH: f32 = 900.0;
const RELOAD_INTERVAL: Duration = Duration::from_millis(500);

/// Режим отображения.
///
/// `enum` в Rust — это тип-сумма: значение всегда ровно один из вариантов,
/// третьего состояния не бывает. `Copy` можно вывести, потому что внутри
/// нет данных: такой enum занимает один байт и копируется бесплатно.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Rendered,
    Source,
}

impl ViewMode {
    fn flipped(self) -> Self {
        // `match` обязан покрыть все варианты. Добавите третий режим —
        // компилятор сам приведёт сюда и потребует его обработать.
        match self {
            ViewMode::Rendered => ViewMode::Source,
            ViewMode::Source => ViewMode::Rendered,
        }
    }

    /// Надпись на кнопке — это то, куда переключимся, а не текущее состояние.
    fn button_text(self) -> &'static str {
        match self {
            ViewMode::Rendered => "Исходник",
            ViewMode::Source => "Просмотр",
        }
    }
}

/// Что пользователь попросил сделать в этом кадре.
///
/// Отдельная структура нужна из-за одалживания: пока рисуется тулбар,
/// поля `self` уже одолжены замыканию, вызвать `self.open_dialog()` изнутри
/// нельзя. Собираем намерения, выполняем после.
#[derive(Default)]
struct Actions {
    open: bool,
    toggle: bool,
    reload: bool,
}

pub struct MdViewApp {
    /// `Option`, потому что приложение может быть запущено без файла.
    /// Никакого null: чтобы добраться до Document, придётся разобрать Option.
    document: Option<Document>,
    error: Option<String>,
    mode: ViewMode,
    /// Кэш egui_commonmark: хранит разобранный markdown и загруженные картинки
    /// между кадрами, иначе каждый кадр файл парсился бы заново.
    cache: CommonMarkCache,
    auto_reload: bool,
    last_check: Instant,
    /// Последний отправленный в ОС заголовок окна — чтобы не слать его каждый кадр.
    shown_title: String,
}

impl MdViewApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        // Без этого картинки в markdown не отрисуются вообще.
        egui_extras::install_image_loaders(&cc.egui_ctx);
        cc.egui_ctx.style_mut(|style| style.url_in_tooltip = true);

        let mut app = Self {
            document: None,
            error: None,
            mode: ViewMode::Rendered,
            cache: CommonMarkCache::default(),
            auto_reload: true,
            last_check: Instant::now(),
            shown_title: String::new(),
        };

        if let Some(path) = initial {
            app.open_path(path);
        }

        app
    }

    fn open_path(&mut self, path: PathBuf) {
        match Document::load(&path) {
            Ok(document) => {
                self.error = None;
                self.document = Some(document);
            }
            Err(err) => {
                self.error = Some(format!("Не удалось открыть {}: {err}", path.display()));
            }
        }
    }

    fn open_dialog(&mut self) {
        let picked = rfd::FileDialog::new()
            .set_title("Открыть Markdown")
            .add_filter("Markdown", &["md", "markdown", "mdown", "mkd", "txt"])
            .add_filter("Все файлы", &["*"])
            .pick_file();

        if let Some(path) = picked {
            self.open_path(path);
        }
    }

    fn toolbar(&mut self, ctx: &egui::Context) -> Actions {
        // Деструктуризация: разбираем `self` на независимые ссылки.
        // Дальше замыкание одалживает их по отдельности, а не структуру целиком.
        let Self {
            document,
            mode,
            auto_reload,
            ..
        } = self;

        let mut actions = Actions::default();
        let has_document = document.is_some();

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                if ui
                    .button("Открыть…")
                    .on_hover_text("Ctrl+O, либо перетащите файл в окно")
                    .clicked()
                {
                    actions.open = true;
                }

                let toggle = egui::Button::new(mode.button_text());
                if ui
                    .add_enabled(has_document, toggle)
                    .on_hover_text("Ctrl+E — переключить рендер / исходник")
                    .clicked()
                {
                    actions.toggle = true;
                }

                let reload = egui::Button::new("Перечитать");
                if ui
                    .add_enabled(has_document, reload)
                    .on_hover_text("Ctrl+R")
                    .clicked()
                {
                    actions.reload = true;
                }

                ui.checkbox(auto_reload, "Следить за файлом");

                ui.separator();

                if let Some(document) = document.as_ref() {
                    ui.label(document.title())
                        .on_hover_text(document.path().display().to_string());
                }

                // Переключатель темы прижат к правому краю.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let dark = ui.visuals().dark_mode;
                    if ui.button(if dark { "Светлая" } else { "Тёмная" }).clicked() {
                        let visuals = if dark {
                            egui::Visuals::light()
                        } else {
                            egui::Visuals::dark()
                        };
                        ui.ctx().set_visuals(visuals);
                    }
                });
            });

            ui.add_space(4.0);
        });

        actions
    }

    fn content(&mut self, ctx: &egui::Context) {
        let Self {
            document,
            error,
            mode,
            cache,
            ..
        } = self;

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(error) = error.as_ref() {
                ui.colored_label(ui.visuals().error_fg_color, error);
                ui.separator();
            }

            // let-else: либо файл есть, либо рисуем заглушку и выходим.
            let Some(document) = document.as_ref() else {
                draw_placeholder(ui);
                return;
            };

            match mode {
                ViewMode::Rendered => {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            let width = ui.available_width().min(MAX_CONTENT_WIDTH);
                            ui.set_max_width(width);
                            CommonMarkViewer::new().show(ui, cache, document.rendered());
                        });
                }
                ViewMode::Source => {
                    egui::ScrollArea::both()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            // TextEdit поверх &str работает в режиме "только чтение":
                            // текст можно выделять и копировать, но не менять.
                            let mut source = document.source();
                            ui.add(
                                egui::TextEdit::multiline(&mut source)
                                    .code_editor()
                                    .desired_width(f32::INFINITY)
                                    .frame(false),
                            );
                        });
                }
            }
        });
    }

    fn sync_window_title(&mut self, ctx: &egui::Context) {
        let title = match self.document.as_ref() {
            Some(document) => format!("{} — mdview", document.title()),
            None => "mdview".to_owned(),
        };

        if title != self.shown_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.shown_title = title;
        }
    }
}

fn draw_placeholder(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(80.0);
        ui.heading("Файл не открыт");
        ui.add_space(12.0);
        ui.label("Перетащите .md в окно или нажмите Ctrl+O.");
    });
}

impl eframe::App for MdViewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Клавиатура. `consume_key` не только проверяет нажатие,
        //    но и "съедает" его, чтобы виджеты ниже его не увидели.
        let mut actions = ctx.input_mut(|input| Actions {
            open: input.consume_key(egui::Modifiers::CTRL, egui::Key::O),
            toggle: input.consume_key(egui::Modifiers::CTRL, egui::Key::E),
            reload: input.consume_key(egui::Modifiers::CTRL, egui::Key::R),
        });

        // 2. Drag & drop. Клонируем пути, чтобы не держать блокировку ввода
        //    во время открытия файла.
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });

        if let Some(path) = dropped.into_iter().next() {
            self.open_path(path);
        }

        // 3. Тулбар добавляет свои намерения к тем, что пришли с клавиатуры.
        let from_toolbar = self.toolbar(ctx);
        actions.open |= from_toolbar.open;
        actions.toggle |= from_toolbar.toggle;
        actions.reload |= from_toolbar.reload;

        // 4. Выполняем — теперь `self` снова свободен целиком.
        if actions.open {
            self.open_dialog();
        }
        if actions.toggle {
            self.mode = self.mode.flipped();
        }
        if actions.reload {
            if let Some(document) = self.document.as_mut() {
                if let Err(err) = document.reload() {
                    self.error = Some(format!("Ошибка чтения: {err}"));
                }
            }
        }

        // 5. Слежение за файлом на диске.
        if self.auto_reload && self.document.is_some() {
            if self.last_check.elapsed() >= RELOAD_INTERVAL {
                self.last_check = Instant::now();
                if let Some(document) = self.document.as_mut() {
                    document.reload_if_changed();
                }
            }
            // egui не перерисовывает окно без событий ввода. Просим разбудить
            // нас через полсекунды, иначе изменение файла никто не заметит.
            ctx.request_repaint_after(RELOAD_INTERVAL);
        }

        // 6. Собственно содержимое.
        self.content(ctx);

        // 7. Заголовок окна.
        self.sync_window_title(ctx);
    }
}
