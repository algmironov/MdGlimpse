//! Разбор структуры markdown: заголовки, якоря, простой текст.
//!
//! Модуль ничего не знает про egui — как и `document.rs`. Всё, что здесь
//! есть, тестируется без окна.
//!
//! # Почему якоря вставляем сами
//!
//! Разумно было бы взять якоря у `egui_commonmark` — он умеет прокручивать
//! к заголовку через `CommonMarkCache::scroll_to_id_target_mut`. Но брать
//! нечего: `pulldown-cmark` **не генерирует** идентификаторы заголовков
//! вовсе. Поле `Tag::Heading { id }` заполняется только из явного
//! синтаксиса `# Заголовок {#my-id}` и только при
//! `Options::ENABLE_HEADING_ATTRIBUTES`; своей слагификации нет ни у него,
//! ни у `egui_commonmark`. Сравнение внутри крейта —
//! `id.into_string() == scroll_target` — для обычного заголовка просто
//! не выполняется.
//!
//! Поэтому `inject_anchors` дописывает `{#slug}` к заголовкам сам, а слаг
//! считает `slug()`. Обе стороны наши, значит совпадают по построению,
//! и при этом мы остаёмся на публичном API крейта.

use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Заголовок документа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// От 1 до 6.
    pub level: u8,
    /// Текст без разметки — то, что видно на экране.
    pub text: String,
    /// Якорь. Пустой у заголовков, которым якорь дать нельзя (см. `is_atx`).
    pub slug: String,
    /// Номер строки, с нуля. По нему прокручивается режим «Исходник».
    pub line: usize,
    /// Заголовок записан в стиле ATX (`# Текст`), а не setext (подчёркнутый).
    ///
    /// Синтаксис атрибутов `{#id}` работает только у ATX, поэтому у setext
    /// якоря не будет и прокрутка в режиме «Рендер» для него не сработает.
    pub is_atx: bool,
}

/// Опции разбора. Одни и те же для всех обходов, чтобы разбор здесь
/// и разбор внутри `egui_commonmark` видели документ одинаково.
fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
}

/// Вытаскивает заголовки в порядке появления.
pub fn extract(source: &str) -> Vec<Heading> {
    let mut headings = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut lines = LineCounter::new(source);

    let mut current: Option<(u8, Range<usize>)> = None;
    let mut text = String::new();

    for (event, range) in Parser::new_ext(source, options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current = Some((level_number(level), range));
                text.clear();
            }
            // Внутри заголовка нас интересует любой видимый текст: и обычный,
            // и код в обратных кавычках, и подпись ссылки. Разметка сама
            // событиями Text не приходит, поэтому отфильтровывать её не нужно.
            Event::Text(chunk) | Event::Code(chunk) if current.is_some() => {
                text.push_str(&chunk);
            }
            Event::End(TagEnd::Heading(_)) => {
                let Some((level, range)) = current.take() else {
                    continue;
                };

                let trimmed = text.trim().to_owned();
                let is_atx = is_atx_heading(source, range.start);
                let slug = if is_atx {
                    unique_slug(&slug(&trimmed), &mut seen)
                } else {
                    String::new()
                };

                headings.push(Heading {
                    level,
                    text: trimmed,
                    slug,
                    line: lines.line_at(range.start),
                    is_atx,
                });
            }
            _ => {}
        }
    }

    headings
}

/// Дописывает `{#slug}` к каждому ATX-заголовку.
///
/// Возвращает текст, пригодный для показа в `egui_commonmark`: после этого
/// его `scroll_to_id_target` начинает попадать в наши заголовки, а ссылки
/// `#якорь` внутри документа — работать.
pub fn inject_anchors(source: &str) -> String {
    let headings = extract(source);
    if headings.is_empty() {
        return source.to_owned();
    }

    // Вставки идут по возрастанию смещений, поэтому собираем результат
    // одним проходом, а не правим строку на месте: правка на месте сдвигала
    // бы все последующие смещения.
    let mut out = String::with_capacity(source.len() + headings.len() * 24);
    let mut cursor = 0;

    for heading in &headings {
        if heading.slug.is_empty() {
            continue;
        }

        let Some(insert_at) = line_end(source, offset_of_line(source, heading.line)) else {
            continue;
        };
        if insert_at < cursor {
            continue;
        }

        out.push_str(&source[cursor..insert_at]);
        out.push_str(" {#");
        out.push_str(&heading.slug);
        out.push('}');
        cursor = insert_at;
    }

    out.push_str(&source[cursor..]);
    out
}

/// Простой текст документа — то, что видно на экране, без разметки.
///
/// Нужен поиску: в режиме «Рендер» считать совпадения по исходнику нельзя,
/// иначе найдутся вхождения внутри URL, атрибутов и самих значков разметки,
/// которых на экране нет.
pub fn plain_text(source: &str) -> String {
    let mut out = String::with_capacity(source.len() / 2);

    for event in Parser::new_ext(source, options()) {
        match event {
            Event::Text(chunk) | Event::Code(chunk) => out.push_str(&chunk),
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::Item
                | TagEnd::CodeBlock
                | TagEnd::TableCell,
            ) => out.push('\n'),
            _ => {}
        }
    }

    out
}

/// Слаг по правилам GitHub: нижний регистр, пробелы в дефисы, пунктуация
/// выброшена. Буквы любых алфавитов остаются, поэтому кириллица переживает
/// преобразование целиком: «Кириллица в разных начертаниях» →
/// `кириллица-в-разных-начертаниях`.
pub fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            out.extend(ch.to_lowercase());
        } else if ch.is_whitespace() {
            out.push('-');
        }
    }

    out
}

/// Одинаковые заголовки получают суффиксы `-1`, `-2` — как на GitHub.
fn unique_slug(base: &str, seen: &mut HashMap<String, usize>) -> String {
    let count = seen.entry(base.to_owned()).or_insert(0);
    let slug = if *count == 0 {
        base.to_owned()
    } else {
        format!("{base}-{count}")
    };
    *count += 1;
    slug
}

fn level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Записан ли заголовок решётками. По CommonMark перед `#` допустимо
/// до трёх пробелов.
fn is_atx_heading(source: &str, start: usize) -> bool {
    source[start..]
        .chars()
        .take(4)
        .find(|ch| *ch != ' ')
        .is_some_and(|ch| ch == '#')
}

/// Смещение начала строки с заданным номером.
fn offset_of_line(source: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }

    let mut seen = 0;
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            seen += 1;
            if seen == line {
                return index + 1;
            }
        }
    }

    source.len()
}

/// Куда дописывать атрибут: конец строки, но до `\r` и `\n`.
fn line_end(source: &str, from: usize) -> Option<usize> {
    if from > source.len() {
        return None;
    }

    let end = source[from..]
        .find('\n')
        .map_or(source.len(), |offset| from + offset);
    let end = if end > from && source.as_bytes()[end - 1] == b'\r' {
        end - 1
    } else {
        end
    };

    Some(end)
}

/// Переводит смещения в номера строк за один проход по тексту.
///
/// Заголовки приходят по возрастанию смещения, поэтому счётчик только
/// движется вперёд. Считать `\n` от начала для каждого заголовка означало бы
/// квадратичное время на файле, где заголовков много.
struct LineCounter<'a> {
    source: &'a str,
    offset: usize,
    line: usize,
}

impl<'a> LineCounter<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            offset: 0,
            line: 0,
        }
    }

    fn line_at(&mut self, offset: usize) -> usize {
        if offset < self.offset {
            // Назад не ходим: пересчитываем с начала. На практике не случается,
            // но молча выдать неверный номер строки хуже, чем разок пройтись.
            self.offset = 0;
            self.line = 0;
        }

        let slice = &self.source.as_bytes()[self.offset..offset.min(self.source.len())];
        self.line += slice.iter().filter(|byte| **byte == b'\n').count();
        self.offset = offset.min(self.source.len());
        self.line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slugs(source: &str) -> Vec<String> {
        extract(source).into_iter().map(|h| h.slug).collect()
    }

    #[test]
    fn levels_and_nesting() {
        let headings = extract("# Один\n\n## Два\n\n### Три\n\n## Ещё два\n");
        let levels: Vec<u8> = headings.iter().map(|h| h.level).collect();
        let texts: Vec<&str> = headings.iter().map(|h| h.text.as_str()).collect();

        assert_eq!(levels, vec![1, 2, 3, 2]);
        assert_eq!(texts, vec!["Один", "Два", "Три", "Ещё два"]);
    }

    #[test]
    fn line_numbers_are_zero_based() {
        let headings = extract("вступление\n\n# Первый\n\nтекст\n\n## Второй\n");
        let lines: Vec<usize> = headings.iter().map(|h| h.line).collect();
        assert_eq!(lines, vec![2, 6]);
    }

    /// Главный тест сверки: кириллический заголовок должен давать ровно тот
    /// якорь, которым принято ссылаться на него в самом markdown.
    #[test]
    fn cyrillic_slug_matches_convention() {
        assert_eq!(
            slug("Проверочный документ MdGlimpse"),
            "проверочный-документ-mdglimpse"
        );
        assert_eq!(
            slug("Кириллица в разных начертаниях"),
            "кириллица-в-разных-начертаниях"
        );
        // Прописные и Ё не должны выпадать.
        assert_eq!(slug("Ёлки-Палки, ЖУРНАЛ!"), "ёлки-палки-журнал");
    }

    #[test]
    fn punctuation_is_dropped_not_replaced() {
        assert_eq!(slug("Что делать?"), "что-делать");
        assert_eq!(slug("A/B: тест (v2)"), "ab-тест-v2");
        assert_eq!(slug("под_чёрк-и-дефис"), "под_чёрк-и-дефис");
    }

    #[test]
    fn duplicate_headings_get_suffixes() {
        assert_eq!(
            slugs("# Раздел\n\n# Раздел\n\n# Раздел\n"),
            vec!["раздел", "раздел-1", "раздел-2"]
        );
    }

    #[test]
    fn code_and_links_inside_heading() {
        let headings = extract("# Запуск `cargo test` и [ссылка](http://example.com/path)\n");
        assert_eq!(headings.len(), 1);
        // В текст входит содержимое кода и подпись ссылки, но не её адрес.
        assert_eq!(headings[0].text, "Запуск cargo test и ссылка");
        assert_eq!(headings[0].slug, "запуск-cargo-test-и-ссылка");
    }

    #[test]
    fn empty_heading_survives() {
        let headings = extract("#\n\n##   \n");
        assert_eq!(headings.len(), 2);
        assert!(headings.iter().all(|h| h.text.is_empty()));
        // Пустой слаг у первого, «-1» у второго: они всё же различимы.
        assert_eq!(headings[0].slug, "");
        assert_eq!(headings[1].slug, "-1");
    }

    #[test]
    fn heading_inside_code_block_is_not_a_heading() {
        let source = "# Настоящий\n\n```\n# Не заголовок\n```\n\n## Тоже настоящий\n";
        assert_eq!(slugs(source), vec!["настоящий", "тоже-настоящий"]);
    }

    #[test]
    fn setext_heading_is_listed_without_anchor() {
        let headings = extract("Подчёркнутый\n============\n\n# Обычный\n");
        assert_eq!(headings.len(), 2);

        assert_eq!(headings[0].text, "Подчёркнутый");
        assert!(!headings[0].is_atx, "setext должен опознаваться");
        assert!(
            headings[0].slug.is_empty(),
            "у setext якоря быть не может: синтаксис {{#id}} к нему неприменим"
        );

        assert!(headings[1].is_atx);
        assert_eq!(headings[1].slug, "обычный");
    }

    #[test]
    fn anchors_are_injected_into_atx_only() {
        let source = "# Первый\n\nтекст\n\nПодчёркнутый\n============\n\n## Второй\n";
        let out = inject_anchors(source);

        assert!(out.contains("# Первый {#первый}"));
        assert!(out.contains("## Второй {#второй}"));
        assert!(out.contains("Подчёркнутый\n============"));
        // Текст между заголовками не пострадал.
        assert!(out.contains("\nтекст\n"));
    }

    #[test]
    fn injection_leaves_code_blocks_alone() {
        let source = "```\n# Не заголовок\n```\n\n# Заголовок\n";
        let out = inject_anchors(source);

        assert!(out.contains("# Не заголовок\n```"), "код тронут: {out}");
        assert!(out.contains("# Заголовок {#заголовок}"));
    }

    #[test]
    fn injection_survives_crlf() {
        let source = "# Заголовок\r\n\r\nтекст\r\n";
        let out = inject_anchors(source);
        assert!(out.contains("# Заголовок {#заголовок}\r\n"), "{out:?}");
    }

    #[test]
    fn plain_text_drops_markup_and_urls() {
        let source =
            "# Заголовок\n\nТекст со [ссылкой](https://example.com/src/path) и **жирным**.\n";
        let plain = plain_text(source);

        assert!(plain.contains("Заголовок"));
        assert!(plain.contains("Текст со ссылкой и жирным."));
        // Ради этого поиск в режиме «Рендер» и считает по простому тексту:
        // в исходнике «src» есть, а на экране его нет.
        assert!(
            !plain.contains("src"),
            "адрес ссылки попал в текст: {plain}"
        );
        assert!(!plain.contains('*'));
    }
}
