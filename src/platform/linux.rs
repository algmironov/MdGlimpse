//! Linux: `.desktop`-файл в каталоге пользователя плюс `xdg-mime`.
//!
//! Прав администратора не нужно и здесь: всё кладётся в `$XDG_DATA_HOME`
//! (по умолчанию `~/.local/share`), то есть в домашний каталог.
//!
//! В отличие от Windows, назначить обработчик по умолчанию тут можно
//! честно — этим и занимается `xdg-mime default`. Если утилиты в системе
//! нет, `.desktop` всё равно записан, и файл будет виден в списке
//! «Открыть с помощью» большинства окружений.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use super::{Association, Error, Result, EXTENSIONS};

/// Имя файла должно быть своим: рядом может стоять проект-близнец,
/// и `.desktop` с общим именем один перезаписал бы у другого.
const DESKTOP_FILE: &str = "mdglimpse.desktop";

/// MIME-тип у markdown стандартный, свой изобретать не надо.
const MIME: &str = "text/markdown";

fn data_home() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| Error::Os("не задана переменная окружения HOME".into()))?;
    Ok(PathBuf::from(home).join(".local").join("share"))
}

fn desktop_path() -> Result<PathBuf> {
    Ok(data_home()?.join("applications").join(DESKTOP_FILE))
}

fn exe_path() -> Result<String> {
    let path = std::env::current_exe()
        .map_err(|error| Error::Os(format!("не удалось узнать путь к программе: {error}")))?;
    Ok(path.display().to_string())
}

pub fn register() -> Result<()> {
    let path = desktop_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            Error::Os(format!("не удалось создать {}: {error}", parent.display()))
        })?;
    }

    // %f, а не %F: открывать умеем один файл за раз, и врать об этом
    // окружению не стоит — иначе оно передаст сразу несколько.
    let contents = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=MdGlimpse\n\
         Comment=Просмотрщик Markdown\n\
         Exec={exe} %f\n\
         Icon=mdglimpse\n\
         Terminal=false\n\
         Categories=Utility;TextEditor;\n\
         MimeType={MIME};\n",
        exe = exe_path()?,
    );

    let mut file = fs::File::create(&path)
        .map_err(|error| Error::Os(format!("не удалось записать {}: {error}", path.display())))?;
    file.write_all(contents.as_bytes())
        .map_err(|error| Error::Os(format!("не удалось записать {}: {error}", path.display())))?;

    // Обе утилиты необязательны: без них файл просто подхватится позже,
    // при следующем обновлении кеша окружением. Ошибку не поднимаем.
    let _ = Command::new("update-desktop-database")
        .arg(path.parent().unwrap_or(&path))
        .status();
    let _ = Command::new("xdg-mime")
        .args(["default", DESKTOP_FILE, MIME])
        .status();

    Ok(())
}

pub fn unregister() -> Result<()> {
    let path = desktop_path()?;
    match fs::remove_file(&path) {
        Ok(()) => {}
        // Нечего удалять — это успех, а не ошибка: результат тот же.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Error::Os(format!(
                "не удалось удалить {}: {error}",
                path.display()
            )))
        }
    }

    let _ = Command::new("update-desktop-database")
        .arg(path.parent().unwrap_or(&path))
        .status();
    Ok(())
}

pub fn state() -> Association {
    let Ok(path) = desktop_path() else {
        return Association::Missing;
    };
    let Ok(contents) = fs::read_to_string(&path) else {
        return Association::Missing;
    };

    let Ok(exe) = exe_path() else {
        return Association::Missing;
    };
    // Ищем строку Exec=: если она ведёт на другой файл, значит .desktop
    // остался от прежней копии программы.
    let matches_exe = contents
        .lines()
        .find_map(|line| line.strip_prefix("Exec="))
        .is_some_and(|command| command.starts_with(&exe));

    if matches_exe {
        Association::Registered
    } else {
        Association::Stale
    }
}

/// Расширения на Linux не используются: там ассоциация по MIME-типу.
/// Ссылаемся на константу, чтобы компилятор не ругался на неиспользованное.
const _: &[&str] = EXTENSIONS;
