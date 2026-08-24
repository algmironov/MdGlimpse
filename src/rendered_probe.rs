//! Единственное место в проекте, где читаются внутренности egui.
//!
//! # Зачем понадобилось лезть внутрь
//!
//! `egui_commonmark` не отдаёт наружу ничего о том, где оказался текст:
//! `CommonMarkViewer::show` возвращает `InnerResponse<()>`, а каждый
//! фрагмент рисуется своим `ui.label(..)`, и `Response` выбрасывается.
//! Значит подсветить найденное в отрендеренном виде «по-хорошему» нельзя:
//! публичного способа узнать координаты слова на экране не существует.
//!
//! Обходной путь — прочитать то, что уже нарисовано, из списка фигур слоя.
//!
//! # Что именно предполагается про устройство egui
//!
//! Ровно четыре допущения, и все они об одном — о внутреннем представлении
//! отрисовки. Ни одно не является частью обещанного API:
//!
//! 1. `Context::graphics` даёт доступ на чтение к `GraphicLayers`,
//!    а `GraphicLayers::get` — к `PaintList` конкретного слоя.
//! 2. `PaintList::all_entries` перечисляет уже накопленные `ClippedShape`
//!    в порядке отрисовки, а `PaintList::next_idx` до отрисовки указывает,
//!    с какого места начнутся наши фигуры.
//! 3. Текст документа приезжает как `Shape::Text(TextShape)`
//!    с координатой `pos` и разложенным `galley`.
//! 4. `Galley::pos_from_cursor` возвращает положение символа внутри galley.
//!
//! # Почему это опасно и что с этим сделано
//!
//! При обновлении egui любое из допущений может тихо перестать выполняться:
//! шейпы поедут в другой слой, текст начнёт рисоваться иначе — и подсветка
//! просто перестанет появляться. Молчаливая поломка хуже громкой, поэтому
//! здесь есть **проверка живости**: если после отрисовки документа в слое
//! не нашлось ни одного текстового шейпа, это не «совпадений нет», а
//! сломанная связка, и модуль возвращает `Err(ProbeBroken)`. Приложение
//! говорит об этом пользователю, а не молчит.
//!
//! Граница модуля намеренно узкая: наружу торчат `mark` и `locate`,
//! всё остальное — детали. Если однажды придётся всё это выбросить,
//! выбрасывать нужно будет один файл.

use eframe::egui;
use egui::text::CCursor;
use egui::{LayerId, Rect, Shape};

use crate::search;

/// Связка с внутренностями egui перестала работать.
#[derive(Debug, Clone, Copy)]
pub struct ProbeBroken;

/// Отметка в списке фигур: с этого места начнётся документ.
///
/// Снимается до отрисовки, чтобы потом не перебирать фигуры тулбара,
/// панелей и всего остального.
#[derive(Debug, Clone, Copy)]
pub struct Mark(usize);

/// Запоминает, сколько фигур в слое было до отрисовки документа.
pub fn mark(ctx: &egui::Context, layer: LayerId) -> Mark {
    let index = ctx.graphics(|layers| layers.get(layer).map_or(0, |list| list.next_idx().0));
    Mark(index)
}

/// Находит на экране прямоугольники совпадений внутри уже нарисованного
/// документа.
///
/// Вызывать только при активном поиске: обход всех фигур слоя на каждом
/// кадре — лишняя работа, а пользы от него без запроса никакой.
///
/// # Ограничение, о котором обязан знать вызывающий
///
/// Фрагмент текста — это один `ui.label`, то есть один galley. Совпадение,
/// пересекающее границу разметки (`**жирный**`, `` `код` ``, ссылка),
/// разрезано на два galley и найдено не будет. Поэтому вернувшееся
/// количество прямоугольников меньше или равно числу совпадений,
/// посчитанному по простому тексту, и разницу надо показать человеку,
/// а не прятать.
pub fn locate(
    ctx: &egui::Context,
    layer: LayerId,
    mark: Mark,
    needle: &str,
    options: search::Options,
) -> Result<Vec<Rect>, ProbeBroken> {
    if needle.is_empty() {
        return Ok(Vec::new());
    }

    ctx.graphics(|layers| {
        let Some(list) = layers.get(layer) else {
            return Err(ProbeBroken);
        };

        let mut saw_text = false;
        let mut rects = Vec::new();

        for clipped in list.all_entries().skip(mark.0) {
            let Shape::Text(text) = &clipped.shape else {
                continue;
            };
            saw_text = true;

            let galley = &text.galley;
            let content = galley.text();
            for range in search::Haystack::new(content).find_all(content, needle, options) {
                // `pos_from_cursor` считает в символах, а поиск отдаёт байты.
                let start = content[..range.start].chars().count();
                let end = content[..range.end].chars().count();

                let from = galley.pos_from_cursor(CCursor::new(start));
                let to = galley.pos_from_cursor(CCursor::new(end));
                rects.push(from.union(to).translate(text.pos.to_vec2()));
            }
        }

        // Вот она, проверка живости. Документ, в котором нарисован хоть
        // какой-то текст, обязан дать хотя бы один текстовый шейп.
        if saw_text {
            Ok(rects)
        } else {
            Err(ProbeBroken)
        }
    })
}
