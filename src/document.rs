//! Всё, что связано с файлом на диске. GUI сюда не заглядывает —
//! этот модуль ничего не знает про egui, его можно тестировать отдельно.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Один открытый markdown-файл.
///
/// Поля приватные, наружу торчат геттеры. Так `rendered` физически
/// не может разъехаться с `source`: обновить одно, забыв другое, нельзя.
pub struct Document {
    path: PathBuf,
    /// Текст ровно как в файле — то, что показываем в режиме "Исходник".
    source: String,
    /// Тот же текст, но с относительными ссылками, развёрнутыми в file:// URI.
    rendered: String,
    /// Время последней записи. `Option`, потому что файл могли удалить.
    modified: Option<SystemTime>,
}

impl Document {
    /// Читает файл с диска.
    ///
    /// Возвращает `Result`: в Rust нет исключений, ошибка — обычное значение,
    /// и вызывающий код обязан её разобрать (или пробросить дальше через `?`).
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let path = path.to_path_buf();
        let source = read_text(&path)?;
        let rendered = absolutize_links(&source, path.parent());
        let modified = mtime(&path);

        Ok(Self {
            path,
            source,
            rendered,
            modified,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn rendered(&self) -> &str {
        &self.rendered
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Имя файла для заголовка окна.
    pub fn title(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// Принудительно перечитать файл.
    pub fn reload(&mut self) -> std::io::Result<()> {
        // Читаем во временные переменные и только потом присваиваем.
        // Иначе borrow checker справедливо ругнётся: справа мы читаем
        // self.path, слева пишем в self.source — нельзя одновременно.
        let source = read_text(&self.path)?;
        let rendered = absolutize_links(&source, self.path.parent());

        self.source = source;
        self.rendered = rendered;
        self.modified = mtime(&self.path);
        Ok(())
    }

    /// Перечитать, только если файл изменился. Возвращает `true`, если перечитали.
    ///
    /// Это "бедный человек" вместо крейта `notify`: раз в полсекунды сравниваем
    /// mtime. Никаких потоков и каналов — для одного файла этого достаточно.
    pub fn reload_if_changed(&mut self) -> bool {
        let current = mtime(&self.path);

        // Файл исчез или время не изменилось — ничего не делаем.
        if current.is_none() || current == self.modified {
            return false;
        }

        self.reload().is_ok()
    }
}

/// Читает файл как текст, не падая на битой кодировке.
///
/// `std::fs::read_to_string` возвращает ошибку, если байты — не UTF-8.
/// Для просмотрщика это плохое поведение: лучше показать текст с "кракозябрами",
/// чем ничего. Поэтому читаем байты и подменяем невалидные последовательности.
fn read_text(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;

    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        // Ошибка не выбрасывает байты — она их владеет и отдаёт обратно.
        Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
    };

    // Убираем BOM, иначе первый заголовок "# ..." не распознается как заголовок.
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
}

fn mtime(path: &Path) -> Option<SystemTime> {
    // `?` работает и с Option: если metadata вернула Err -> ok() даст None -> выходим.
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Переписывает относительные ссылки вида `](picture.png)` в `](file:///abs/path)`.
///
/// Нужно потому, что egui загружает картинки по URI, а `picture.png` для него
/// ничего не значит — он не знает, откуда взялся текст. Ссылки на http(s),
/// якоря `#section` и пути к несуществующим файлам остаются как были.
fn absolutize_links(text: &str, base_dir: Option<&Path>) -> String {
    let Some(base_dir) = base_dir else {
        return text.to_owned();
    };

    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    // Идём по строке срезами (&str), ничего не копируя лишний раз.
    while let Some(start) = rest.find("](") {
        out.push_str(&rest[..start + 2]);
        rest = &rest[start + 2..];

        // let-else: если закрывающей скобки нет, разбирать больше нечего.
        let Some(end) = rest.find(')') else {
            break;
        };

        let link = &rest[..end];
        match resolve_link(link, base_dir) {
            Some(uri) => out.push_str(&uri),
            None => out.push_str(link),
        }
        out.push(')');
        rest = &rest[end + 1..];
    }

    out.push_str(rest);
    out
}

fn resolve_link(link: &str, base_dir: &Path) -> Option<String> {
    let trimmed = link.trim();

    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.contains("://")
        || trimmed.starts_with("mailto:")
    {
        return None;
    }

    // canonicalize заодно проверяет существование файла: если его нет,
    // получаем Err -> ok() -> None -> оставляем ссылку нетронутой.
    let absolute = base_dir.join(trimmed).canonicalize().ok()?;
    let mut path = absolute.to_string_lossy().replace('\\', "/");

    // На Windows canonicalize отдаёт extended-length путь `\\?\C:\...`.
    if let Some(stripped) = path.strip_prefix("//?/") {
        path = stripped.to_owned();
    }

    Some(format!("file:///{}", path.trim_start_matches('/')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_links_are_untouched() {
        let md = "[сайт](https://example.com) и [якорь](#glava-1)";
        assert_eq!(absolutize_links(md, Some(Path::new("/tmp"))), md);
    }

    #[test]
    fn missing_files_are_untouched() {
        let md = "![схема](nope-does-not-exist.png)";
        assert_eq!(absolutize_links(md, Some(Path::new("/tmp"))), md);
    }
}
