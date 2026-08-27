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
use crate::platform;
use crate::rendered_probe::{self, ProbeBroken};
use crate::search;
use crate::settings::{self, Settings};
use std::ops::Range;

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
const OUTLINE_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::B);

const OUTLINE_DEFAULT_WIDTH: f32 = 240.0;

const FIND_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::F);

/// Состояние поиска.
///
/// Совпадения считаются по разному тексту в разных режимах: в «Исходнике» —
/// по исходнику, в «Рендере» — по извлечённому простому тексту. Поэтому
/// подготовленный `Haystack` помнит, для какого режима он сделан,
/// и пересобирается при переключении.
#[derive(Default)]
struct Search {
    open: bool,
    query: String,
    options: search::Options,
    matches: Vec<Range<usize>>,
    /// Номер текущего совпадения. Осмыслен, только если `matches` не пуст.
    current: usize,
    prepared: Option<Prepared>,
    /// Поставить фокус в поле на следующем кадре.
    request_focus: bool,
    /// Сколько совпадений удалось подсветить в «Рендере».
    ///
    /// Меньше общего числа, если совпадение разрезано разметкой:
    /// такой фрагмент рисуется двумя разными galley, и найти его целиком
    /// в нарисованном невозможно.
    highlighted: usize,
    /// Чтение слоя графики перестало работать. См. `rendered_probe`.
    probe_broken: bool,
    /// Входные данные, для которых уже посчитаны совпадения.
    ///
    /// Без этого `refresh` пересчитывал бы всё каждый кадр — около двух
    /// миллисекунд на файле 5 МБ, то есть восьмая часть кадрового бюджета
    /// впустую, пока открыт поиск.
    computed_for: Option<(String, search::Options, ViewMode)>,
}

struct Prepared {
    mode: ViewMode,
    /// Простой текст для «Рендера». Для «Исходника» пуст: там ищем прямо
    /// по `document.source()`, лишняя копия не нужна.
    plain: String,
    haystack: search::Haystack,
}

impl Search {
    /// Текст, по которому идёт поиск в текущем режиме.
    fn text<'a>(&'a self, document: &'a Document) -> &'a str {
        match self.prepared.as_ref() {
            Some(prepared) if prepared.mode == ViewMode::Rendered => &prepared.plain,
            _ => document.source(),
        }
    }

    /// Пересчитывает совпадения, если что-то изменилось.
    ///
    /// Подготовка текста (свёртка регистра) стоит около 28 мс на файле 5 МБ,
    /// поэтому делается только при смене документа или режима, а не на каждое
    /// нажатие: сам поиск после неё занимает около двух миллисекунд.
    fn refresh(&mut self, document: &Document, mode: ViewMode) {
        let inputs = (self.query.clone(), self.options, mode);
        if self.computed_for.as_ref() == Some(&inputs) {
            return;
        }
        self.computed_for = Some(inputs);

        let needs_prepare = self
            .prepared
            .as_ref()
            .is_none_or(|prepared| prepared.mode != mode);

        if needs_prepare {
            let plain = match mode {
                ViewMode::Rendered => crate::outline::plain_text(document.source()),
                ViewMode::Source => String::new(),
            };
            let haystack = match mode {
                ViewMode::Rendered => search::Haystack::new(&plain),
                ViewMode::Source => search::Haystack::new(document.source()),
            };
            self.prepared = Some(Prepared {
                mode,
                plain,
                haystack,
            });
        }

        let Some(prepared) = self.prepared.as_ref() else {
            return;
        };
        let text = match mode {
            ViewMode::Rendered => prepared.plain.as_str(),
            ViewMode::Source => document.source(),
        };

        self.matches = prepared.haystack.find_all(text, &self.query, self.options);
        self.current = self.current.min(self.matches.len().saturating_sub(1));
    }

    /// Сбрасывает подготовку — например, когда открыли другой файл.
    fn invalidate(&mut self) {
        self.prepared = None;
        self.computed_for = None;
        self.matches.clear();
        self.current = 0;
    }

    fn step(&mut self, forward: bool) {
        if self.matches.is_empty() {
            return;
        }
        let last = self.matches.len() - 1;
        self.current = if forward {
            if self.current >= last {
                0
            } else {
                self.current + 1
            }
        } else if self.current == 0 {
            last
        } else {
            self.current - 1
        };
    }
}

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
    /// Свернуть/развернуть боковую панель с оглавлением.
    toggle_outline: bool,
    /// Открыть файл из списка недавних по его номеру.
    open_recent: Option<usize>,
    /// Очистить список недавних.
    clear_recent: bool,
    /// Открыть поиск и поставить в него фокус.
    find: bool,
    /// Закрыть поиск.
    close_find: bool,
    /// Перейти к следующему (`true`) или предыдущему совпадению.
    step_match: Option<bool>,
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
    /// Зарегистрировать себя обработчиком .md.
    associate: bool,
    /// Убрать регистрацию.
    unassociate: bool,
}

/// Что делаем с масштабом. Отдельный enum, а не число: «сбросить» —
/// это не «прибавить ноль», и match заставит обработать все три случая.
#[derive(Debug, Clone, Copy)]
enum Zoom {
    In,
    Out,
    Reset,
}

pub struct MdGlimpseApp {
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
    /// Зарегистрированы ли мы обработчиком .md.
    ///
    /// Кэш, а не запрос на каждый кадр: меню перерисовывается непрерывно,
    /// а лезть в реестр по шестьдесят раз в секунду незачем. Значение
    /// обновляется при запуске и после каждой попытки что-то изменить.
    association: platform::Association,
    /// Открыто ли окно «Горячие клавиши».
    shortcuts_open: bool,
    /// Развёрнута ли боковая панель с оглавлением.
    outline_open: bool,
    /// Строка, к которой надо прокрутить «Исходник» на следующем кадре.
    pending_scroll_line: Option<usize>,
    /// Первая видимая строка в «Исходнике» — по ней подсвечивается
    /// текущий раздел в оглавлении. Заполняется при отрисовке.
    first_visible_line: usize,
    /// Поиск по документу.
    search: Search,
    /// То, что переживает перезапуск.
    settings: Settings,
}

impl MdGlimpseApp {
    pub fn new(cc: &eframe::CreationContext<'_>, requested: &[PathBuf]) -> Self {
        // Хранилище есть только при включённой фиче persistence и только
        // если eframe сумел его открыть, поэтому всюду значения по умолчанию.
        // Хранилище есть только при включённой фиче persistence и только
        // если eframe сумел его открыть, поэтому всюду значения по умолчанию.
        let mut settings: Settings = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, settings::STORAGE_KEY))
            .unwrap_or_default();
        // Исчезнувшие файлы из «Недавних» вычищаем при загрузке, а не при
        // показе меню: иначе список молча врал бы до первого клика.
        settings.prune(|path| path.exists());

        // Без этого картинки в markdown не отрисуются вообще.
        egui_extras::install_image_loaders(&cc.egui_ctx);
        install_fonts(&cc.egui_ctx);
        // В 0.36 у контекста два независимых Style — для светлой и тёмной темы.
        // Метода style_mut больше нет; all_styles_mut правит оба разом.
        cc.egui_ctx
            .all_styles_mut(|style| style.url_in_tooltip = true);
        install_code_colors(&cc.egui_ctx);

        let mut app = Self {
            document: None,
            error: None,
            mode: if settings.source_mode {
                ViewMode::Source
            } else {
                ViewMode::Rendered
            },
            cache: CommonMarkCache::default(),
            auto_reload: settings.auto_reload,
            last_check: Instant::now(),
            shown_title: String::new(),
            select_all: false,
            notice: None,
            gallery_open: false,
            about_open: false,
            association: platform::state(),
            shortcuts_open: false,
            outline_open: settings.outline_open,
            pending_scroll_line: None,
            first_visible_line: 0,
            search: Search::default(),
            settings,
        };

        // Открываем первый файл; про остальные говорим прямо, а не молчим.
        if let Some((first, rest)) = requested.split_first() {
            app.open_path(first.clone());
            if !rest.is_empty() {
                app.notice = Some((
                    format!(
                        "Открыт только «{}». Ещё {} {} осталось неоткрытым: \
                         вкладок в MdGlimpse пока нет, откройте их по одному.",
                        document::file_name(first),
                        rest.len(),
                        plural(rest.len(), "файл", "файла", "файлов"),
                    ),
                    Instant::now(),
                ));
            }
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
                self.search.invalidate();
                self.settings.remember(&path);
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
            outline_open,
            search,
            settings,
            association,
            ..
        } = self;
        let search_open = search.open;

        let mut actions = Actions::default();
        let has_document = document.is_some();

        // В 0.36 SidePanel и TopBottomPanel слились в один тип Panel,
        // и панель прикрепляется к Ui, а не к Context.
        egui::Panel::top("toolbar").show(ui, |ui| {
            menu_bar(
                ui,
                &mut actions,
                MenuState {
                    has_document,
                    mode: *mode,
                    watching: *auto_reload,
                    outline_open: *outline_open,
                    recent: &settings.recent,
                    association: *association,
                },
            );
            ui.separator();

            ui.horizontal(|ui| {
                // Замыкание держит клон Context, а не `ui`: иначе оно одолжило бы
                // `ui` неизменяемо на всю функцию, а он ниже нужен изменяемо.
                // Клон дешёвый — внутри Arc.
                let ctx = ui.ctx().clone();
                let shortcut = |s| ctx.format_shortcut(s);

                let outline_hint = format!("Оглавление ({})", shortcut(&OUTLINE_SHORTCUT));
                if icons::icon_toggle(ui, Icon::Sidebar, outline_open, &outline_hint).clicked() {
                    // icon_toggle уже переключил флаг; отдельного намерения
                    // не нужно, состояние панели живёт прямо в приложении.
                }

                // Поиск, в отличие от оглавления, флагом не обойдёшься:
                // открытие должно ещё и поставить фокус в поле. Поэтому
                // кнопка отдаёт намерение, а переключает уже приложение —
                // тем же путём, что и Ctrl+F.
                let search_hint = format!("Поиск ({})", shortcut(&FIND_SHORTCUT));
                let mut search_on = search_open;
                if icons::icon_toggle(ui, Icon::Search, &mut search_on, &search_hint).clicked() {
                    if search_on {
                        actions.find = true;
                    } else {
                        actions.close_find = true;
                    }
                }

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
        let scroll_to_line = self.pending_scroll_line.take();
        let mut first_visible = self.first_visible_line;
        // Подсветку передаём только когда поиск открыт и ищет по исходнику:
        // в «Рендере» смещения указывают в другой текст.
        let highlight = (self.search.open && self.mode == ViewMode::Source).then_some(Highlight {
            matches: self.search.matches.as_slice(),
            current: self.search.current,
        });
        // Слой графики обходим только при живом запросе — не каждый кадр.
        let probe_query =
            (self.search.open && self.mode == ViewMode::Rendered && !self.search.query.is_empty())
                .then(|| self.search.query.clone());
        let probe_options = self.search.options;
        let mut probe_result: Option<Result<usize, ProbeBroken>> = None;

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
                    // Полосы прокрутки по умолчанию плавающие: рисуются
                    // поверх текста. Здесь это мешает — переводим на solid,
                    // чтобы полоса занимала своё место.
                    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();

                    // Обе оси, а не только вертикальная. Иначе широкая
                    // таблица просто обрезается по краю области и становится
                    // недостижимой — для просмотрщика это потеря содержимого.
                    //
                    // Перенос абзацев при этом не ломается: egui задаёт
                    // внутреннему Ui ширину видимой области, а не бесконечную
                    // («better to wrap text and shrink images than showing
                    // a horizontal scrollbar» — комментарий в scroll_area.rs),
                    // так что текст по-прежнему укладывается в колонку,
                    // а вылезает только то, что физически шире.
                    egui::ScrollArea::both()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            centered_column(ui, MAX_CONTENT_WIDTH, |ui| {
                                // Подсветку рисуем ПОД текстом: место под неё
                                // резервируем заранее пустой фигурой, а после
                                // отрисовки документа подменяем настоящей.
                                // Иначе прямоугольники легли бы поверх букв.
                                let slot = ui.painter().add(egui::Shape::Noop);
                                let mark = rendered_probe::mark(ui.ctx(), ui.layer_id());

                                CommonMarkViewer::new()
                                    // Без этого крейт не включит разбор
                                    // атрибутов `{#id}`, и вставленные
                                    // нами якоря будут просто текстом.
                                    .enable_scroll_to_heading(true)
                                    .show(ui, cache, document.rendered());

                                if let Some(query) = probe_query {
                                    probe_result = Some(highlight_rendered(
                                        ui,
                                        slot,
                                        mark,
                                        query,
                                        probe_options,
                                    ));
                                }
                            });
                        });
                }
                ViewMode::Source => {
                    first_visible =
                        draw_source(ui, document, select_all, scroll_to_line, highlight);
                }
            }
        });

        self.first_visible_line = first_visible;
        match probe_result {
            Some(Ok(count)) => {
                self.search.highlighted = count;
                self.search.probe_broken = false;
            }
            Some(Err(ProbeBroken)) => {
                self.search.highlighted = 0;
                self.search.probe_broken = true;
            }
            None => {}
        }
        render_anyway
    }

    /// Кладёт весь текст документа в буфер обмена.
    ///
    /// Копируем из `Document`, а не из нарисованного: на экране в любой
    /// момент есть только видимые строки, и «скопировать всё» через них
    /// было бы враньём.
    /// Регистрирует или снимает регистрацию обработчика .md.
    ///
    /// Единственное место во всей программе, которое пишет за её пределы,
    /// и вызывается оно только из меню — то есть по явному действию
    /// человека. При запуске в систему ничего не пишется.
    fn change_association(&mut self, register: bool) {
        let outcome = if register {
            platform::register()
        } else {
            platform::unregister()
        };

        // Состояние перечитываем в любом случае: регистрация могла
        // упасть на полпути, и показывать после этого прежний ответ
        // значило бы врать.
        self.association = platform::state();

        let message = match (outcome, register) {
            (Ok(()), true) => format!(
                "MdGlimpse зарегистрирован для {}. Чтобы открывать их двойным \
                 щелчком, выберите MdGlimpse в «Открыть с помощью» и отметьте \
                 «Всегда использовать»: сделать это за вас программа не может — \
                 Windows оставляет выбор за человеком.",
                extension_list()
            ),
            (Ok(()), false) => format!("Регистрация для {} снята.", extension_list()),
            (Err(error), _) => format!("Не получилось: {error}"),
        };
        self.notice = Some((message, Instant::now()));
    }

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

    /// Полоса поиска. Живёт под тулбаром и внутри области содержимого —
    /// то есть правее оглавления и над самим текстом, а не поверх него.
    fn show_search(&mut self, ui: &mut egui::Ui) {
        if !self.search.open {
            return;
        }
        let Some(document) = self.document.as_ref() else {
            return;
        };

        // Пересчёт до отрисовки: счётчик в полосе должен показывать
        // то же, что подсвечено в тексте на этом же кадре.
        let mode = self.mode;
        let before = (self.search.query.clone(), self.search.options);
        let mut stepped = None;
        let mut close = false;

        egui::Panel::top("search").show(ui, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.search.query)
                        .hint_text("Найти")
                        .desired_width(220.0),
                );
                if std::mem::take(&mut self.search.request_focus) {
                    field.request_focus();
                }
                // Enter в поле — к следующему совпадению, как в редакторах.
                if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    stepped = Some(true);
                    field.request_focus();
                }

                if ui
                    .button("<")
                    .on_hover_text("Предыдущее (Shift+F3)")
                    .clicked()
                {
                    stepped = Some(false);
                }
                if ui.button(">").on_hover_text("Следующее (F3)").clicked() {
                    stepped = Some(true);
                }

                ui.checkbox(&mut self.search.options.case_sensitive, "Аа")
                    .on_hover_text("Учитывать регистр");
                ui.checkbox(&mut self.search.options.whole_word, "|аб|")
                    .on_hover_text("Слово целиком");

                ui.separator();
                let (label, hint) = search_status(&self.search, mode);
                let response = ui.label(label);
                if let Some(hint) = hint {
                    response.on_hover_text(hint);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::icon_button(ui, Icon::Close, "Закрыть поиск (Esc)").clicked()
                    {
                        close = true;
                    }
                });
            });
            ui.add_space(3.0);
        });

        if (self.search.query.clone(), self.search.options) != before {
            self.search.current = 0;
        }
        self.search.refresh(document, mode);

        if let Some(forward) = stepped {
            self.search.step(forward);
            self.scroll_to_current_match();
        }
        if close {
            self.search.open = false;
        }
    }

    /// Прокручивает к текущему совпадению.
    ///
    /// В «Исходнике» смещение совпадения переводится в номер строки —
    /// точно. В «Рендере» точной прокрутки пока нет: позиции текста
    /// на экране добываются чтением слоя графики, и этим занимается
    /// отдельный шаг; здесь — прокрутка к ближайшему заголовку выше
    /// совпадения, чтобы человек хотя бы попал в нужный раздел.
    fn scroll_to_current_match(&mut self) {
        let Some(document) = self.document.as_ref() else {
            return;
        };
        let Some(range) = self.search.matches.get(self.search.current) else {
            return;
        };

        match self.mode {
            ViewMode::Source => {
                let line = document.line_of_offset(range.start);
                // Пара строк сверху, чтобы совпадение не липло к краю.
                self.pending_scroll_line = Some(line.saturating_sub(2));
            }
            ViewMode::Rendered => {
                // Смещение здесь — в простом тексте, а не в исходнике,
                // поэтому номер строки исходника из него не получить.
                // Ближайший заголовок ищем по порядковому номеру совпадения
                // в простом тексте: заголовки в нём идут в том же порядке.
                let plain = self.search.text(document);
                let before = &plain[..range.start];
                let slug = document
                    .headings()
                    .iter()
                    .filter(|heading| !heading.slug.is_empty())
                    .rfind(|heading| {
                        !heading.text.is_empty() && before.contains(heading.text.as_str())
                    })
                    .map(|heading| heading.slug.clone());

                if let Some(slug) = slug {
                    *self.cache.scroll_to_id_target_mut() = Some(slug);
                }
            }
        }
    }

    /// Боковая панель с оглавлением и обработка перехода по её пунктам.
    fn show_outline(&mut self, ui: &mut egui::Ui) {
        if !self.outline_open {
            return;
        }
        let Some(document) = self.document.as_ref() else {
            return;
        };

        // В «Исходнике» текущий раздел известен: первая видимая строка
        // приходит из виртуализации. В «Рендере» её взять неоткуда,
        // не залезая во внутренности egui, — поэтому там подсветки нет.
        let current = match self.mode {
            ViewMode::Source => current_heading(document.headings(), self.first_visible_line),
            ViewMode::Rendered => None,
        };

        let clicked = outline_panel(ui, document.headings(), current);

        let Some(index) = clicked else {
            return;
        };
        let Some(heading) = self
            .document
            .as_ref()
            .and_then(|document| document.headings().get(index))
        else {
            return;
        };

        match self.mode {
            // Режим «Рендер»: просим крейт прокрутиться к нашему якорю.
            // Работает потому, что якоря мы сами и вставили в текст.
            ViewMode::Rendered => {
                if heading.slug.is_empty() {
                    self.notice = Some((
                        "У заголовка, набранного подчёркиванием, нет якоря —                          перейти можно только в режиме «Исходник»"
                            .to_owned(),
                        Instant::now(),
                    ));
                } else {
                    *self.cache.scroll_to_id_target_mut() = Some(heading.slug.clone());
                }
            }
            // Режим «Исходник»: номер строки известен точно, прокручиваем сами.
            ViewMode::Source => self.pending_scroll_line = Some(heading.line),
        }
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
            Some(document) => format!("{} — MdGlimpse", document.title()),
            None => "MdGlimpse".to_owned(),
        };

        if title != self.shown_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.shown_title = title;
        }
    }
}

/// Цвет фона встроенного кода — отдельно для каждой темы.
///
/// Умолчания egui для этой роли неудачны в обе стороны: в тёмной теме
/// `from_gray(64)` слишком светлый и лезет в глаза, в светлой
/// `from_gray(230)` почти не отличим от белого фона. Задаём свои,
/// с лёгким холодным оттенком, чтобы код читался как код.
///
/// Это единственный рычаг, который `egui_commonmark 0.25` оставляет
/// для встроенного кода: он вызывает `RichText::code()`, а тот берёт
/// фон из `visuals.code_bg_color`. Цвет самого текста и высоту заливки
/// крейт наружу не отдаёт.
fn install_code_colors(ctx: &egui::Context) {
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.visuals.code_bg_color = egui::Color32::from_rgb(48, 54, 66);
    });
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.visuals.code_bg_color = egui::Color32::from_rgb(226, 230, 239);
    });
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

/// Расширения списком через запятую: «.md и .markdown».
///
/// Собирается из той же константы, что и регистрация, — разойтись
/// подписи и поведению негде.
fn extension_list() -> String {
    let names: Vec<String> = platform::EXTENSIONS
        .iter()
        .map(|extension| format!(".{extension}"))
        .collect();
    match names.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} и {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// Окно «О программе».
fn about_window(ctx: &egui::Context, open: &mut bool, association: platform::Association) {
    egui::Window::new("О программе")
        .open(open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.heading("MdGlimpse");
            ui.label(format!("Версия {}", env!("CARGO_PKG_VERSION")));
            ui.add_space(6.0);
            ui.label(env!("CARGO_PKG_DESCRIPTION"));
            ui.add_space(6.0);
            ui.label("Просмотрщик, а не редактор: сохранения и правки текста нет и не будет.");
            ui.add_space(6.0);

            ui.label(format!(
                "Файлы {}: {}",
                extension_list(),
                association.label()
            ));

            // Ссылка появляется, только если поле repository заполнено
            // в Cargo.toml. Пустая строка — не адрес, и подсовывать
            // выдуманный лучше не надо.
            let repository = env!("CARGO_PKG_REPOSITORY");
            if !repository.is_empty() {
                ui.hyperlink(repository);
            }
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

/// Подпись счётчика и, если есть что сказать, пояснение к ней.
///
/// Возвращает саму подпись и текст всплывающей подсказки.
fn search_status(search: &Search, mode: ViewMode) -> (egui::RichText, Option<String>) {
    let scope = match mode {
        ViewMode::Rendered => "в тексте",
        ViewMode::Source => "в исходнике",
    };

    if search.query.is_empty() {
        return (egui::RichText::new(scope).weak(), None);
    }
    if search.matches.is_empty() {
        return (
            egui::RichText::new(format!("нет совпадений · {scope}")).weak(),
            None,
        );
    }

    let mut label = format!(
        "{} из {} · {scope}",
        search.current + 1,
        search.matches.len()
    );
    let mut hint = None;

    if mode == ViewMode::Rendered {
        if search.probe_broken {
            label.push_str(" · подсветка не работает");
            hint = Some(
                "Подсветка в режиме «Рендер» опирается на внутреннее устройство egui и перестала работать — вероятно, после обновления библиотеки. Счётчик и переходы при этом верны. В режиме «Исходник» подсветка есть."
                    .to_owned(),
            );
        } else {
            let missed = search.matches.len().saturating_sub(search.highlighted);
            if missed > 0 {
                label.push_str(&format!(" · {missed} без подсветки"));
                hint = Some(format!(
                    "{missed} {} попало внутрь разметки — жирного текста, кода или ссылки. Такое совпадение рисуется двумя отдельными кусками, и подсветить его целиком нельзя. В счётчике и переходах оно учтено. В режиме «Исходник» подсвечивается всё.",
                    plural(missed, "совпадение", "совпадения", "совпадений")
                ));
            }
        }
    }

    (egui::RichText::new(label), hint)
}

/// Боковая панель с оглавлением.
///
/// Возвращает номер заголовка, по которому кликнули.
/// Ширину меняет сама панель — `Panel` пишет её обратно в `width`,
/// чтобы растянутая мышью величина пережила перезапуск.
fn outline_panel(
    ui: &mut egui::Ui,
    headings: &[crate::outline::Heading],
    current: Option<usize>,
) -> Option<usize> {
    let mut clicked = None;

    // Ширину дальше ведёт сама egui: `default_size` действует только на
    // первый запуск, потом побеждает сохранённая геометрия панели.
    egui::Panel::left("outline")
        .resizable(true)
        .default_size(OUTLINE_DEFAULT_WIDTH)
        .min_size(140.0)
        .show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Оглавление").strong());
            ui.separator();

            if headings.is_empty() {
                ui.label(egui::RichText::new("Заголовков нет").weak());
                return;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for (index, heading) in headings.iter().enumerate() {
                        // Отступ по уровню — иначе структура документа
                        // в плоском списке не читается.
                        let indent = (heading.level.saturating_sub(1)) as f32 * 12.0;
                        ui.horizontal(|ui| {
                            ui.add_space(indent);

                            let label = if heading.text.is_empty() {
                                "(без названия)"
                            } else {
                                heading.text.as_str()
                            };
                            let selected = current == Some(index);
                            let button = egui::Button::selectable(selected, label)
                                .wrap_mode(egui::TextWrapMode::Truncate);

                            let mut response = ui.add(button);
                            if !heading.is_atx {
                                // Честно предупреждаем: у setext-заголовка
                                // якоря нет, и в режиме «Рендер» клик
                                // никуда не приведёт.
                                response = response.on_hover_text(
                                    "Заголовок подчёркиванием: в режиме «Рендер»                                      перехода не будет, только в «Исходнике»",
                                );
                            }
                            if response.clicked() {
                                clicked = Some(index);
                            }
                        });
                    }
                });
        });

    clicked
}

/// Номер заголовка, в разделе которого мы сейчас находимся.
///
/// Считается от строки, а не от последнего клика: иначе прокрутка колесом
/// и переход по совпадению поиска дрались бы за подсветку.
fn current_heading(headings: &[crate::outline::Heading], line: usize) -> Option<usize> {
    // `take_while` не умеет ходить назад, поэтому ищем позицию первого
    // заголовка ниже текущей строки и берём предыдущий.
    let after = headings.partition_point(|heading| heading.line <= line);
    after.checked_sub(1)
}

/// Снимок состояния, от которого зависят подписи и галочки в меню.
///
/// Появился не для красоты: аргументов у `menu_bar` стало восемь, и это
/// та граница, за которой список параметров перестаёт читаться. Структура
/// с именованными полями называет каждое значение в месте вызова.
struct MenuState<'a> {
    has_document: bool,
    mode: ViewMode,
    /// Включено ли слежение за файлом.
    watching: bool,
    outline_open: bool,
    recent: &'a [PathBuf],
    association: platform::Association,
}

/// Строка меню: Файл, Правка, Вид, Справка.
///
/// Нативного меню Windows тут нет и быть не может: egui рисует интерфейс
/// сам, доступа к Win32-менюбару через winit не предусмотрено. Это меню
/// в стиле VS Code или Blender — нарисованное приложением.
///
/// Меню осталось единственным местом, где действия названы словами:
/// тулбар теперь целиком на иконках.
fn menu_bar(ui: &mut egui::Ui, actions: &mut Actions, state: MenuState<'_>) {
    let MenuState {
        has_document,
        mode,
        watching,
        outline_open,
        recent,
        association,
    } = state;
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
                if recent.is_empty() {
                    ui.add_enabled(false, egui::Button::new("пока пусто"));
                    return;
                }
                for (index, path) in recent.iter().enumerate() {
                    // В пункте — имя файла, полный путь в подсказке:
                    // пути бывают длиннее любого разумного меню.
                    let label = crate::document::file_name(path);
                    if ui
                        .button(label)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        actions.open_recent = Some(index);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Очистить список").clicked() {
                    actions.clear_recent = true;
                    ui.close();
                }
            });

            if ui
                .add_enabled(has_document, item("Перечитать", Some(&RELOAD_SHORTCUT)))
                .clicked()
            {
                actions.reload = true;
                ui.close();
            }

            ui.separator();

            // Ассоциация — действие с побочным эффектом вне программы,
            // поэтому пункт один и он же показывает текущее состояние:
            // человек должен видеть, что именно произойдёт по нажатию.
            match association {
                platform::Association::Registered => {
                    if ui
                        .add(item("Убрать из «Открыть с помощью»", None))
                        .clicked()
                    {
                        actions.unassociate = true;
                        ui.close();
                    }
                }
                platform::Association::Unsupported => {
                    ui.add_enabled(false, egui::Button::new("Добавить в «Открыть с помощью»"))
                        .on_disabled_hover_text("На этой системе не поддерживается");
                }
                // Stale — записи есть, но ведут на другую копию программы.
                // Лечится тем же действием, что и полное отсутствие.
                platform::Association::Missing | platform::Association::Stale => {
                    if ui
                        .add(item("Добавить в «Открыть с помощью»", None))
                        .clicked()
                    {
                        actions.associate = true;
                        ui.close();
                    }
                }
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
                .add(
                    egui::Button::selectable(outline_open, "Оглавление")
                        .shortcut_text(ctx.format_shortcut(&OUTLINE_SHORTCUT)),
                )
                .clicked()
            {
                actions.toggle_outline = true;
                ui.close();
            }
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
/// Отступ слева, при котором колонка шириной `desired` встанет по центру
/// области шириной `available`.
///
/// Вынесено из отрисовки отдельной функцией по одной причине: это
/// арифметика, а арифметику можно проверить тестом без окна. Ошибку
/// в ней иначе ловят глазами по скриншотам, а это плохой способ.
///
/// Ограничение снизу нулём не для красоты: если колонка шире доступного
/// места, отступ ушёл бы в минус и вёрстка поехала бы. Так колонка
/// максимум прижмётся влево, а горизонтальная прокрутка её достанет.
fn column_offset(available: f32, desired: f32) -> f32 {
    let width = available.min(desired);
    ((available - width) / 2.0).max(0.0)
}

fn centered_column<R>(
    ui: &mut egui::Ui,
    desired_width: f32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    // `available_width` спрашивается заново каждый кадр и относится
    // к области содержимого — той, что осталась после боковой панели
    // и полос. Ничего не запоминается между кадрами.
    let available = ui.available_width();
    let width = available.min(desired_width);
    let side = column_offset(available, desired_width);

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
/// Что подсветить в исходнике.
#[derive(Clone, Copy)]
struct Highlight<'a> {
    /// Байтовые диапазоны внутри `document.source()`, по возрастанию.
    matches: &'a [Range<usize>],
    current: usize,
}

fn draw_source(
    ui: &mut egui::Ui,
    document: &Document,
    select_all: bool,
    scroll_to_line: Option<usize>,
    highlight: Option<Highlight<'_>>,
) -> usize {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);

    // Шрифт моноширинный, поэтому ширина колонки считается точно:
    // самая длинная строка на ширину глифа. Ради этого и держим
    // `max_line_chars` в документе.
    let glyph_width = ui.ctx().fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
    let column_width = document.max_line_columns() as f32 * glyph_width;

    // Интервал обнуляем ДО вызова: show_rows читает его из этого `ui`,
    // чтобы посчитать общую высоту. Поменять его внутри замыкания — значит
    // разъехаться с собственным расчётом прокрутки.
    ui.spacing_mut().item_spacing.y = 0.0;

    let mut area = egui::ScrollArea::both().auto_shrink([false; 2]);
    if let Some(line) = scroll_to_line {
        // Прокрутка считается арифметикой, а не поиском виджета: все строки
        // одной высоты, поэтому смещение — это просто номер строки на высоту.
        area = area.vertical_scroll_offset(line as f32 * row_height);
    }

    let mut first_visible = 0;
    area.show_rows(ui, row_height, document.line_count(), |ui, rows| {
        first_visible = rows.start;
        // Та же центрирующая обёртка, что и у рендера: два расчёта
        // разъехались бы при первой же правке. Номера строк из Этапа 4
        // рисуются внутри неё же — тогда они прилипнут к колонке,
        // а не к краю окна.
        centered_column(ui, column_width, |ui| {
            let job = source_layout(ui, document, rows, select_all, highlight);
            ui.add(
                egui::Label::new(job)
                    .selectable(true)
                    .wrap_mode(egui::TextWrapMode::Extend),
            );
        });
    });

    first_visible
}

/// Рисует подсветку совпадений поверх уже отрисованного документа.
///
/// Возвращает, сколько совпадений удалось найти на экране. Это число
/// меньше общего, если совпадение разрезано разметкой, — разницу
/// показывает счётчик в полосе поиска.
fn highlight_rendered(
    ui: &egui::Ui,
    slot: egui::layers::ShapeIdx,
    mark: rendered_probe::Mark,
    query: String,
    options: search::Options,
) -> Result<usize, ProbeBroken> {
    let rects = rendered_probe::locate(ui.ctx(), ui.layer_id(), mark, &query, options)?;

    let colour = ui.visuals().warn_fg_color.gamma_multiply(0.35);
    let shapes = rects
        .iter()
        .map(|rect| egui::Shape::rect_filled(rect.expand(1.0), 2.0, colour))
        .collect();

    // Подменяем зарезервированную пустышку: подсветка оказывается
    // под текстом, а не поверх него.
    ui.painter().set(slot, egui::Shape::Vec(shapes));
    Ok(rects.len())
}

/// Собирает раскладку видимых строк, размечая совпадения поиска.
///
/// Подсветка делается не прямоугольниками поверх текста, а фоном самих
/// участков: `LayoutJob` умеет задать `TextFormat::background` на диапазон,
/// и тогда фон считает сам раскладчик. Ничего не поедет ни при переносе,
/// ни при смене шрифта, ни при масштабировании.
fn source_layout(
    ui: &egui::Ui,
    document: &Document,
    rows: std::ops::Range<usize>,
    select_all: bool,
    highlight: Option<Highlight<'_>>,
) -> egui::text::LayoutJob {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let visuals = ui.visuals();
    let color = visuals.text_color();

    // Цвета берём из темы, а не из литералов. Все совпадения — приглушённо,
    // текущее — в полную силу: так принято в редакторах и так видно,
    // где ты сейчас.
    let plain = egui::TextFormat {
        font_id: font.clone(),
        color,
        background: if select_all {
            visuals.selection.bg_fill
        } else {
            egui::Color32::TRANSPARENT
        },
        ..Default::default()
    };
    let dim = egui::TextFormat {
        background: visuals.selection.bg_fill.gamma_multiply(0.45),
        ..plain.clone()
    };
    let bright = egui::TextFormat {
        background: visuals.warn_fg_color.gamma_multiply(0.75),
        color: visuals.extreme_bg_color,
        ..plain.clone()
    };

    let mut job = egui::text::LayoutJob::default();
    // Перенос выключен: строки исходника не должны заворачиваться,
    // для длинных есть горизонтальная прокрутка.
    job.wrap.max_width = f32::INFINITY;

    for (position, line_index) in rows.enumerate() {
        if position > 0 {
            job.append("\n", 0.0, plain.clone());
        }

        let range = document.line_range(line_index);
        let line = document.line(line_index);
        let Some(highlight) = highlight else {
            job.append(line, 0.0, plain.clone());
            continue;
        };

        // Совпадения отсортированы, поэтому берём срез, пересекающийся
        // со строкой, двоичным поиском, а не перебором всего списка:
        // на пятимегабайтном файле их бывают десятки тысяч.
        let from = highlight
            .matches
            .partition_point(|found| found.end <= range.start);
        let mut cursor = range.start;

        for (index, found) in highlight.matches.iter().enumerate().skip(from) {
            if found.start >= range.end {
                break;
            }

            let start = found.start.max(range.start);
            let end = found.end.min(range.end);
            if start > cursor {
                job.append(&document.source()[cursor..start], 0.0, plain.clone());
            }

            let format = if index == highlight.current {
                bright.clone()
            } else {
                dim.clone()
            };
            job.append(&document.source()[start..end], 0.0, format);
            cursor = end;
        }

        if cursor < range.end {
            job.append(&document.source()[cursor..range.end], 0.0, plain.clone());
        }
    }

    job
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

impl eframe::App for MdGlimpseApp {
    /// В 0.36 у трейта `App` нет `update(&mut self, ctx, frame)`: приложение
    /// получает корневой `Ui` без полей и фона, а панели прикрепляются к нему.
    /// Сохранение состояния между запусками. Вызывается eframe при выходе
    /// и раз в тридцать секунд.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.settings.outline_open = self.outline_open;
        self.settings.source_mode = self.mode == ViewMode::Source;
        self.settings.auto_reload = self.auto_reload;
        eframe::set_value(storage, settings::STORAGE_KEY, &self.settings);

        // Ключи прежних версий. Хранилище их само не удаляет.
        for key in settings::DEAD_KEYS {
            storage.remove_string(key);
        }
    }

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
        let search_open = self.search.open;

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
                toggle_outline: input.consume_shortcut(&OUTLINE_SHORTCUT),
                find: input.consume_shortcut(&FIND_SHORTCUT),
                // Esc и F3 разбираем только при открытом поиске, иначе
                // отняли бы их у остального интерфейса.
                close_find: search_open
                    && input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                step_match: if search_open {
                    if input.consume_key(egui::Modifiers::SHIFT, egui::Key::F3) {
                        Some(false)
                    } else if input.consume_key(egui::Modifiers::NONE, egui::Key::F3) {
                        Some(true)
                    } else {
                        None
                    }
                } else {
                    None
                },
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
        actions.associate |= from_toolbar.associate;
        actions.unassociate |= from_toolbar.unassociate;
        actions.toggle_outline |= from_toolbar.toggle_outline;
        actions.open_recent = actions.open_recent.or(from_toolbar.open_recent);
        actions.clear_recent |= from_toolbar.clear_recent;
        actions.find |= from_toolbar.find;
        actions.close_find |= from_toolbar.close_find;
        actions.step_match = actions.step_match.or(from_toolbar.step_match);

        // 4. Выполняем — теперь `self` снова свободен целиком.
        if actions.open {
            self.open_dialog();
        }
        if let Some(index) = actions.open_recent {
            if let Some(path) = self.settings.recent.get(index).cloned() {
                self.open_path(path);
            }
        }
        if actions.clear_recent {
            self.settings.recent.clear();
        }
        if actions.associate {
            self.change_association(true);
        }
        if actions.unassociate {
            self.change_association(false);
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
        if actions.toggle_outline {
            self.outline_open = !self.outline_open;
        }
        if actions.find {
            self.search.open = true;
            self.search.request_focus = true;
            // Две подсветки сразу — каша; выделение «всего» уступает поиску.
            self.select_all = false;
        }
        if actions.close_find {
            self.search.open = false;
        }
        if let Some(forward) = actions.step_match {
            self.search.step(forward);
            self.scroll_to_current_match();
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

        // 6. Уведомление и боковая панель — до содержимого: центральная
        //    панель занимает всё, что осталось, поэтому её добавляют последней.
        self.show_notice(ui);
        self.show_outline(ui);
        self.show_search(ui);

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
            about_window(&ctx, &mut self.about_open, self.association);
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
    fn column_offset_is_symmetric() {
        // Слева столько же, сколько останется справа.
        for (available, desired) in [(1000.0, 600.0), (1452.0, 900.0), (300.5, 100.25)] {
            let side = column_offset(available, desired);
            let width = available.min(desired);
            let right = available - side - width;
            assert!(
                (side - right).abs() < 0.001,
                "не по центру: доступно {available}, колонка {desired}, слева {side}, справа {right}"
            );
        }
    }

    #[test]
    fn column_offset_is_zero_when_column_does_not_fit() {
        // Колонка шире доступного места — прижимаем влево, а не в минус.
        assert_eq!(column_offset(400.0, 900.0), 0.0);
        assert_eq!(column_offset(900.0, 900.0), 0.0);
        assert_eq!(column_offset(0.0, 900.0), 0.0);
    }

    #[test]
    fn column_offset_matches_measured_values() {
        // Числа сняты с живой сборки: окно 1800, колонка 900.
        // С открытой боковой панелью доступно 1452, без неё 1768.
        assert_eq!(column_offset(1452.0, 900.0), 276.0);
        assert_eq!(column_offset(1768.0, 900.0), 434.0);
        // Разница отступов — ровно половина ширины панели (316).
        let shift = column_offset(1768.0, 900.0) - column_offset(1452.0, 900.0);
        assert_eq!(shift, 316.0 / 2.0);
    }

    #[test]
    fn decimal_uses_comma() {
        assert_eq!(decimal(2.34), "2,3");
        assert_eq!(decimal(5.0), "5,0");
    }
}
