//! Всё, что связано с файлом на диске. GUI сюда не заглядывает —
//! этот модуль ничего не знает про egui, его можно тестировать отдельно.

use crate::outline::{self, Heading};
use std::io::Read as _;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Сколько байт с начала файла нюхаем, решая, текст это или нет.
///
/// Именно «с начала», а не весь файл: у трёхгигабайтного `.exe` мы должны
/// понять, что это не текст, не прочитав его целиком.
const SNIFF_BYTES: usize = 8 * 1024;

/// Расширения, при которых имеет смысл рендерить markdown.
///
/// Всё остальное текстовое (`.toml`, `.txt`, `.rs`) открывать можно,
/// но показывать сразу исходником — рендерить `Cargo.toml` как markdown
/// бессмысленно.
const MARKDOWN_EXTENSIONS: &[&str] = &["md", "markdown", "mdown", "mkd", "mdwn", "mkdn"];

/// Доля управляющих C0-символов, выше которой это уже не текст.
///
/// У текста в любой восьмибитной кодировке их ровно ноль, так что порог
/// щедрый: он на случай файла, который «почти текст», но с мусором внутри.
const MAX_CONTROL_SHARE: f32 = 0.05;

/// Доля `U+FFFD` после lossy-декодирования, выше которой это не текст.
///
/// Порог намеренно очень высокий, и вот почему. Соблазнительно считать
/// «много невалидного UTF-8 — значит бинарник», но русский текст в CP1251
/// невалиден для UTF-8 примерно на 60 %: каждая кириллическая буква там —
/// одиночный байт 0xC0–0xFF. Порог вроде 30 % отверг бы ровно те файлы,
/// ради которых lossy-чтение и делалось. Настоящий бинарник от текста
/// в чужой кодировке отделяет не эта проверка, а нулевой байт и доля
/// управляющих символов.
const MAX_REPLACEMENT_SHARE: f32 = 0.90;

/// Размер, выше которого markdown не рендерится сам собой.
///
/// Откуда число. `egui_commonmark` строит виджеты для всего документа
/// целиком — виртуализации в рендере нет. Замер на файле 5 МБ дал
/// 364 МБ рабочего набора против 160 МБ у пустого окна, то есть примерно
/// 40 байт памяти на байт исходника. При 2 МиБ надбавка укладывается
/// в ~80 МБ, и это ещё терпимо; дальше растёт быстрее, чем полезность.
pub const MAX_RENDER_BYTES: u64 = 2 * 1024 * 1024;

/// Жёсткий потолок: файл крупнее не открывается вовсе.
///
/// Проверка на бинарность отсекает `.exe`, но ничего не говорит про
/// гигантский текстовый файл — а `fs::read` на нём попытается выделить
/// столько же памяти, сколько файл весит, и убьёт процесс.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Почему файл сочтён не текстом.
///
/// Отдельный enum, а не строка: так вызывающий код может решать по варианту,
/// а не разбирать сообщение, а текст сообщения живёт в одном месте.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotTextReason {
    /// Нулевой байт. Самый надёжный признак: в тексте его не бывает никогда.
    NulByte,
    /// Слишком много управляющих символов.
    ControlBytes,
    /// Почти всё содержимое не декодируется ни во что осмысленное.
    BrokenEncoding,
}

impl std::fmt::Display for NotTextReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::NulByte => "внутри нулевые байты",
            Self::ControlBytes => "слишком много управляющих символов",
            Self::BrokenEncoding => "содержимое не похоже на текст ни в одной кодировке",
        };
        f.write_str(text)
    }
}

/// Ошибка уровня приложения.
///
/// До этого везде был голый `std::io::Error`, и «файл не читается» было
/// не отличить от «файл не текстовый». Свой enum разделяет эти случаи.
///
/// В настоящем проекте такой тип обычно генерирует макрос `thiserror`.
/// Здесь он написан руками — ровно чтобы было видно, из чего он состоит:
/// сам enum, `Display` для человекочитаемого текста, `Error` для встраивания
/// в чужие цепочки ошибок и `From` ради того, чтобы `?` продолжал работать.
#[derive(Debug)]
pub enum DocumentError {
    /// Файл не открылся: нет прав, исчез, отвалился сетевой диск.
    Io(std::io::Error),
    /// Файл открылся, но это не текст.
    NotText(NotTextReason),
    /// Файл текстовый, но такой, что его нельзя прочитать не подавившись.
    TooLarge { size: u64, limit: u64 },
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "не удалось прочитать файл: {err}"),
            Self::NotText(reason) => write!(f, "это не текстовый файл ({reason})"),
            Self::TooLarge { size, limit } => write!(
                f,
                "файл слишком большой: {} против допустимых {}",
                human_size(*size),
                human_size(*limit)
            ),
        }
    }
}

impl std::error::Error for DocumentError {
    /// Возвращает «причину под причиной». Благодаря этому внешний код может
    /// раскрутить всю цепочку ошибок, не зная про наши внутренности.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::NotText(_) | Self::TooLarge { .. } => None,
        }
    }
}

/// Ради этого `?` в теле `load` продолжает работать так же, как раньше:
/// оператор сам вызывает `From::from` для ошибки, которую пробрасывает.
impl From<std::io::Error> for DocumentError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// Один открытый файл.
///
/// Поля приватные, наружу торчат геттеры. Так `rendered` физически
/// не может разъехаться с `source`: обновить одно, забыв другое, нельзя.
pub struct Document {
    path: PathBuf,
    /// Текст ровно как в файле — то, что показываем в режиме "Исходник".
    source: String,
    /// Тот же текст, но подготовленный к показу: к заголовкам дописаны
    /// якоря, относительные ссылки развёрнуты в file:// URI.
    rendered: String,
    /// Заголовки документа — для боковой панели с оглавлением.
    headings: Vec<Heading>,
    /// Время последней записи. `Option`, потому что файл могли удалить.
    modified: Option<SystemTime>,
    /// Стоит ли вообще предлагать режим рендера. Считается один раз
    /// по расширению — путь между перечитываниями не меняется.
    is_markdown: bool,
    /// Ширина самой длинной строки в знакоместах.
    ///
    /// Считается один раз при загрузке — по ней в режиме «Исходник»
    /// вычисляется ширина колонки. Каждый кадр перебирать строки
    /// пятимегабайтного файла ради этого было бы расточительством.
    max_line_columns: usize,
    /// Границы строк внутри `source`.
    ///
    /// Именно диапазоны, а не `Vec<String>`: копия всех строк удвоила бы
    /// память на ровном месте. Диапазон — это пара чисел, текст остаётся
    /// на месте, а `line()` отдаёт срез в него.
    line_ranges: Vec<Range<usize>>,
}

impl Document {
    /// Читает файл с диска.
    ///
    /// Возвращает `Result`: в Rust нет исключений, ошибка — обычное значение,
    /// и вызывающий код обязан её разобрать (или пробросить дальше через `?`).
    pub fn load(path: &Path) -> Result<Self, DocumentError> {
        // Размер узнаём из метаданных — до чтения, а не после.
        let size = std::fs::metadata(path)?.len();
        if size > MAX_FILE_BYTES {
            return Err(DocumentError::TooLarge {
                size,
                limit: MAX_FILE_BYTES,
            });
        }

        // Дальше нюхаем начало файла и только потом читаем целиком.
        if let Some(reason) = sniff_binary(&read_head(path)?) {
            return Err(DocumentError::NotText(reason));
        }

        let path = path.to_path_buf();
        let source = read_text(&path)?;
        let headings = outline::extract(&source);
        let rendered = prepare_for_render(&source, path.parent());
        let modified = mtime(&path);
        let is_markdown = has_markdown_extension(&path);
        let line_ranges = line_ranges(&source);
        let max_line_columns = max_line_columns(&source, &line_ranges);

        Ok(Self {
            path,
            source,
            rendered,
            modified,
            is_markdown,
            line_ranges,
            max_line_columns,
            headings,
        })
    }

    pub fn rendered(&self) -> &str {
        &self.rendered
    }

    /// Весь исходный текст. Нужен для копирования в буфер обмена:
    /// показываем документ построчно, а копируем целиком.
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Стоит ли показывать этот файл отрендеренным.
    pub fn is_markdown(&self) -> bool {
        self.is_markdown
    }

    /// Размер исходника в байтах.
    pub fn size_bytes(&self) -> u64 {
        self.source.len() as u64
    }

    /// Настолько ли файл велик, что рендерить его по умолчанию не стоит.
    pub fn is_large(&self) -> bool {
        self.size_bytes() > MAX_RENDER_BYTES
    }

    /// Сколько строк в исходнике.
    pub fn line_count(&self) -> usize {
        self.line_ranges.len()
    }

    /// Длина самой длинной строки в символах.
    pub fn max_line_columns(&self) -> usize {
        self.max_line_columns
    }

    /// Заголовки документа в порядке появления.
    pub fn headings(&self) -> &[Heading] {
        &self.headings
    }

    /// Байтовые границы строки внутри `source`.
    ///
    /// Нужны поиску: совпадения хранятся смещениями в весь текст,
    /// а рисуются построчно.
    pub fn line_range(&self, index: usize) -> Range<usize> {
        self.line_ranges.get(index).cloned().unwrap_or(0..0)
    }

    /// Номер строки, в которую попадает байтовое смещение.
    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.line_ranges
            .partition_point(|range| range.start <= offset)
            .saturating_sub(1)
    }

    /// Строка по номеру, без копирования: это срез внутрь `source`.
    pub fn line(&self, index: usize) -> &str {
        self.line_ranges
            .get(index)
            .map_or("", |range| &self.source[range.clone()])
    }

    /// Имя файла для заголовка окна.
    pub fn title(&self) -> String {
        file_name(&self.path)
    }

    /// Принудительно перечитать файл.
    pub fn reload(&mut self) -> Result<(), DocumentError> {
        // Перепроверяем на бинарность: файл мог быть подменён на диске
        // между открытием и перечитыванием.
        if let Some(reason) = sniff_binary(&read_head(&self.path)?) {
            return Err(DocumentError::NotText(reason));
        }

        // Читаем во временные переменные и только потом присваиваем.
        // Иначе borrow checker справедливо ругнётся: справа мы читаем
        // self.path, слева пишем в self.source — нельзя одновременно.
        let source = read_text(&self.path)?;
        let headings = outline::extract(&source);
        let rendered = prepare_for_render(&source, self.path.parent());

        self.headings = headings;
        self.line_ranges = line_ranges(&source);
        self.max_line_columns = max_line_columns(&source, &self.line_ranges);
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

        // Ошибку здесь глотаем сознательно: мы попали в момент, когда редактор
        // ещё дописывает файл. Показывать из-за этого ошибку в окне — мигание
        // на ровном месте; на следующем опросе через полсекунды всё получится.
        // Явное перечитывание по Ctrl+R об ошибках, наоборот, сообщает.
        self.reload().is_ok()
    }
}

/// Границы строк внутри текста, в байтах.
///
/// Считается один раз при загрузке. Каждый кадр заново делить пятимегабайтный
/// файл на строки — ровно та работа, которой виртуализация и должна избежать.
fn line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;

    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            // Файлы с CRLF: возврат каретки в саму строку не включаем,
            // иначе он приедет в буфер обмена при копировании.
            let mut end = index;
            if end > start && text.as_bytes()[end - 1] == b'\r' {
                end -= 1;
            }
            ranges.push(start..end);
            start = index + 1;
        }
    }

    // Хвост после последнего перевода строки. Если файл кончается переводом
    // строки, пустой хвост не добавляем — иначе внизу висела бы лишняя строка.
    if start < text.len() {
        ranges.push(start..text.len());
    }

    ranges
}

/// Сколько знакомест занимает табуляция.
///
/// Не выдумано: epaint отрисовывает `\t` фиксированным сдвигом
/// `FontTweak::tab_size * ширина пробела`, а `tab_size` по умолчанию равен
/// четырём (см. `epaint/src/text/font.rs`). Это именно фиксированный сдвиг,
/// а не переход к следующей табстопе, поэтому позицию таба в строке знать
/// не нужно — достаточно посчитать его за четыре знакоместа.
const TAB_COLUMNS: usize = 4;

/// Ширина самой длинной строки в знакоместах.
///
/// В знакоместах, а не в байтах: кириллица в UTF-8 занимает два байта,
/// и по длине в байтах колонка вышла бы вдвое шире нужного. И не просто
/// в символах: таб рисуется шире остальных, и файл с табами давал колонку
/// уже настоящей — строка уезжала за правый край без прокрутки.
///
/// Точность держится на том, что шрифт моноширинный. Символы двойной
/// ширины (иероглифы, часть эмодзи) в неё не укладываются и по-прежнему
/// дадут заниженную оценку; лечится это только настоящим обмером строк,
/// который на пятимегабайтном файле стоит слишком дорого.
fn max_line_columns(text: &str, ranges: &[Range<usize>]) -> usize {
    ranges
        .iter()
        .map(|range| {
            text[range.clone()]
                .chars()
                .map(|ch| if ch == '\t' { TAB_COLUMNS } else { 1 })
                .sum()
        })
        .max()
        .unwrap_or(0)
}

/// Размер в килобайтах или мегабайтах — для сообщений человеку.
fn human_size(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    if bytes >= MIB {
        format!("{:.1} МБ", bytes as f64 / MIB as f64)
    } else {
        format!("{} КБ", bytes / 1024)
    }
}

/// Имя файла для показа человеку.
///
/// Вынесено из `Document::title` наружу, потому что сообщение об ошибке
/// нужно как раз тогда, когда `Document` создать не удалось.
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Читает начало файла — не больше `SNIFF_BYTES`.
fn read_head(path: &Path) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;

    let mut head = Vec::with_capacity(SNIFF_BYTES);
    // `take` съедает `file` по значению и отдаёт обёртку, которая физически
    // не может прочитать больше лимита. Это не проверка после чтения,
    // а ограничение самого чтения.
    file.take(SNIFF_BYTES as u64).read_to_end(&mut head)?;

    Ok(head)
}

/// Похоже ли начало файла на бинарное содержимое.
///
/// `None` — это текст, `Some(reason)` — нет.
fn sniff_binary(head: &[u8]) -> Option<NotTextReason> {
    // Пустой файл — вполне законный пустой текст.
    if head.is_empty() {
        return None;
    }

    if head.contains(&0) {
        return Some(NotTextReason::NulByte);
    }

    let controls = head.iter().filter(|byte| is_stray_control(**byte)).count();
    if controls as f32 / head.len() as f32 > MAX_CONTROL_SHARE {
        return Some(NotTextReason::ControlBytes);
    }

    // Если окно упёрлось в лимит, последний символ могло разрезать пополам.
    // Это граница окна, а не порча кодировки, — отбрасываем хвост.
    // Максимальная длина символа UTF-8 — четыре байта, значит хватит трёх.
    let body = if head.len() == SNIFF_BYTES {
        &head[..head.len() - 3]
    } else {
        head
    };

    let text = String::from_utf8_lossy(body);
    let total = text.chars().count();
    if total > 0 {
        let broken = text
            .chars()
            .filter(|ch| *ch == char::REPLACEMENT_CHARACTER)
            .count();
        if broken as f32 / total as f32 > MAX_REPLACEMENT_SHARE {
            return Some(NotTextReason::BrokenEncoding);
        }
    }

    None
}

/// Управляющий символ, которому нечего делать в текстовом файле.
///
/// Табуляция, перевод строки, возврат каретки и подача страницы — законные,
/// они встречаются в настоящих текстах.
fn is_stray_control(byte: u8) -> bool {
    (byte < 0x20 && !matches!(byte, b'\t' | b'\n' | b'\r' | 0x0c)) || byte == 0x7f
}

fn has_markdown_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|ext| MARKDOWN_EXTENSIONS.contains(&ext.as_str()))
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

/// Готовит текст к показу в `egui_commonmark`.
///
/// Порядок важен. Якоря вставляются по смещениям, посчитанным разбором
/// исходника, поэтому вставлять их надо ДО того, как переписывание ссылок
/// поменяет длину текста и сдвинет все смещения.
fn prepare_for_render(source: &str, base_dir: Option<&Path>) -> String {
    absolutize_links(&outline::inject_anchors(source), base_dir)
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Уникальный каталог во временной папке.
    ///
    /// Тесты в Rust по умолчанию идут параллельно в потоках одного процесса,
    /// поэтому одного фиксированного имени на всех мало — добавляем pid и счётчик.
    fn temp_dir_for(tag: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("mdglimpse-test-{}-{tag}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("создать временный каталог");
        dir
    }

    /// Кладёт файл с заданными байтами и пытается его открыть.
    fn load_bytes(tag: &str, name: &str, bytes: &[u8]) -> Result<Document, DocumentError> {
        let dir = temp_dir_for(tag);
        let file = dir.join(name);
        std::fs::write(&file, bytes).expect("создать файл");

        let result = Document::load(&file);
        std::fs::remove_dir_all(&dir).ok();
        result
    }

    #[test]
    fn external_links_are_untouched() {
        let md = "[сайт](https://example.com) и [якорь](#glava-1)";
        let tmp = std::env::temp_dir();
        assert_eq!(absolutize_links(md, Some(tmp.as_path())), md);
    }

    #[test]
    fn missing_files_are_untouched() {
        let md = "![схема](nope-does-not-exist.png)";
        let tmp = std::env::temp_dir();
        assert_eq!(absolutize_links(md, Some(tmp.as_path())), md);
    }

    /// Ради этого теста всё и затевалось: относительная ссылка на реально
    /// существующий файл должна стать корректным `file:///` URI.
    ///
    /// Самое хрупкое место — Windows: `canonicalize` возвращает
    /// extended-length путь вида `\\?\C:\...`, и если его не срезать,
    /// загрузчик картинок получит мусор и молча ничего не покажет.
    #[test]
    fn existing_file_becomes_file_uri() {
        let dir = temp_dir_for("uri");
        std::fs::write(dir.join("picture.png"), b"not really a png").expect("создать файл");

        let out = absolutize_links("![схема](picture.png)", Some(dir.as_path()));

        // Прибираем за собой до проверок: упавший assert не должен оставлять мусор.
        std::fs::remove_dir_all(&dir).ok();

        let uri = out
            .strip_prefix("![схема](")
            .and_then(|rest| rest.strip_suffix(')'))
            .expect("разметка вокруг ссылки поехала");

        assert!(uri.starts_with("file:///"), "нет схемы file://: {uri}");
        assert!(uri.ends_with("/picture.png"), "потерялось имя файла: {uri}");
        assert!(!uri.contains('\\'), "остались обратные слэши: {uri}");
        assert!(
            !uri.contains('?'),
            "не срезан extended-length префикс: {uri}"
        );

        // Под Windows после `file:///` обязана идти буква диска: `file:///C:/...`.
        #[cfg(windows)]
        {
            let mut chars = uri["file:///".len()..].chars();
            let drive = chars.next().expect("пустой путь после схемы");
            assert!(drive.is_ascii_alphabetic(), "нет буквы диска: {uri}");
            assert_eq!(chars.next(), Some(':'), "нет двоеточия после диска: {uri}");
        }
    }

    #[test]
    fn binary_file_is_rejected() {
        // Заголовок настоящего PE-файла: "MZ", дальше нули.
        let mut bytes = b"MZ\x90\x00\x03".to_vec();
        bytes.extend(std::iter::repeat_n(0u8, 512));

        let Err(err) = load_bytes("bin", "fake.exe", &bytes) else {
            panic!("бинарник должен быть отвергнут");
        };
        assert!(
            matches!(err, DocumentError::NotText(NotTextReason::NulByte)),
            "ожидали отказ по нулевому байту, получили {err:?}"
        );
    }

    /// Бинарник без единого нулевого байта — на нём должна сработать
    /// вторая проверка, по доле управляющих символов.
    #[test]
    fn control_bytes_are_rejected() {
        let bytes: Vec<u8> = (1u8..=31).cycle().take(1024).collect();

        let Err(err) = load_bytes("ctrl", "noise.bin", &bytes) else {
            panic!("мусор должен быть отвергнут");
        };
        assert!(
            matches!(err, DocumentError::NotText(NotTextReason::ControlBytes)),
            "ожидали отказ по управляющим символам, получили {err:?}"
        );
    }

    #[test]
    fn utf8_cyrillic_is_accepted() {
        let document = load_bytes(
            "utf8",
            "text.md",
            "# Заголовок\n\nПривет, мир!\n".as_bytes(),
        )
        .expect("UTF-8 с кириллицей должен читаться");

        assert!(document.source().contains("Привет, мир!"));
        assert!(document.is_markdown());
    }

    /// Один битый байт посреди нормального текста — это не повод отказывать.
    /// Так выглядит, например, файл, склеенный из двух кодировок.
    #[test]
    fn single_broken_byte_is_tolerated() {
        let mut bytes = "Обычный русский текст, в котором ".as_bytes().to_vec();
        bytes.push(0xff);
        bytes.extend_from_slice(" один байт испорчен.".as_bytes());

        let document = load_bytes("broken", "text.md", &bytes)
            .expect("один битый байт не делает файл бинарным");

        assert!(document.source().contains("Обычный русский текст"));
        assert!(document.source().contains("один байт испорчен"));
    }

    /// Сторож компромисса из `MAX_REPLACEMENT_SHARE`.
    ///
    /// Русский текст в CP1251 для UTF-8 невалиден примерно на 60 %.
    /// Если кто-то решит «ужесточить» порог невалидных последовательностей,
    /// этот тест упадёт первым и объяснит, почему так делать нельзя.
    #[test]
    fn cp1251_russian_text_is_accepted() {
        // "Привет, мир! Это текст в кодировке CP1251." — кириллица одним байтом.
        let bytes: Vec<u8> = vec![
            0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2, 0x2c, 0x20, 0xec, 0xe8, 0xf0, 0x21, 0x20, 0xdd,
            0xf2, 0xee, 0x20, 0xf2, 0xe5, 0xea, 0xf1, 0xf2, 0x20, 0xe2, 0x20, 0xea, 0xee, 0xe4,
            0xe8, 0xf0, 0xee, 0xe2, 0xea, 0xe5, 0x20, 0x43, 0x50, 0x31, 0x32, 0x35, 0x31, 0x2e,
            0x0a,
        ];

        let document = load_bytes("cp1251", "text.md", &bytes)
            .expect("текст в CP1251 не бинарник, отвергать его нельзя");

        // Читается он «кракозябрами» — это ожидаемо и задокументировано
        // в read_text. Важно, что файл вообще открылся.
        assert!(document.source().contains("CP1251"));
    }

    #[test]
    fn line_index_handles_crlf_and_tail() {
        // LF, CRLF, пустая строка и хвост без перевода строки — всё сразу.
        let text = "первая\nвторая\r\n\nхвост";
        let ranges = line_ranges(text);
        let lines: Vec<&str> = ranges.iter().map(|r| &text[r.clone()]).collect();

        assert_eq!(lines, vec!["первая", "вторая", "", "хвост"]);
        // Возврат каретки не должен попасть в строку: иначе он приедет
        // в буфер обмена при копировании.
        assert!(!lines.iter().any(|line| line.contains('\r')));
    }

    #[test]
    fn trailing_newline_does_not_add_empty_line() {
        assert_eq!(line_ranges("одна\n").len(), 1);
        assert_eq!(line_ranges("одна\nдве\n").len(), 2);
        assert_eq!(line_ranges("").len(), 0);
    }

    #[test]
    fn tabs_count_as_four_columns() {
        // Именно ради этого теста таб перестал считаться за один символ:
        // без него строка с табами уезжала за правый край без прокрутки.
        let text = "ab\tc";
        let ranges = line_ranges(text);
        assert_eq!(max_line_columns(text, &ranges), 2 + 4 + 1);

        // Самой длинной должна оказаться строка с табами, хотя символов в ней меньше.
        let text = "1234567890\n\t\t\tx";
        let ranges = line_ranges(text);
        assert_eq!(max_line_columns(text, &ranges), 13);
    }

    #[test]
    fn max_line_columns_counts_characters_not_bytes() {
        // Кириллица в UTF-8 занимает два байта — колонка вышла бы вдвое шире.
        let text = "привет";
        let ranges = line_ranges(text);
        assert_eq!(max_line_columns(text, &ranges), 6);
    }

    #[test]
    fn non_markdown_extension_is_detected() {
        let document = load_bytes("toml", "Cargo.toml", b"[package]\nname = \"mdglimpse\"\n")
            .expect("toml — это текст, открывать можно");

        assert!(
            !document.is_markdown(),
            "Cargo.toml не должен считаться markdown"
        );
    }
}
