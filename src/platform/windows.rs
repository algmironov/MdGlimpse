//! Windows: ProgID в `HKCU\Software\Classes` плюс запись в `OpenWithProgids`.
//!
//! # Почему только HKCU
//!
//! `HKLM` требует прав администратора, а просмотрщик markdown — не та
//! программа, ради которой стоит показывать запрос UAC. В `HKCU` пишет
//! сам пользователь для себя, и это ровно тот масштаб, который тут нужен.
//!
//! # Что именно создаётся
//!
//! ```text
//! HKCU\Software\Classes\MdGlimpse.markdown              (по умолчанию) = "Документ Markdown"
//! HKCU\Software\Classes\MdGlimpse.markdown\DefaultIcon  (по умолчанию) = "C:\...\mdglimpse.exe,0"
//! HKCU\Software\Classes\MdGlimpse.markdown\shell\open\command
//!                                                       (по умолчанию) = "\"C:\...\mdglimpse.exe\" \"%1\""
//! HKCU\Software\Classes\.md\OpenWithProgids             значение "MdGlimpse.markdown" (пустое)
//! HKCU\Software\Classes\.markdown\OpenWithProgids       значение "MdGlimpse.markdown" (пустое)
//! ```
//!
//! Кавычки вокруг `%1` обязательны: без них путь с пробелом приедет
//! в программу разрезанным на несколько аргументов.
//!
//! # Что удаляется при отмене
//!
//! Дерево `MdGlimpse.markdown` целиком — оно наше и создано нами. А вот из
//! `OpenWithProgids` удаляется **только своё значение**, не ключ: в нём
//! перечислены все программы, умеющие открывать это расширение, и снести
//! его целиком значило бы выкинуть из списка чужие.

use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, KEY_WRITE, REG_NONE,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

use super::{Association, Error, Result, EXTENSIONS};

/// Наш ProgID. Точка в середине — соглашение вида «Вендор.Тип»,
/// оно же гарантия, что мы не наступим на чужой ключ.
///
/// Имя обязано отличаться от того, что мог бы занять проект-близнец
/// на .NET: два приложения с одинаковым ProgID перетирали бы друг другу
/// команду запуска, и последний зарегистрировавшийся забирал бы файлы себе.
const PROG_ID: &str = "MdGlimpse.markdown";

/// Подпись, которую человек увидит в проводнике в колонке «Тип».
const FRIENDLY_TYPE: &str = "Документ Markdown";

const CLASSES: &str = "Software\\Classes";

/// Строка Rust -> нуль-терминированная UTF-16 для Win32.
///
/// Windows-API существует в двух видах: A (однобайтный, кодовая страница)
/// и W (UTF-16). Берём только W: в пути к файлу может быть что угодно,
/// и на кодовой странице оно сломается.
fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(once(0)).collect()
}

/// Обёртка над HKEY, закрывающая ключ при выходе из области видимости.
///
/// Ровно то, ради чего в Rust существует Drop: возвращаемся ли мы
/// нормально или через `?` по ошибке, `RegCloseKey` вызовется сам.
/// В C такие утечки ловят годами.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: сюда попадает только успешно открытый ключ, и ровно один раз.
        unsafe { RegCloseKey(self.0) };
    }
}

fn check(status: WIN32_ERROR, what: &str) -> Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(Error::Os(format!("{what}: код ошибки Windows {status}")))
    }
}

/// Создаёт (или открывает существующий) подключ HKCU.
fn create(path: &str) -> Result<Key> {
    let mut handle: HKEY = std::ptr::null_mut();
    // SAFETY: путь нуль-терминирован, handle — валидный указатель на HKEY.
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(path).as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &mut handle,
            std::ptr::null_mut(),
        )
    };
    check(status as WIN32_ERROR, &format!("создание ключа {path}"))?;
    Ok(Key(handle))
}

fn open(path: &str, access: u32) -> Option<Key> {
    let mut handle: HKEY = std::ptr::null_mut();
    // SAFETY: то же, что и в create.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(path).as_ptr(),
            0,
            access,
            &mut handle,
        )
    };
    (status as WIN32_ERROR == ERROR_SUCCESS).then_some(Key(handle))
}

/// Пишет строковое значение. `name = None` — значение по умолчанию.
fn set_string(key: &Key, name: Option<&str>, value: &str) -> Result<()> {
    let data = wide(value);
    let name = name.map(wide);
    // Длина в БАЙТАХ, включая нулевой символ: RegSetValueExW не знает,
    // что мы пишем UTF-16, для него это просто буфер.
    let bytes = std::mem::size_of_val(&data[..]);
    // SAFETY: data жив до конца вызова, длина посчитана по нему же.
    let status = unsafe {
        RegSetValueExW(
            key.0,
            name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
            0,
            REG_SZ,
            data.as_ptr().cast(),
            bytes as u32,
        )
    };
    check(status as WIN32_ERROR, "запись значения")
}

/// Читает строковое значение. Ошибку и отсутствие не различаем:
/// вызывающему в обоих случаях нужен один и тот же ответ.
fn get_string(path: &str, name: Option<&str>) -> Option<String> {
    let key = open(path, KEY_READ)?;
    let name = name.map(wide);
    let name_ptr = name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr());

    // Сначала спрашиваем размер, потом читаем: классический двухшаговый
    // танец Win32, размер заранее неизвестен.
    let mut bytes: u32 = 0;
    // SAFETY: буфера нет, функция только возвращает нужный размер.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            name_ptr,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    if status as WIN32_ERROR != ERROR_SUCCESS || bytes == 0 {
        return None;
    }

    let mut buffer = vec![0u16; bytes.div_ceil(2) as usize];
    let mut size = bytes;
    // SAFETY: buffer вмещает size байт, что и сообщается функции.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            name_ptr,
            std::ptr::null(),
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status as WIN32_ERROR != ERROR_SUCCESS {
        return None;
    }

    // Реестр не обязан хранить строку с нулём на конце, но обычно хранит.
    let len = buffer
        .iter()
        .position(|&ch| ch == 0)
        .unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]))
}

fn exe_path() -> Result<String> {
    let path = std::env::current_exe()
        .map_err(|error| Error::Os(format!("не удалось узнать путь к программе: {error}")))?;
    Ok(path.display().to_string())
}

/// Строка команды запуска — она же то, с чем сверяется [`state`].
fn open_command(exe: &str) -> String {
    format!("\"{exe}\" \"%1\"")
}

pub fn register() -> Result<()> {
    let exe = exe_path()?;

    let root = create(&format!("{CLASSES}\\{PROG_ID}"))?;
    set_string(&root, None, FRIENDLY_TYPE)?;
    // FriendlyAppName — то, что видно в списке «Открыть с помощью».
    // Без него Windows подставит имя файла, то есть «mdglimpse.exe».
    set_string(&root, Some("FriendlyAppName"), "MdGlimpse")?;

    let icon = create(&format!("{CLASSES}\\{PROG_ID}\\DefaultIcon"))?;
    // ",0" — индекс иконки внутри .exe. Наша единственная и первая.
    set_string(&icon, None, &format!("{exe},0"))?;

    let command = create(&format!("{CLASSES}\\{PROG_ID}\\shell\\open\\command"))?;
    set_string(&command, None, &open_command(&exe))?;

    for extension in EXTENSIONS {
        let key = create(&format!("{CLASSES}\\.{extension}\\OpenWithProgids"))?;
        // Значение пустое и типа REG_NONE: смысл несёт само его имя.
        // Так это описано в документации Microsoft, и так лежат в этом
        // ключе записи остальных программ.
        // SAFETY: имя нуль-терминировано, данных нет — длина 0.
        let status = unsafe {
            RegSetValueExW(
                key.0,
                wide(PROG_ID).as_ptr(),
                0,
                REG_NONE,
                std::ptr::null(),
                0,
            )
        };
        check(
            status as WIN32_ERROR,
            &format!("добавление .{extension} в OpenWithProgids"),
        )?;
    }

    notify_shell();
    Ok(())
}

pub fn unregister() -> Result<()> {
    // Сначала убираем себя из чужих списков, потом сносим своё дерево:
    // если что-то пойдёт не так посередине, лучше остаться с осиротевшим
    // ProgID, чем со ссылкой на несуществующий.
    for extension in EXTENSIONS {
        let path = format!("{CLASSES}\\.{extension}\\OpenWithProgids");
        let Some(key) = open(&path, KEY_SET_VALUE) else {
            continue; // ключа нет — удалять нечего.
        };
        // SAFETY: ключ открыт на запись, имя нуль-терминировано.
        let status = unsafe { RegDeleteValueW(key.0, wide(PROG_ID).as_ptr()) } as WIN32_ERROR;
        if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
            return Err(Error::Os(format!(
                "не удалось убрать .{extension} из OpenWithProgids: код ошибки Windows {status}"
            )));
        }
    }

    // RegDeleteTreeW сносит ключ вместе с подключами: DefaultIcon
    // и shell\open\command уйдут вместе с корнем.
    // SAFETY: путь нуль-терминирован, удаляем только своё поддерево.
    let status = unsafe {
        RegDeleteTreeW(
            HKEY_CURRENT_USER,
            wide(&format!("{CLASSES}\\{PROG_ID}")).as_ptr(),
        )
    } as WIN32_ERROR;
    if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
        return Err(Error::Os(format!(
            "не удалось удалить ключ {PROG_ID}: код ошибки Windows {status}"
        )));
    }

    notify_shell();
    Ok(())
}

pub fn state() -> Association {
    let Some(stored) = get_string(&format!("{CLASSES}\\{PROG_ID}\\shell\\open\\command"), None)
    else {
        return Association::Missing;
    };

    match exe_path() {
        // Сравниваем без учёта регистра: пути в Windows регистр не различают,
        // а записан ключ мог быть с другим регистром буквы диска.
        Ok(exe) if stored.eq_ignore_ascii_case(&open_command(&exe)) => Association::Registered,
        _ => Association::Stale,
    }
}

/// Сообщает оболочке, что ассоциации изменились.
///
/// Без этого новый пункт в «Открыть с помощью» появится только после
/// перезапуска проводника или повторного входа в систему.
fn notify_shell() {
    // SAFETY: оба указателя нулевые — это допустимо и означает
    // «изменилось вообще всё, перечитай сама».
    unsafe {
        // Константа объявлена как u32, а параметр функции — i32.
        // Расхождение в самом Windows-API, не у нас.
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
}
