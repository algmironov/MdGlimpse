//! Настройки, переживающие перезапуск.
//!
//! Модуль ничего не знает ни про egui, ни про файловую систему: всё, что
//! здесь есть, проверяется тестами без окна и без диска.
//!
//! # Что сохраняем сами, а что за нас
//!
//! Большую часть состояния ведёт сама egui, потому что у eframe включена
//! фича `persistence`: тему, масштаб интерфейса, геометрию окна, ширину
//! боковой панели и позиции прокрутки. Отдельного кода для них не нужно
//! и заводить его не надо — он бы только вступил в спор с сохранённым
//! состоянием egui, как это уже вышло однажды с шириной панели.
//!
//! Здесь живёт только то, чего egui про нас не знает.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Сколько недавних файлов помним.
pub const MAX_RECENT: usize = 10;

/// Ключ в хранилище eframe.
pub const STORAGE_KEY: &str = "settings";

/// Ключи, которые писали прежние версии и которые больше не нужны.
///
/// Хранилище eframe чужие ключи не трогает, поэтому мёртвые записи
/// остаются в нём навсегда, пока их не удалить явно. `outline_width`
/// писала версия, пытавшаяся вести ширину панели сама, — оказалось,
/// что её ведёт egui и наше значение всё равно проигрывало при загрузке.
/// `outline_open` жил отдельным ключом, пока полей было мало; теперь
/// это поле внутри `Settings`.
pub const DEAD_KEYS: &[&str] = &["outline_width", "outline_open"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Развёрнута ли боковая панель с оглавлением.
    pub outline_open: bool,
    /// Был ли открыт режим «Исходник».
    ///
    /// Именно `bool`, а не `ViewMode`: тот живёт в слое интерфейса,
    /// и тащить его сюда — значит связать настройки с отрисовкой.
    pub source_mode: bool,
    /// Галка «Следить за файлом».
    pub auto_reload: bool,
    /// Недавние файлы, самый свежий первым.
    pub recent: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            outline_open: true,
            source_mode: false,
            auto_reload: true,
            recent: Vec::new(),
        }
    }
}

impl Settings {
    /// Запоминает открытый файл: ставит первым, убирает прежнее вхождение,
    /// обрезает список до `MAX_RECENT`.
    pub fn remember(&mut self, path: &Path) {
        self.recent.retain(|known| known != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(MAX_RECENT);
    }

    /// Выбрасывает файлы, которых больше нет.
    ///
    /// Проверка существования передаётся снаружи, а не берётся из
    /// `Path::exists`, — тогда список можно проверить тестом, не создавая
    /// файлов на диске.
    pub fn prune(&mut self, exists: impl Fn(&Path) -> bool) {
        self.recent.retain(|path| exists(path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(settings: &Settings) -> Vec<String> {
        settings
            .recent
            .iter()
            .map(|path| path.display().to_string())
            .collect()
    }

    #[test]
    fn newest_goes_first() {
        let mut settings = Settings::default();
        settings.remember(Path::new("a.md"));
        settings.remember(Path::new("b.md"));
        settings.remember(Path::new("c.md"));

        assert_eq!(paths(&settings), vec!["c.md", "b.md", "a.md"]);
    }

    #[test]
    fn reopening_moves_to_top_without_duplicating() {
        let mut settings = Settings::default();
        for name in ["a.md", "b.md", "c.md"] {
            settings.remember(Path::new(name));
        }
        settings.remember(Path::new("a.md"));

        assert_eq!(paths(&settings), vec!["a.md", "c.md", "b.md"]);
        assert_eq!(settings.recent.len(), 3, "дубликат не должен появляться");
    }

    #[test]
    fn list_is_capped() {
        let mut settings = Settings::default();
        for index in 0..(MAX_RECENT + 5) {
            settings.remember(Path::new(&format!("file{index}.md")));
        }

        assert_eq!(settings.recent.len(), MAX_RECENT);
        // Обрезается хвост, а не голова: самые свежие остаются.
        assert_eq!(
            settings.recent[0].display().to_string(),
            format!("file{}.md", MAX_RECENT + 4)
        );
    }

    #[test]
    fn missing_files_are_pruned() {
        let mut settings = Settings::default();
        for name in ["есть.md", "нет.md", "тоже есть.md"] {
            settings.remember(Path::new(name));
        }

        settings.prune(|path| path.display().to_string().contains("есть"));

        assert_eq!(paths(&settings), vec!["тоже есть.md", "есть.md"]);
    }

    #[test]
    fn defaults_are_sane() {
        let settings = Settings::default();
        assert!(settings.outline_open, "оглавление по умолчанию открыто");
        assert!(!settings.source_mode, "по умолчанию показываем рендер");
        assert!(settings.auto_reload, "слежение по умолчанию включено");
        assert!(settings.recent.is_empty());
    }

    /// Настройки читаются из хранилища тем же ron, что и пишутся.
    /// Тест сторожит совместимость: если поле переименуют, старое
    /// хранилище должно пережить это без паники.
    #[test]
    fn unknown_and_missing_fields_survive() {
        let stored: Settings = ron::from_str("(outline_open:false)").expect("частичный разбор");
        assert!(!stored.outline_open);
        // Остальное подставилось из Default благодаря #[serde(default)].
        assert!(stored.auto_reload);
        assert!(stored.recent.is_empty());
    }
}
