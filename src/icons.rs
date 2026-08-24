//! Иконки и виджеты, которых в egui нет.
//!
//! Ограничение «не знать про egui» касается только `document.rs` — этот
//! модуль как раз про отрисовку, и без egui существовать не может.
//!
//! Иконки нарисованы примитивами `Painter`, а не взяты из иконочного шрифта
//! и не загружены из SVG: ради семи глифов не хочется ни вшивать шрифт
//! на сотни килобайт, ни тянуть `resvg` со всем его деревом и временем
//! сборки. Порог, за которым этот выбор перестанет окупаться, — примерно
//! десяток-полтора иконок; см. Этап 0.5 в ROADMAP.
//!
//! # Как устроены иконки
//!
//! Все они описаны в **единой логической сетке 16×16** и единой толщиной
//! штриха в её единицах. Ни одна координата не задана в пикселях. Размер
//! иконки берётся от высоты строки текущего шрифта, поэтому Ctrl+плюс
//! масштабирует тулбар целиком, а не разваливает его. Цвет берётся
//! из `visuals` темы и нигде не прописан числом.

use eframe::egui;
use egui::{vec2, Pos2, Rect, Stroke, Vec2};

/// Сторона логической сетки, в которой описаны все иконки.
const GRID: f32 = 16.0;

/// Толщина штриха в единицах сетки.
const STROKE_UNITS: f32 = 1.4;

/// Доля стороны иконки, уходящая на поля внутри кнопки.
const BUTTON_PADDING: f32 = 0.32;

/// Скругление рамок.
const CORNER: f32 = 5.0;

/// Какую иконку рисовать.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Глаз — режим просмотра.
    Eye,
    /// `</>` — режим исходника.
    Code,
    /// Раскрытая папка — «Открыть…».
    Folder,
    /// Круговая стрелка — «Перечитать».
    Reload,
    /// Солнце — переключить на светлую тему.
    Sun,
    /// Луна — переключить на тёмную тему.
    Moon,
    /// Расходящиеся волны — «Следить за файлом».
    Waves,
    /// Боковая панель — «Оглавление».
    Sidebar,
    /// Крестик — «Закрыть».
    Close,
    /// Лупа — «Поиск».
    Search,
}

impl Icon {
    /// Имя для отладочной галереи.
    fn name(self) -> &'static str {
        match self {
            Self::Eye => "Eye",
            Self::Code => "Code",
            Self::Folder => "Folder",
            Self::Reload => "Reload",
            Self::Sun => "Sun",
            Self::Moon => "Moon",
            Self::Waves => "Waves",
            Self::Sidebar => "Sidebar",
            Self::Close => "Close",
            Self::Search => "Search",
        }
    }

    /// Все иконки — для галереи. Забыть новую здесь компилятор не даст:
    /// `name()` выше требует покрыть каждый вариант, и добавляя иконку,
    /// вы придёте сюда следом.
    const ALL: &'static [Self] = &[
        Self::Eye,
        Self::Code,
        Self::Folder,
        Self::Reload,
        Self::Sun,
        Self::Moon,
        Self::Waves,
        Self::Sidebar,
        Self::Close,
        Self::Search,
    ];
}

/// Сторона иконки для текущего масштаба интерфейса.
///
/// Считается от высоты строки основного шрифта, а не от константы
/// в пикселях: иначе при Ctrl+плюс текст растёт, а иконки нет.
pub fn icon_size(ui: &egui::Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Body).round()
}

/// Кнопка с иконкой.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str) -> egui::Response {
    let response = button_frame(ui, icon, false);
    response.on_hover_text(tooltip)
}

/// Кнопка-переключатель: включённая подсвечена фоном, как активный
/// сегмент переключателя режимов. Единый язык на весь тулбар.
pub fn icon_toggle(ui: &mut egui::Ui, icon: Icon, on: &mut bool, tooltip: &str) -> egui::Response {
    let response = button_frame(ui, icon, *on);
    if response.clicked() {
        *on = !*on;
    }
    response.on_hover_text(tooltip)
}

/// Общая обвязка кнопки: место, реакция, фон, иконка.
fn button_frame(ui: &mut egui::Ui, icon: Icon, active: bool) -> egui::Response {
    let side = icon_size(ui);
    let outer = side * (1.0 + 2.0 * BUTTON_PADDING);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(outer), egui::Sense::click());

    let enabled = ui.is_enabled();
    let visuals = ui.visuals();

    let fill = if active {
        Some(visuals.selection.bg_fill)
    } else if enabled && response.hovered() {
        Some(visuals.widgets.hovered.bg_fill)
    } else if enabled {
        Some(visuals.widgets.inactive.bg_fill)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, CORNER, fill);
    }

    let color = icon_color(ui, active, enabled);
    draw(ui.painter(), inner_square(rect, side), icon, color);

    response
}

/// Цвет иконки для текущей темы и состояния. Ни одного литерала.
fn icon_color(ui: &egui::Ui, active: bool, enabled: bool) -> egui::Color32 {
    let visuals = ui.visuals();
    if !enabled {
        visuals.widgets.noninteractive.fg_stroke.color
    } else if active {
        visuals.selection.stroke.color
    } else {
        visuals.widgets.inactive.fg_stroke.color
    }
}

fn inner_square(rect: Rect, side: f32) -> Rect {
    Rect::from_center_size(rect.center(), Vec2::splat(side))
}

/// Сегментированный переключатель: несколько иконок в общей рамке,
/// выбранная подсвечена.
///
/// Возвращает индекс сегмента, по которому кликнули, или `None`.
/// Виджет не хранит состояние сам — какой сегмент активен, решает
/// вызывающий код. Так и надо в immediate mode GUI: состояние живёт
/// в структуре приложения, а не внутри виджета.
pub struct Segment<'a> {
    pub icon: Icon,
    /// Подсказка при наведении. Без неё иконку не разгадать.
    pub tooltip: &'a str,
}

pub fn segmented_icons(
    ui: &mut egui::Ui,
    segments: &[Segment<'_>],
    selected: usize,
) -> Option<usize> {
    let mut clicked = None;

    let side = icon_size(ui);
    let cell_side = side * (1.0 + 2.0 * BUTTON_PADDING);
    let size = vec2(cell_side * segments.len() as f32, cell_side);
    // Место занимаем сразу под весь переключатель, а клики ловим по каждому
    // сегменту отдельно — потому это Sense::hover(), а не click().
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());

    let enabled = ui.is_enabled();
    ui.painter()
        .rect_filled(rect, CORNER, ui.visuals().widgets.inactive.bg_fill);

    for (index, segment) in segments.iter().enumerate() {
        let cell = Rect::from_min_size(
            rect.min + vec2(cell_side * index as f32, 0.0),
            Vec2::splat(cell_side),
        );

        // Свой Id на сегмент: иначе egui сочтёт их одним виджетом
        // и подсветка будет прыгать.
        let response = ui.interact(cell, ui.id().with(("segment", index)), egui::Sense::click());
        let is_selected = index == selected;

        if is_selected {
            ui.painter().rect_filled(
                cell.shrink(2.0),
                CORNER - 1.0,
                ui.visuals().selection.bg_fill,
            );
        } else if response.hovered() && enabled {
            ui.painter().rect_filled(
                cell.shrink(2.0),
                CORNER - 1.0,
                ui.visuals().widgets.hovered.bg_fill,
            );
        }

        let color = icon_color(ui, is_selected, enabled);
        draw(ui.painter(), inner_square(cell, side), segment.icon, color);

        if response.clicked() {
            clicked = Some(index);
        }
        response.on_hover_text(segment.tooltip);
    }

    clicked
}

// ---------------------------------------------------------------------------
// Собственно отрисовка
// ---------------------------------------------------------------------------

/// Перо, переводящее координаты сетки 16×16 в экранные.
///
/// Всё ниже описано в единицах сетки, поэтому иконки согласованы между собой
/// по размеру и толщине штриха автоматически, а не «на глаз».
struct Pen<'a> {
    painter: &'a egui::Painter,
    rect: Rect,
    stroke: Stroke,
}

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.rect.min + vec2(x / GRID * self.rect.width(), y / GRID * self.rect.height())
    }

    fn path(&self, points: &[(f32, f32)]) {
        let points = points.iter().map(|&(x, y)| self.at(x, y)).collect();
        self.painter.line(points, self.stroke);
    }

    fn circle(&self, cx: f32, cy: f32, r: f32) {
        let radius = r / GRID * self.rect.width();
        self.painter
            .circle_stroke(self.at(cx, cy), radius, self.stroke);
    }

    /// Дуга по углам в градусах. Ось Y экранная, вниз, поэтому 90° — низ.
    fn arc(&self, cx: f32, cy: f32, r: f32, from_deg: f32, to_deg: f32) -> Vec<Pos2> {
        const STEPS: usize = 24;
        (0..=STEPS)
            .map(|step| {
                let t = step as f32 / STEPS as f32;
                let angle = (from_deg + (to_deg - from_deg) * t).to_radians();
                self.at(cx + r * angle.cos(), cy + r * angle.sin())
            })
            .collect()
    }

    fn stroke_arc(&self, cx: f32, cy: f32, r: f32, from_deg: f32, to_deg: f32) {
        self.painter
            .line(self.arc(cx, cy, r, from_deg, to_deg), self.stroke);
    }
}

fn draw(painter: &egui::Painter, rect: Rect, icon: Icon, color: egui::Color32) {
    // Толщина тоже в единицах сетки — иначе на крупной иконке штрих
    // выглядел бы волосяным, а на мелкой жирным.
    let width = STROKE_UNITS / GRID * rect.width();
    let pen = Pen {
        painter,
        rect,
        stroke: Stroke::new(width, color),
    };

    match icon {
        Icon::Eye => eye(&pen),
        Icon::Code => code(&pen),
        Icon::Folder => folder(&pen),
        Icon::Reload => reload(&pen),
        Icon::Sun => sun(&pen),
        Icon::Moon => moon(&pen),
        Icon::Waves => waves(&pen),
        Icon::Sidebar => sidebar(&pen),
        Icon::Close => close(&pen),
        Icon::Search => search(&pen),
    }
}

/// Глаз: две симметричные дуги, сходящиеся в уголках, плюс зрачок.
///
/// Дуга приближается параболой `y = h·(1 - t²)` при `t` от -1 до 1 —
/// от настоящей окружности на таком размере не отличить, а считать проще.
fn eye(pen: &Pen<'_>) {
    const STEPS: usize = 16;
    let lid = |direction: f32| -> Vec<(f32, f32)> {
        (0..=STEPS)
            .map(|step| {
                let t = -1.0 + 2.0 * step as f32 / STEPS as f32;
                (8.0 + t * 6.0, 8.0 + direction * 3.4 * (1.0 - t * t))
            })
            .collect()
    };

    pen.path(&lid(-1.0));
    pen.path(&lid(1.0));
    pen.circle(8.0, 8.0, 2.1);
}

/// `</>`: угловая скобка влево, дробь посередине, скобка вправо.
fn code(pen: &Pen<'_>) {
    pen.path(&[(6.2, 4.6), (2.6, 8.0), (6.2, 11.4)]);
    pen.path(&[(9.8, 4.6), (13.4, 8.0), (9.8, 11.4)]);
    pen.path(&[(9.2, 3.8), (6.8, 12.2)]);
}

/// Раскрытая папка: задняя стенка с язычком и отогнутая передняя.
fn folder(pen: &Pen<'_>) {
    pen.path(&[
        (2.0, 12.6),
        (2.0, 4.2),
        (6.2, 4.2),
        (7.8, 6.4),
        (13.2, 6.4),
        (13.2, 8.4),
    ]);
    pen.path(&[
        (2.0, 12.6),
        (4.6, 8.4),
        (15.0, 8.4),
        (12.4, 12.6),
        (2.0, 12.6),
    ]);
}

/// Круговая стрелка: незамкнутая окружность и наконечник на конце.
///
/// Первая версия рисовала наконечник длинными лучами, и в галерее иконка
/// читалась просто как «круг с разрывом». Наконечник укорочен, разрыв
/// увеличен — теперь видно, что стрелка.
fn reload(pen: &Pen<'_>) {
    const END: f32 = -60.0;
    let radius = 5.0;

    pen.stroke_arc(8.0, 8.0, radius, END, 250.0);

    // Наконечник строим от касательной в конце дуги, а не «на глаз»:
    // при смене угла или радиуса он поедет вместе с дугой.
    let angle = END.to_radians();
    let tip = (8.0 + radius * angle.cos(), 8.0 + radius * angle.sin());
    // Внутрь окружности и наружу от неё — короткие усы по 2,4 единицы.
    let inward = (-angle.cos(), -angle.sin());
    let along = (-angle.sin(), angle.cos());

    pen.path(&[
        (
            tip.0 + 2.4 * inward.0 + 1.2 * along.0,
            tip.1 + 2.4 * inward.1 + 1.2 * along.1,
        ),
        tip,
        (
            tip.0 - 2.4 * inward.0 + 1.2 * along.0,
            tip.1 - 2.4 * inward.1 + 1.2 * along.1,
        ),
    ]);
}

/// Солнце: кружок и восемь лучей.
///
/// В первой версии ядро было крупным, а лучи короткими, и в галерее
/// иконка читалась как шестерёнка. Ядро уменьшено, зазор до лучей
/// увеличен — силуэт стал солнечным.
fn sun(pen: &Pen<'_>) {
    pen.circle(8.0, 8.0, 2.7);

    for index in 0..8 {
        let angle = (index as f32 * 45.0_f32).to_radians();
        let (sin, cos) = angle.sin_cos();
        pen.path(&[
            (8.0 + 4.6 * cos, 8.0 + 4.6 * sin),
            (8.0 + 6.4 * cos, 8.0 + 6.4 * sin),
        ]);
    }
}

/// Луна: серп из двух дуг.
///
/// Концы дуг — точки пересечения двух окружностей, иначе серп не сомкнётся
/// и на краях будут торчать хвосты. Углы посчитаны один раз для радиуса 6
/// и смещения (3,2; -3,2) и вписаны сюда числами.
fn moon(pen: &Pen<'_>) {
    // Радиус 5,4 вместо 6: с шестёркой луна оптически перевешивала
    // соседние иконки в галерее.
    let outer = pen.arc(8.0, 8.0, 5.4, 22.9, 247.1);
    let inner = pen.arc(10.9, 5.1, 5.4, 202.9, 67.1);

    pen.painter.line(outer, pen.stroke);
    pen.painter.line(inner, pen.stroke);
}

/// Расходящиеся волны: точка и три дуги, как у значка приёма сигнала.
///
/// Круговая стрелка уже занята «Перечитать», поэтому здесь принципиально
/// другой силуэт — спутать не с чем.
fn waves(pen: &Pen<'_>) {
    pen.circle(4.6, 11.4, 0.85);
    for radius in [3.4, 6.0, 8.6] {
        pen.stroke_arc(4.6, 11.4, radius, -90.0, 0.0);
    }
}

/// Боковая панель: рамка, вертикальная перегородка и строчки списка справа.
fn sidebar(pen: &Pen<'_>) {
    pen.path(&[
        (2.2, 3.0),
        (13.8, 3.0),
        (13.8, 13.0),
        (2.2, 13.0),
        (2.2, 3.0),
    ]);
    pen.path(&[(6.6, 3.0), (6.6, 13.0)]);
    for y in [6.0, 8.0, 10.0] {
        pen.path(&[(3.6, y), (5.6, y)]);
    }
}

/// Крестик: две диагонали.
///
/// Рисуем, а не берём символ `✕` из шрифта, и на то есть причина.
/// Символа U+2715 нет в Inter, и он доставался из последнего запасного
/// шрифта цепочки — иконочного, где выглядел не крестиком. Кроме того,
/// символ в тексте не масштабируется вместе с иконками и не подчиняется
/// их толщине штриха: рядом с рисованными иконками он всегда чужой.
fn close(pen: &Pen<'_>) {
    pen.path(&[(4.6, 4.6), (11.4, 11.4)]);
    pen.path(&[(11.4, 4.6), (4.6, 11.4)]);
}

/// Лупа: окружность и ручка по той же диагонали.
fn search(pen: &Pen<'_>) {
    pen.circle(6.8, 6.8, 4.0);
    // Ручка начинается на самой окружности, иначе видно стык.
    let edge = 6.8 + 4.0 / (2.0_f32).sqrt();
    pen.path(&[(edge, edge), (13.6, 13.6)]);
}

// ---------------------------------------------------------------------------
// Символы, подставляемые прямо в текст
// ---------------------------------------------------------------------------

/// Символы вне латиницы и кириллицы, которые интерфейс подставляет
/// в текст, а не рисует иконкой.
///
/// Список ведётся руками, и это осознанно: автоматически собрать его
/// из исходников нельзя, а цена ошибки — пустой квадрат вместо символа,
/// который поштучно почти не заметишь. Раздел в галерее проверяет каждый
/// символ на наличие глифа в обеих семьях шрифтов.
///
/// Если добавляете в интерфейс новый символ — впишите его сюда
/// и посмотрите галерею. Если глифа нет, рисуйте иконку, как сделано
/// с крестиком.
pub const TEXT_SYMBOLS: &[(char, &str)] = &[
    ('«', "кавычки в сообщениях"),
    ('»', "кавычки в сообщениях"),
    ('·', "разделитель в счётчике поиска"),
    ('×', "подписи размеров в этой галерее"),
    ('—', "тире в заголовке окна и текстах"),
    ('…', "многоточие в «Открыть…»"),
    ('−', "минус в таблице горячих клавиш"),
];

// ---------------------------------------------------------------------------
// Отладочная галерея
// ---------------------------------------------------------------------------

/// Окно со всеми иконками в трёх размерах и обеих темах.
///
/// Поштучно рассогласование толщин и оптических размеров не видно —
/// его ловит только вид «всё разом». Открывается скрытым сочетанием
/// Ctrl+Shift+I и остаётся в сборке: каждая новая иконка на следующих
/// этапах будет проверяться здесь же.
pub fn gallery(ctx: &egui::Context, open: &mut bool) {
    egui::Window::new("Иконки — отладка")
        .open(open)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label("Сетка 16×16, толщина штриха и размер — от высоты строки шрифта.");
            ui.separator();

            for (title, visuals) in [
                ("Тёмная тема", egui::Visuals::dark()),
                ("Светлая тема", egui::Visuals::light()),
            ] {
                ui.label(egui::RichText::new(title).strong());

                // Локальная подмена визуалов: обе темы видны одновременно,
                // без перезапуска и переключения темы всего приложения.
                let frame = egui::Frame::new()
                    .fill(visuals.panel_fill)
                    .inner_margin(8)
                    .corner_radius(CORNER);

                frame.show(ui, |ui| {
                    *ui.visuals_mut() = visuals.clone();
                    gallery_rows(ui);
                });
                ui.add_space(8.0);
            }

            ui.separator();
            ui.label(egui::RichText::new("Символы, подставляемые в текст").strong());
            ui.label(
                egui::RichText::new(
                    "«НЕТ» означает пустой квадрат в интерфейсе: глифа нет                      ни в одном шрифте цепочки. Такой символ надо заменить иконкой.",
                )
                .weak(),
            );
            symbol_coverage(ui);
        });
}

fn gallery_rows(ui: &mut egui::Ui) {
    let base = icon_size(ui);

    for (label, scale) in [("×1", 1.0), ("×1,5", 1.5), ("×2", 2.0)] {
        ui.horizontal(|ui| {
            ui.label(label);
            for &icon in Icon::ALL {
                let side = base * scale;
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::hover());
                draw(ui.painter(), rect, icon, icon_color(ui, false, true));
            }
            // Тот же ряд в состоянии «отключено» — проверяем, что серый
            // цвет читается и что иконка не пропадает совсем.
            ui.separator();
            for &icon in Icon::ALL {
                let side = base * scale;
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::hover());
                draw(ui.painter(), rect, icon, icon_color(ui, false, false));
            }
        });
    }

    ui.horizontal(|ui| {
        ui.label("имена");
        for &icon in Icon::ALL {
            ui.label(egui::RichText::new(icon.name()).small().weak());
        }
    });
}

/// Проверка, что каждому символу из `TEXT_SYMBOLS` найдётся глиф.
///
/// `has_glyph` — публичный API egui, он спрашивает всю цепочку шрифтов
/// семьи. Юнит-тестом это не сделать: нужен живой `Context` с уже
/// загруженными шрифтами, — поэтому проверка отладочная. Но настоящая:
/// «нет» здесь означает пустой квадрат в интерфейсе.
fn symbol_coverage(ui: &mut egui::Ui) {
    // Цвет берём заранее: замыкание ниже иначе одолжило бы `ui`
    // неизменяемо, а он тут же нужен изменяемо.
    let missing_color = ui.visuals().error_fg_color;
    // Слова, а не значки: значок «галочка» сам может оказаться
    // непокрытым, и тогда отчёт соврёт о самом себе.
    let verdict = move |found: bool| {
        if found {
            egui::RichText::new("есть").weak()
        } else {
            egui::RichText::new("НЕТ").color(missing_color)
        }
    };

    egui::Grid::new("symbols").num_columns(4).show(ui, |ui| {
        ui.label(egui::RichText::new("символ").strong());
        ui.label(egui::RichText::new("Inter").strong());
        ui.label(egui::RichText::new("JetBrains Mono").strong());
        ui.label(egui::RichText::new("где используется").strong());
        ui.end_row();

        for &(symbol, usage) in TEXT_SYMBOLS {
            let proportional = egui::TextStyle::Body.resolve(ui.style());
            let monospace = egui::TextStyle::Monospace.resolve(ui.style());
            let (in_body, in_mono) = ui.ctx().fonts_mut(|fonts| {
                (
                    fonts.has_glyph(&proportional, symbol),
                    fonts.has_glyph(&monospace, symbol),
                )
            });

            ui.label(format!("{symbol}  U+{:04X}", symbol as u32));
            ui.label(verdict(in_body));
            ui.label(verdict(in_mono));
            ui.label(egui::RichText::new(usage).weak());
            ui.end_row();
        }
    });
}
