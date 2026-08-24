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

use crate::document::{self, Document};
use crate::icons::{self, Icon, Segment};

/// Ширина колонки текста. Длинные строки на весь монитор читать невозможно.
const MAX_CONTENT_WIDTH: f32 = 900.0;

/// Поля центральной панели. Без них текст лип к рамке окна вплотную.
/// Тип `i8` — не описка: `egui::Margin` в 0.36 хранит поля именно так,
/// экономя на размере структуры, которая встречается в каждом виджете.
const CONTENT_MARGIN: egui::Margin = egui::Margin::symmetric(16, 12);
const RELOAD_INTERVAL: Duration = Duration::from_millis(500);

/// Горячие клавиши объявлены один раз как `KeyboardShortcut`, а не строкой
/// в подсказке и отдельно кодом в обработчике: `ctx.format_shortcut`
/// печатает то же самое сочетание, которое реально ловится.
const OPEN_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::O);
const TOGGLE_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::E);
const RELOAD_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::R);
const SELECT_ALL_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::A);
/// Только для таблицы в «Горячих клавишах»: сам Ctrl+C до приложения
/// доходит как `Event::Copy`, а не как нажатие клавиши, и ловится иначе.
const COPY_SHORTCUT_HINT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::C);
/// Скрытое сочетание: галерея иконок. В меню его нет намеренно.
const GALLERY_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::I,
);

/// Сколько висит короткое уведомление внизу окна.
const NOTICE_DURATION: Duration = Duration::from_millis(2500);

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

    /// Номер сегмента в переключателе режимов.
    fn segment(self) -> usize {
        match self {
            ViewMode::Rendered => 0,
            ViewMode::Source => 1,
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
    /// Переключить на противоположный режим — это делает Ctrl+E.
    toggle: bool,
    /// Установить конкретный режим — это делает щелчок по переключателю.
    mode: Option<ViewMode>,
    reload: bool,
    /// Ctrl+A — пометить весь документ выделенным.
    select_all: bool,
    /// Скопировать документ целиком.
    copy_all: bool,
    /// Показать/спрятать отладочную галерею иконок.
    gallery: bool,
    /// Включить или выключить слежение за файлом.
    watch: Option<bool>,
    /// Закрыть приложение.
    quit: bool,
    /// Выбрать тему явно.
    theme: Option<egui::ThemePreference>,
    /// Изменить масштаб интерфейса.
    zoom: Option<Zoom>,
    /// Открыть окно «О программе».
    about: bool,
    /// Открыть окно «Горячие клавиши».
    shortcuts: bool,
}

/// Что делаем с масштабом. Отдельный enum, а не число: «сбросить» —
/// это не «прибавить ноль», и match заставит обработать все три случая.
#[derive(Debug, Clone, Copy)]
enum Zoom {
    In,
    Out,
    Reset,
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
    /// «Выделен весь документ» после Ctrl+A.
    ///
    /// Настоящего выделения через весь текст в виртуализированном списке
    /// быть не может: невидимых строк попросту не существует, выделять
    /// нечего. Поэтому храним намерение флагом, видимые строки подсвечиваем
    /// сами, а копируем из `Document`, а не из того, что нарисовано.
    select_all: bool,
    /// Короткое уведомление внизу окна и момент, когда его показали.
    notice: Option<(String, Instant)>,
    /// Открыта ли отладочная галерея иконок.
    gallery_open: bool,
    /// Открыто ли окно «О программе».
    about_open: bool,
    /// Открыто ли окно «Горячие клавиши».
    shortcuts_open: bool,
}

impl MdViewApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        // Без этого картинки в markdown не отрисуются вообще.
        egui_extras::install_image_loaders(&cc.egui_ctx);
        install_fonts(&cc.egui_ctx);
        // В 0.36 у контекста два независимых Style — для светлой и тёмной темы.
        // Метода style_mut больше нет; all_styles_mut правит оба разом.
        cc.egui_ctx
            .all_styles_mut(|style| style.url_in_tooltip = true);

        let mut app = Self {
            document: None,
            error: None,
            mode: ViewMode::Rendered,
            cache: CommonMarkCache::default(),
            auto_reload: true,
            last_check: Instant::now(),
            shown_title: String::new(),
            select_all: false,
            notice: None,
            gallery_open: false,
            about_open: false,
            shortcuts_open: false,
        };

        if let Some(path) = initial {
            app.open_path(path);
        }

        app
    }

    fn open_path(&mut self, path: PathBuf) {
        match Document::load(&path) {
            Ok(document) => {
                // Исходником открываем в двух случаях. Первый: файл не markdown —
                // рендерить Cargo.toml бессмысленно. Второй: файл слишком велик,
                // и рендер съел бы сотни мегабайт. Оба раза это умолчание,
                // а не запрет: переключиться руками можно всегда.
                self.mode = if document.is_markdown() && !document.is_large() {
                    ViewMode::Rendered
                } else {
                    ViewMode::Source
                };
                self.error = None;
                self.select_all = false;
                self.document = Some(document);
            }
            Err(err) => {
                // `self.document` намеренно не трогаем: ранее открытый файл
                // должен пережить неудачную попытку открыть другой.
                self.error = Some(format!("«{}» — {err}", document::file_name(&path)));
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

    fn toolbar(&mut self, ui: &mut egui::Ui) -> Actions {
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

        // В 0.36 SidePanel и TopBottomPanel слились в один тип Panel,
        // и панель прикрепляется к Ui, а не к Context.
        egui::Panel::top("toolbar").show(ui, |ui| {
            menu_bar(ui, &mut actions, has_document, *mode, *auto_reload);
            ui.separator();

            ui.horizontal(|ui| {
                // Замыкание держит клон Context, а не `ui`: иначе оно одолжило бы
                // `ui` неизменяемо на всю функцию, а он ниже нужен изменяемо.
                // Клон дешёвый — внутри Arc.
                let ctx = ui.ctx().clone();
                let shortcut = |s| ctx.format_shortcut(s);

                if icons::icon_button(
                    ui,
                    Icon::Folder,
                    &format!(
                        "Открыть… ({}), либо перетащите файл в окно",
                        shortcut(&OPEN_SHORTCUT)
                    ),
                )
                .clicked()
                {
                    actions.open = true;
                }

                let combo = shortcut(&TOGGLE_SHORTCUT);
                let segments = [
                    Segment {
                        icon: Icon::Eye,
                        tooltip: &format!("Просмотр ({combo})"),
                    },
                    Segment {
                        icon: Icon::Code,
                        tooltip: &format!("Исходник ({combo})"),
                    },
                ];

                ui.add_enabled_ui(has_document, |ui| {
                    if let Some(index) =
                        crate::icons::segmented_icons(ui, &segments, mode.segment())
                    {
                        actions.mode = Some(if index == 0 {
                            ViewMode::Rendered
                        } else {
                            ViewMode::Source
                        });
                    }
                });

                ui.add_enabled_ui(has_document, |ui| {
                    if icons::icon_button(
                        ui,
                        Icon::Reload,
                        &format!("Перечитать ({})", shortcut(&RELOAD_SHORTCUT)),
                    )
                    .clicked()
                    {
                        actions.reload = true;
                    }
                });

                let watch_hint = if *auto_reload {
                    "Слежение за файлом включено: изменения на диске подхватываются сами"
                } else {
                    "Слежение за файлом выключено: нажмите, чтобы подхватывать изменения"
                };
                icons::icon_toggle(ui, Icon::Waves, auto_reload, watch_hint);

                ui.separator();

                if let Some(document) = document.as_ref() {
                    ui.label(document.title())
                        .on_hover_text(document.path().display().to_string());
                }

                // Переключатель темы прижат к правому краю.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Иконка показывает то состояние, в которое переключит.
                    let dark = ui.visuals().dark_mode;
                    let (icon, hint) = if dark {
                        (Icon::Sun, "Переключить на светлую тему")
                    } else {
                        (Icon::Moon, "Переключить на тёмную тему")
                    };
                    if icons::icon_button(ui, icon, hint).clicked() {
                        // Именно set_theme, а не set_visuals: последний записал бы
                        // светлые визуалы внутрь тёмной темы и сломал бы её насовсем.
                        ui.ctx().set_theme(if dark {
                            egui::Theme::Light
                        } else {
                            egui::Theme::Dark
                        });
                    }
                });
            });

            ui.add_space(4.0);
        });

        actions
    }

    /// Рисует содержимое. Возвращает `true`, если пользователь нажал
    /// «Всё равно отрисовать» на предупреждении о большом файле.
    fn content(&mut self, ui: &mut egui::Ui) -> bool {
        let Self {
            document,
            error,
            mode,
            cache,
            select_all,
            ..
        } = self;
        let select_all = *select_all;

        let mut render_anyway = false;

        let frame = egui::Frame::central_panel(ui.style()).inner_margin(CONTENT_MARGIN);

        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            if let Some(error) = error.as_ref() {
                ui.colored_label(ui.visuals().error_fg_color, error);
                ui.separator();
            }

            // let-else: либо файл есть, либо рисуем заглушку и выходим.
            let Some(document) = document.as_ref() else {
                draw_placeholder(ui);
                return;
            };

            // Предупреждение про большой файл висит, пока он показан исходником.
            // Переключились на рендер — значит человек настоял, и оно не нужно.
            if document.is_large() && *mode == ViewMode::Source {
                render_anyway = draw_large_file_notice(ui, document.size_bytes());
                ui.separator();
            }

            match mode {
                ViewMode::Rendered => {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            centered_column(ui, |ui| {
                                CommonMarkViewer::new().show(ui, cache, document.rendered());
                            });
                        });
                }
                ViewMode::Source => draw_source(ui, document, select_all),
            }
        });

        render_anyway
    }

    /// Кладёт весь текст документа в буфер обмена.
    ///
    /// Копируем из `Document`, а не из нарисованного: на экране в любой
    /// момент есть только видимые строки, и «скопировать всё» через них
    /// было бы враньём.
    fn copy_everything(&mut self, ctx: &egui::Context) {
        let Some(document) = self.document.as_ref() else {
            return;
        };

        let text = document.source();
        let lines = document.line_count();
        let message = format!(
            "Скопировано в буфер обмена: {lines} {}, {} КБ",
            plural(lines, "строка", "строки", "строк"),
            decimal(text.len() as f64 / 1024.0)
        );

        ctx.copy_text(text.to_owned());
        self.notice = Some((message, Instant::now()));
    }

    /// Короткое уведомление внизу окна. Гаснет само.
    fn show_notice(&mut self, ui: &mut egui::Ui) {
        let expired = self
            .notice
            .as_ref()
            .is_some_and(|(_, shown_at)| shown_at.elapsed() >= NOTICE_DURATION);
        if expired {
            self.notice = None;
        }

        let Some((text, shown_at)) = self.notice.as_ref() else {
            return;
        };

        egui::Panel::bottom("notice").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(text).weak());
            });
        });

        // Без этого уведомление провисит до следующего движения мышью:
        // egui не перерисовывает окно, пока ничего не происходит.
        ui.ctx()
            .request_repaint_after(NOTICE_DURATION.saturating_sub(shown_at.elapsed()));
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

/// Подключает вшитые в бинарник шрифты.
///
/// Встроенных в egui Ubuntu-Light и Hack хватало по покрытию кириллицы,
/// но выглядят они простовато. Inter рисовался под интерфейсы и мелкие
/// кегли, JetBrains Mono — под чтение кода; кириллица у обоих родная,
/// а не пристроенная сбоку. Оба под OFL, тексты лицензий лежат рядом
/// со шрифтами в assets/fonts.
///
/// Взята версия JetBrains Mono **без лигатур** (NL). Просмотрщик обязан
/// показывать ровно то, что лежит в файле, а лигатуры склеили бы `!=`
/// в один глиф `≠` — в режиме «Исходник» это прямой обман.
///
/// `include_bytes!` вшивает файлы в бинарник на этапе компиляции: искать
/// шрифты по системным путям нельзя, на другой машине их не окажется.
/// `FontData::from_static` поэтому не копирует байты — они уже лежат
/// в неизменяемой секции исполняемого файла и живут всю программу.
fn install_fonts(ctx: &egui::Context) {
    // Эти три типа egui наружу не переэкспортирует — берём прямо из epaint,
    // который сам доступен как `egui::epaint`.
    use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};

    // Highest, а не замена списка целиком: встроенные шрифты остаются
    // запасными, и эмодзи с редкими символами по-прежнему находятся.
    let priority_in = |family: egui::FontFamily| {
        vec![InsertFontFamily {
            family,
            priority: FontPriority::Highest,
        }]
    };

    ctx.add_font(FontInsert::new(
        "Inter",
        egui::FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf")),
        priority_in(egui::FontFamily::Proportional),
    ));

    ctx.add_font(FontInsert::new(
        "JetBrains Mono",
        egui::FontData::from_static(include_bytes!(
            "../assets/fonts/JetBrainsMonoNL-Regular.ttf"
        )),
        priority_in(egui::FontFamily::Monospace),
    ));
}

/// Окно «О программе».
fn about_window(ctx: &egui::Context, open: &mut bool) {
    egui::Window::new("О программе")
        .open(open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.heading("mdview");
            ui.label(format!("Версия {}", env!("CARGO_PKG_VERSION")));
            ui.add_space(6.0);
            ui.label(env!("CARGO_PKG_DESCRIPTION"));
            ui.add_space(6.0);
            ui.label("Просмотрщик, а не редактор: сохранения и правки текста нет и не будет.");
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(concat!(
                    "Лицензия: ",
                    env!("CARGO_PKG_LICENSE"),
                    ". Шрифты Inter и JetBrains Mono — под OFL."
                ))
                .weak(),
            );
        });
}

/// Окно «Горячие клавиши».
///
/// Список строится из тех же констант, что и обработчики: разъехаться
/// подписи и поведению здесь физически негде.
fn shortcuts_window(ctx: &egui::Context, open: &mut bool) {
    let rows: [(&egui::KeyboardShortcut, &str); 5] = [
        (&OPEN_SHORTCUT, "Открыть файл"),
        (&TOGGLE_SHORTCUT, "Переключить рендер и исходник"),
        (&RELOAD_SHORTCUT, "Перечитать файл с диска"),
        (&SELECT_ALL_SHORTCUT, "Выделить весь документ"),
        (
            &COPY_SHORTCUT_HINT,
            "Копировать выделенное или весь документ",
        ),
    ];

    egui::Window::new("Горячие клавиши")
        .open(open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::Grid::new("shortcuts").num_columns(2).show(ui, |ui| {
                for (shortcut, description) in rows {
                    ui.label(egui::RichText::new(ctx.format_shortcut(shortcut)).monospace());
                    ui.label(description);
                    ui.end_row();
                }
                ui.label(egui::RichText::new("Ctrl + = / − / 0").monospace());
                ui.label("Масштаб интерфейса");
                ui.end_row();
                ui.label(egui::RichText::new("перетаскивание").monospace());
                ui.label("Открыть файл, брошенный в окно");
                ui.end_row();
            });
        });
}

/// Строка меню: Файл, Правка, Вид, Справка.
///
/// Нативного меню Windows тут нет и быть не может: egui рисует интерфейс
/// сам, доступа к Win32-менюбару через winit не предусмотрено. Это меню
/// в стиле VS Code или Blender — нарисованное приложением.
///
/// Меню осталось единственным местом, где действия названы словами:
/// тулбар теперь целиком на иконках.
fn menu_bar(
    ui: &mut egui::Ui,
    actions: &mut Actions,
    has_document: bool,
    mode: ViewMode,
    watching: bool,
) {
    let ctx = ui.ctx().clone();
    // Пункт с подписью сочетания справа — как принято в настольных
    // приложениях. Сочетание берётся из той же константы, что и обработчик.
    let item = |label: &str, shortcut: Option<&egui::KeyboardShortcut>| {
        let button = egui::Button::new(label);
        match shortcut {
            Some(shortcut) => button.shortcut_text(ctx.format_shortcut(shortcut)),
            None => button,
        }
    };

    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("Файл", |ui| {
            if ui.add(item("Открыть…", Some(&OPEN_SHORTCUT))).clicked() {
                actions.open = true;
                ui.close();
            }

            ui.menu_button("Недавние", |ui| {
                // Задел на Этап 2: список недавних файлов появится вместе
                // с сохранением состояния между запусками.
                ui.add_enabled(false, egui::Button::new("пока пусто"));
            });

            if ui
                .add_enabled(has_document, item("Перечитать", Some(&RELOAD_SHORTCUT)))
                .clicked()
            {
                actions.reload = true;
                ui.close();
            }

            ui.separator();

            if ui.add(item("Выход", None)).clicked() {
                actions.quit = true;
                ui.close();
            }
        });

        // «Правка» в исходном списке не значилась — но выделение и копирование
        // по настольным правилам живут именно здесь, а не в «Виде».
        ui.menu_button("Правка", |ui| {
            if ui
                .add_enabled(
                    has_document,
                    item("Выделить всё", Some(&SELECT_ALL_SHORTCUT)),
                )
                .clicked()
            {
                actions.select_all = true;
                ui.close();
            }
            if ui
                .add_enabled(has_document, item("Копировать всё", None))
                .clicked()
            {
                actions.copy_all = true;
                ui.close();
            }
        });

        ui.menu_button("Вид", |ui| {
            let mut rendered = mode == ViewMode::Rendered;
            if ui
                .add_enabled(
                    has_document,
                    egui::Button::selectable(rendered, "Рендер")
                        .shortcut_text(ctx.format_shortcut(&TOGGLE_SHORTCUT)),
                )
                .clicked()
            {
                actions.mode = Some(ViewMode::Rendered);
                ui.close();
            }
            rendered = !rendered;
            if ui
                .add_enabled(
                    has_document,
                    egui::Button::selectable(rendered, "Исходник")
                        .shortcut_text(ctx.format_shortcut(&TOGGLE_SHORTCUT)),
                )
                .clicked()
            {
                actions.mode = Some(ViewMode::Source);
                ui.close();
            }

            ui.separator();

            if ui
                .add(egui::Button::selectable(watching, "Следить за файлом"))
                .clicked()
            {
                actions.watch = Some(!watching);
                ui.close();
            }

            ui.separator();

            ui.menu_button("Тема", |ui| {
                let current = ctx.options(|options| options.theme_preference);
                for (label, preference) in [
                    ("Светлая", egui::ThemePreference::Light),
                    ("Тёмная", egui::ThemePreference::Dark),
                    ("Как в системе", egui::ThemePreference::System),
                ] {
                    if ui
                        .add(egui::Button::selectable(current == preference, label))
                        .clicked()
                    {
                        actions.theme = Some(preference);
                        ui.close();
                    }
                }
            });

            ui.menu_button("Масштаб", |ui| {
                if ui.add(item("Увеличить", None)).clicked() {
                    actions.zoom = Some(Zoom::In);
                    ui.close();
                }
                if ui.add(item("Уменьшить", None)).clicked() {
                    actions.zoom = Some(Zoom::Out);
                    ui.close();
                }
                if ui.add(item("Сбросить", None)).clicked() {
                    actions.zoom = Some(Zoom::Reset);
                    ui.close();
                }
            });
        });

        ui.menu_button("Справка", |ui| {
            if ui.add(item("Горячие клавиши", None)).clicked() {
                actions.shortcuts = true;
                ui.close();
            }
            if ui.add(item("О программе", None)).clicked() {
                actions.about = true;
                ui.close();
            }
        });
    });
}

/// Колонка ограниченной ширины, прижатая к центру.
///
/// `MAX_CONTENT_WIDTH` сам по себе только обрезает ширину, оставляя колонку
/// у левого края. Поэтому слева добавляется отступ в половину остатка,
/// а содержимое рисуется во вложенном `Ui` с обычной вёрсткой сверху вниз —
/// текст внутри колонки должен остаться выключенным влево, а не по центру.
fn centered_column<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let available = ui.available_width();
    let width = available.min(MAX_CONTENT_WIDTH);
    let side = ((available - width) / 2.0).max(0.0);

    ui.horizontal_top(|ui| {
        ui.add_space(side);
        ui.vertical(|ui| {
            ui.set_max_width(width);
            add_contents(ui)
        })
        .inner
    })
    .inner
}

/// Режим «Исходник» с виртуализацией.
///
/// `show_rows` строит только те строки, которые сейчас видны, поэтому
/// потребление памяти не зависит от размера файла. Видимый кусок рисуется
/// одним `Label`, а не строкой на виджет: так внутри окна работает обычное
/// выделение мышью через несколько строк, и высота ряда совпадает
/// с расчётной без возни с межстрочными интервалами.
fn draw_source(ui: &mut egui::Ui, document: &Document, select_all: bool) {
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);

    // Интервал обнуляем ДО вызова: show_rows читает его из этого `ui`,
    // чтобы посчитать общую высоту. Поменять его внутри замыкания — значит
    // разъехаться с собственным расчётом прокрутки.
    ui.spacing_mut().item_spacing.y = 0.0;

    egui::ScrollArea::both().auto_shrink([false; 2]).show_rows(
        ui,
        row_height,
        document.line_count(),
        |ui, rows| {
            let visible: Vec<&str> = rows.map(|index| document.line(index)).collect();
            let mut text = egui::RichText::new(visible.join("\n")).monospace();
            // После Ctrl+A подсвечиваем видимые строки цветом выделения.
            // Это именно индикация: выделен весь документ, а не эти строки.
            if select_all {
                text = text.background_color(ui.visuals().selection.bg_fill);
            }
            ui.add(
                egui::Label::new(text)
                    .selectable(true)
                    .wrap_mode(egui::TextWrapMode::Extend),
            );
        },
    );
}

/// Предупреждение о большом файле. Возвращает `true`, если нажата кнопка.
fn draw_large_file_notice(ui: &mut egui::Ui, size_bytes: u64) -> bool {
    let mut render_anyway = false;

    ui.horizontal_wrapped(|ui| {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!(
                "Файл большой ({} МБ) — показан исходником.",
                decimal(size_bytes as f64 / (1024.0 * 1024.0))
            ),
        );
        ui.label("Рендер такого документа съест сотни мегабайт.");
        if ui
            .button("Всё равно отрисовать")
            .on_hover_text("egui_commonmark строит виджеты для всего документа сразу")
            .clicked()
        {
            render_anyway = true;
        }
    });

    render_anyway
}

/// Русское склонение существительного при числительном.
///
/// В русском три формы, и выбор идёт по последним двум цифрам: 21 строка,
/// но 11 строк; 22 строки, но 12 строк. Формы передаются явно, чтобы
/// функция не знала ничего про конкретные слова.
fn plural(count: usize, one: &'static str, few: &'static str, many: &'static str) -> &'static str {
    let hundreds = count % 100;
    let tens = count % 10;

    if (11..=14).contains(&hundreds) {
        many
    } else if tens == 1 {
        one
    } else if (2..=4).contains(&tens) {
        few
    } else {
        many
    }
}

/// Число с одним знаком после запятой — именно запятой, а не точки.
fn decimal(value: f64) -> String {
    format!("{value:.1}").replace('.', ",")
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
    /// В 0.36 у трейта `App` нет `update(&mut self, ctx, frame)`: приложение
    /// получает корневой `Ui` без полей и фона, а панели прикрепляются к нему.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Клон дешёвый: Context внутри — это Arc. Клон нужен по делу:
        // `ui.ctx()` одолжил бы `ui` неизменяемо, а ниже он нужен изменяемо.
        let ctx = ui.ctx().clone();

        // 1. Клавиатура. `consume_key` не только проверяет нажатие,
        //    но и "съедает" его, чтобы виджеты ниже его не увидели.
        // Есть ли прямо сейчас живое выделение мышью. Плагин, который им
        // заведует, egui регистрирует сам; `unwrap_or(false)` — на случай,
        // если его почему-то нет.
        let has_selection = ctx
            .with_plugin::<egui::text_selection::LabelSelectionState, _>(|state| {
                state.has_selection()
            })
            .unwrap_or(false);
        // Пока не выделено мышью — Ctrl+C наш; иначе пусть копирует egui.
        let copy_is_ours = self.select_all || !has_selection;

        let mut actions = ctx.input_mut(|input| {
            // Ctrl+C до нас не доходит как нажатие клавиши: egui-winit
            // подменяет его на отдельный Event::Copy ещё до обработки ввода.
            // Поэтому ищем именно событие, а `retain` заодно снимает его
            // с egui, чтобы копирование не случилось дважды.
            let copy_all = copy_is_ours && {
                let requested = input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Copy));
                if requested {
                    input
                        .events
                        .retain(|event| !matches!(event, egui::Event::Copy));
                }
                requested
            };

            Actions {
                open: input.consume_shortcut(&OPEN_SHORTCUT),
                toggle: input.consume_shortcut(&TOGGLE_SHORTCUT),
                reload: input.consume_shortcut(&RELOAD_SHORTCUT),
                select_all: input.consume_shortcut(&SELECT_ALL_SHORTCUT),
                copy_all,
                gallery: input.consume_shortcut(&GALLERY_SHORTCUT),
                // Остальные намерения приходят только из меню.
                ..Actions::default()
            }
        });

        // 2. Drag & drop. Клонируем пути, чтобы не держать блокировку ввода
        //    во время открытия файла.
        // `path` в 0.36 — метод трейта `DroppedFile`, а не поле, и возвращает
        // `&Path` без Option: путь есть всегда (на вебе — только имя файла).
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });

        if let Some(path) = dropped.into_iter().next() {
            self.open_path(path);
        }

        // 3. Тулбар добавляет свои намерения к тем, что пришли с клавиатуры.
        let from_toolbar = self.toolbar(ui);
        actions.open |= from_toolbar.open;
        actions.toggle |= from_toolbar.toggle;
        actions.mode = actions.mode.or(from_toolbar.mode);
        actions.reload |= from_toolbar.reload;
        actions.select_all |= from_toolbar.select_all;
        actions.copy_all |= from_toolbar.copy_all;
        actions.watch = actions.watch.or(from_toolbar.watch);
        actions.quit |= from_toolbar.quit;
        actions.theme = actions.theme.or(from_toolbar.theme);
        actions.zoom = actions.zoom.or(from_toolbar.zoom);
        actions.about |= from_toolbar.about;
        actions.shortcuts |= from_toolbar.shortcuts;

        // 4. Выполняем — теперь `self` снова свободен целиком.
        if actions.open {
            self.open_dialog();
        }
        if actions.toggle {
            self.mode = self.mode.flipped();
        }
        if let Some(mode) = actions.mode {
            self.mode = mode;
        }
        if actions.select_all && self.document.is_some() {
            self.select_all = true;
        }
        if actions.copy_all {
            self.copy_everything(&ctx);
        }
        if actions.gallery {
            self.gallery_open = !self.gallery_open;
        }
        if let Some(watch) = actions.watch {
            self.auto_reload = watch;
        }
        if let Some(preference) = actions.theme {
            ctx.set_theme(preference);
        }
        if let Some(zoom) = actions.zoom {
            // Тот же механизм, что у встроенных Ctrl+плюс и Ctrl+минус.
            let factor = match zoom {
                Zoom::In => (ctx.zoom_factor() * 1.1).min(4.0),
                Zoom::Out => (ctx.zoom_factor() / 1.1).max(0.5),
                Zoom::Reset => 1.0,
            };
            ctx.set_zoom_factor(factor);
        }
        if actions.about {
            self.about_open = true;
        }
        if actions.shortcuts {
            self.shortcuts_open = true;
        }
        if actions.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if actions.reload {
            if let Some(document) = self.document.as_mut() {
                if let Err(err) = document.reload() {
                    self.error = Some(format!("«{}» — {err}", document.title()));
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

        // 6. Уведомление — до содержимого: центральная панель занимает
        //    всё, что осталось, поэтому её добавляют последней.
        self.show_notice(ui);

        // 7. Собственно содержимое.
        if self.content(ui) {
            self.mode = ViewMode::Rendered;
        }

        self.sync_window_title(&ctx);

        // Окна показываем после панелей: панели делят между собой площадь
        // кадра, окна кладутся поверх готовой раскладки.
        if self.gallery_open {
            icons::gallery(&ctx, &mut self.gallery_open);
        }
        if self.about_open {
            about_window(&ctx, &mut self.about_open);
        }
        if self.shortcuts_open {
            shortcuts_window(&ctx, &mut self.shortcuts_open);
        }

        // 8. Любой щелчок мышью снимает пометку «выделено всё»: дальше
        //    человек уже выделяет сам, и две подсветки сразу только мешают.
        if self.select_all && ctx.input(|input| input.pointer.any_pressed()) {
            self.select_all = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plural_follows_russian_rules() {
        let form = |n| plural(n, "строка", "строки", "строк");

        assert_eq!(form(1), "строка");
        assert_eq!(form(2), "строки");
        assert_eq!(form(5), "строк");
        assert_eq!(form(21), "строка");
        assert_eq!(form(22), "строки");
        // Подвох, ради которого функция и написана: вторая цифра решает
        // не всегда — с одиннадцати по четырнадцать всегда «строк».
        assert_eq!(form(11), "строк");
        assert_eq!(form(12), "строк");
        assert_eq!(form(14), "строк");
        assert_eq!(form(111), "строк");
        assert_eq!(form(121), "строка");
        assert_eq!(form(0), "строк");
    }

    #[test]
    fn decimal_uses_comma() {
        assert_eq!(decimal(2.34), "2,3");
        assert_eq!(decimal(5.0), "5,0");
    }
}
