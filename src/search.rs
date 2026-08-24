//! Поиск подстроки. Про egui не знает, тестируется без окна.
//!
//! Ищем по-разному в разных режимах, и это осознанно. В «Исходнике» ищем
//! по исходному тексту — там пользователь видит именно его. В «Рендере»
//! ищем по извлечённому простому тексту (`outline::plain_text`): в исходнике
//! есть адреса ссылок, значки разметки и атрибуты, которых на экране нет,
//! и поиск слова `src` находил бы вхождения внутри путей. Счётчики в двух
//! режимах поэтому будут разными, и это правильный ответ, а не расхождение.

use std::ops::Range;

/// Как искать.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    pub case_sensitive: bool,
    pub whole_word: bool,
}

/// Текст, подготовленный к многократному поиску.
///
/// Свёртка регистра считается один раз. Это не преждевременная
/// оптимизация, а следствие замера: на файле в 5 МБ поиск без учёта
/// регистра занимал 30 мс против 1,9 мс с учётом — почти всё время
/// уходило на `to_lowercase` всего текста, и происходило это на каждое
/// нажатие клавиши в поле поиска.
///
/// Ценой лишней копии текста в памяти: на документ 5 МБ это ещё 5 МБ,
/// и только пока открыт поиск.
pub struct Haystack {
    /// Свёрнутая копия. `None`, если свёртка меняет длину в байтах:
    /// тогда смещения в ней не совпадают с исходными и брать их нельзя.
    folded: Option<String>,
    /// Длина текста, для которого всё посчитано. Только ради проверки,
    /// что дальше пришёл тот же текст.
    source_len: usize,
}

impl Haystack {
    pub fn new(text: &str) -> Self {
        let folded = text.to_lowercase();
        Self {
            folded: (folded.len() == text.len()).then_some(folded),
            source_len: text.len(),
        }
    }

    /// Все вхождения `needle` в `text`, в порядке возрастания смещения.
    ///
    /// `text` обязан быть тем же, для которого создавался `Haystack`.
    pub fn find_all(&self, text: &str, needle: &str, options: Options) -> Vec<Range<usize>> {
        debug_assert_eq!(
            text.len(),
            self.source_len,
            "Haystack готовился для другого текста — смещения будут неверны"
        );

        if needle.is_empty() || text.is_empty() {
            return Vec::new();
        }

        let candidates = if options.case_sensitive {
            exact(text, needle)
        } else {
            let needle = needle.to_lowercase();
            match &self.folded {
                Some(folded) => exact(folded, &needle),
                None => scan_by_chars(text, &needle),
            }
        };

        if !options.whole_word {
            return candidates;
        }

        candidates
            .into_iter()
            .filter(|range| is_whole_word(text, range))
            .collect()
    }
}

/// Разовый поиск без подготовки. Удобен в тестах и на коротком тексте;
/// приложение ходит через `Haystack`, чтобы не складывать регистр заново
/// на каждое нажатие.
#[cfg(test)]
pub fn find_all(haystack: &str, needle: &str, options: Options) -> Vec<Range<usize>> {
    Haystack::new(haystack).find_all(haystack, needle, options)
}

fn exact(haystack: &str, needle: &str) -> Vec<Range<usize>> {
    haystack
        .match_indices(needle)
        .map(|(start, matched)| start..start + matched.len())
        .collect()
}

/// Медленный, но всегда верный путь: идём по символам исходного текста
/// и сравниваем со свёрнутым образцом.
fn scan_by_chars(haystack: &str, folded_needle: &str) -> Vec<Range<usize>> {
    let needle: Vec<char> = folded_needle.chars().collect();
    let mut out = Vec::new();

    for (start, _) in haystack.char_indices() {
        let mut expected = needle.iter();
        let mut end = start;

        for ch in haystack[start..].chars() {
            let Some(&want) = expected.next() else {
                break;
            };
            // Берём первый символ свёртки: для латиницы и кириллицы
            // свёртка всегда односимвольная.
            if ch.to_lowercase().next() != Some(want) {
                end = start;
                break;
            }
            end += ch.len_utf8();
        }

        if end > start && expected.next().is_none() {
            out.push(start..end);
        }
    }

    out
}

/// Стоит ли совпадение отдельным словом.
fn is_whole_word(haystack: &str, range: &Range<usize>) -> bool {
    let before = haystack[..range.start].chars().next_back();
    let after = haystack[range.end..].chars().next();

    !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found<'a>(haystack: &'a str, needle: &str, options: Options) -> Vec<&'a str> {
        find_all(haystack, needle, options)
            .into_iter()
            .map(|range| &haystack[range])
            .collect()
    }

    #[test]
    fn empty_query_finds_nothing() {
        assert!(find_all("текст", "", Options::default()).is_empty());
        assert!(find_all("", "текст", Options::default()).is_empty());
    }

    #[test]
    fn case_insensitive_by_default() {
        let hits = find_all("Ёжик и ёжик и ЁЖИК", "ёжик", Options::default());
        assert_eq!(hits.len(), 3);
        // Смещения должны указывать в исходный текст, а не в свёрнутый.
        assert_eq!(
            found("Ёжик и ёжик и ЁЖИК", "ёжик", Options::default()),
            vec!["Ёжик", "ёжик", "ЁЖИК"]
        );
    }

    #[test]
    fn case_sensitive_when_asked() {
        let options = Options {
            case_sensitive: true,
            whole_word: false,
        };
        assert_eq!(
            found("Ёжик и ёжик", "ёжик", options),
            vec!["ёжик"],
            "с учётом регистра должно найтись только одно"
        );
    }

    #[test]
    fn whole_word_filters_substrings() {
        let options = Options {
            case_sensitive: false,
            whole_word: true,
        };
        // «кот» внутри «котёл» и «шкот» словом не является.
        assert_eq!(found("кот, котёл, шкот, кот!", "кот", options).len(), 2);
    }

    #[test]
    fn whole_word_respects_underscore_and_digits() {
        let options = Options {
            case_sensitive: false,
            whole_word: true,
        };
        assert!(found("my_var", "my", options).is_empty());
        assert!(found("var2", "var", options).is_empty());
        assert_eq!(found("var-2", "var", options).len(), 1);
    }

    #[test]
    fn offsets_are_byte_ranges_into_original() {
        let haystack = "аб test вг test";
        let hits = find_all(haystack, "test", Options::default());
        assert_eq!(hits.len(), 2);
        for range in hits {
            assert_eq!(&haystack[range], "test");
        }
    }

    /// Сторож быстрого пути: если свёртка меняет длину, смещения из неё
    /// брать нельзя, и модуль обязан уйти на посимвольный проход.
    #[test]
    fn fold_that_changes_length_still_gives_valid_offsets() {
        // U+0130 в нижнем регистре становится длиннее на байт.
        let haystack = "İstanbul и istanbul";
        assert_ne!(
            haystack.to_lowercase().len(),
            haystack.len(),
            "тест потерял смысл: свёртка перестала менять длину"
        );

        let hits = find_all(haystack, "istanbul", Options::default());
        // Оба вхождения должны найтись, и оба среза должны быть валидны.
        assert_eq!(hits.len(), 2, "{hits:?}");
        for range in hits {
            let slice = &haystack[range];
            assert!(slice.to_lowercase().ends_with("stanbul"), "{slice}");
        }
    }

    #[test]
    fn overlapping_matches_are_not_reported_twice() {
        // match_indices не выдаёт перекрывающихся совпадений — фиксируем
        // это поведение, чтобы счётчик не разошёлся с подсветкой.
        assert_eq!(found("аааа", "аа", Options::default()).len(), 2);
    }
}
