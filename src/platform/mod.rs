//! Связь с операционной системой: регистрация просмотрщика для .md.
//!
//! Как и `document.rs`, про egui ничего не знает — наружу торчат три
//! функции и два перечисления, а вся возня с реестром, `.desktop`-файлами
//! и прочей платформенной кухней спрятана в подмодулях.
//!
//! # Главное ограничение, которое надо понимать
//!
//! Сделать себя обработчиком `.md` по умолчанию программа под Windows 10
//! и 11 **не может** — и это не наша недоработка. Начиная с Windows 8
//! запись `HKCU\Software\Classes\.md\UserChoice` защищена хешем, который
//! считается из пути, расширения и идентификатора пользователя по
//! недокументированному алгоритму. Подделать его технически возможно,
//! но это ровно то поведение, ради борьбы с которым защиту и вводили.
//! Поэтому мы делаем честную половину: регистрируем ProgID и добавляем
//! себя в «Открыть с помощью», а выбор по умолчанию оставляем человеку.

use std::fmt;

#[cfg_attr(not(windows), allow(unused_imports))]
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as sys;

#[cfg(all(unix, not(target_os = "macos")))]
mod linux;
#[cfg(all(unix, not(target_os = "macos")))]
use linux as sys;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as sys;

/// Пропускает чужие подмодули через компилятор на любой платформе.
///
/// CLAUDE.md требует, чтобы сборка проходила под все три системы, но
/// установлена здесь одна цель — `x86_64-pc-windows-msvc`, и собрать
/// под Linux с этой машины нельзя: eframe на этапе сборки хочет заголовки
/// X11. Зато сами эти модули системных API не трогают — только `std::fs`,
/// `std::process` и `std::env`, — поэтому проверить их синтаксис и типы
/// можно и отсюда. `cargo test` сделает это и поймает опечатку до того,
/// как она доедет до человека с Linux.
///
/// Это НЕ замена настоящей сборке: поведение так не проверяется никак.
/// Если однажды здесь понадобится что-то из `std::os::unix`, трюк честно
/// перестанет компилироваться — тогда его нужно убрать, а не чинить.
#[cfg(test)]
mod compiles_on_every_platform {
    // Подмодули пишут `use super::{..}`, и внутри обёртки `super` — это
    // она сама. Пробрасываем имена дальше, иначе они не найдутся.
    #[allow(unused_imports)]
    pub use super::{Association, Error, Result, EXTENSIONS};

    #[cfg(not(all(unix, not(target_os = "macos"))))]
    #[path = "../linux.rs"]
    #[allow(dead_code)]
    mod linux;

    #[cfg(not(target_os = "macos"))]
    #[path = "../macos.rs"]
    #[allow(dead_code)]
    mod macos;
}

/// Расширения, которые берём на себя.
///
/// Только два самых распространённых. `.mdown`, `.mkd`, `.mdtext` и прочая
/// экзотика встречается редко, а каждое лишнее расширение — это ещё один
/// ключ, который обязана вычистить отмена регистрации, и ещё одна строка
/// в «Открыть с помощью». Добавить потом — одна строка здесь.
pub const EXTENSIONS: &[&str] = &["md", "markdown"];

/// Что сейчас записано в системе.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Association {
    /// Записи есть и ведут на этот же исполняемый файл.
    Registered,
    /// Записи есть, но ведут на другой путь: программу перенесли
    /// или установили второй копией. Регистрацию стоит повторить.
    Stale,
    /// Записей нет.
    Missing,
    /// Платформа не поддерживается — состояние неизвестно и неважно.
    ///
    /// Возвращается только macOS-подмодулем; под Windows и Linux
    /// компилятор считает вариант неиспользуемым и он прав. Разводить
    /// перечисление по платформам ради этого не стоит: тогда и меню,
    /// которое его показывает, обросло бы `cfg`.
    #[allow(dead_code)]
    Unsupported,
}

impl Association {
    /// Короткая строка для меню и окна «О программе».
    pub fn label(self) -> &'static str {
        match self {
            Association::Registered => "зарегистрирован",
            Association::Stale => "зарегистрирован на другой путь",
            Association::Missing => "не зарегистрирован",
            Association::Unsupported => "не поддерживается на этой системе",
        }
    }
}

#[derive(Debug)]
pub enum Error {
    /// На этой платформе такого механизма нет.
    ///
    /// Собирается только в macOS-подмодуле, поэтому при сборке под Windows
    /// и Linux компилятор справедливо считает вариант неиспользуемым.
    /// Убирать его под cfg — значит развести формы ошибки по платформам
    /// и заставить вызывающий код тоже обрасти cfg; проще разрешить.
    #[allow(dead_code)]
    Unsupported,
    /// Система отказала. Внутри — уже пригодный для показа текст.
    Os(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported => write!(f, "на этой системе ассоциация файлов не поддерживается"),
            Error::Os(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Регистрирует просмотрщик как обработчик `EXTENSIONS`.
///
/// Вызывать только по явному действию человека: молча писать в реестр
/// при запуске — дурной тон, за который приложения справедливо ругают.
pub fn register() -> Result<()> {
    sys::register()
}

/// Убирает ровно то, что создал [`register`], и ничего сверх того.
pub fn unregister() -> Result<()> {
    sys::unregister()
}

/// Что сейчас в системе. Ошибку чтения намеренно превращаем в `Missing`:
/// состояние показывается в меню каждый кадр, и всплывающая ошибка там
/// была бы навязчивее пользы.
pub fn state() -> Association {
    sys::state()
}

/// Проверка регистрации на живом реестре.
///
/// Помечен `#[ignore]` намеренно: тест **пишет в HKCU того, кто его
/// запускает**, а обычный `cargo test` не должен трогать систему.
/// Запускать осознанно:
///
/// ```text
/// cargo test -- --ignored --nocapture
/// ```
///
/// Юнит-тестом работу с реестром не закрыть — подменить `HKEY_CURRENT_USER`
/// нечем, — поэтому проверка именно такая: зарегистрировались, посмотрели,
/// что записалось, сняли регистрацию, посмотрели, что не осталось.
///
/// Тест ровно один, и это не лень. Реестр — общий ресурс, а `cargo test`
/// по умолчанию гоняет тесты параллельно: два теста, регистрирующих
/// и снимающих регистрацию одновременно, ломают друг друга. Проверено
/// на себе — при двух тестах второй падал на пустом месте.
#[cfg(all(test, windows))]
mod registry_tests {
    use super::*;
    use std::process::Command;

    // Пути и имена здесь выписаны заново, а не взяты из констант модуля.
    // Это нарочно: тест сверяет реестр с тем, что обещано в документации,
    // и переименование константы в коде обязано его уронить, а не тихо
    // проехать вместе с ним.
    const KEY: &str = r"HKCU\Software\Classes\MdGlimpse.markdown";
    const PROG_ID_NAME: &str = "MdGlimpse.markdown";
    const QUOTED_ARGUMENT: &str = "\"%1\"";

    fn progids_key(extension: &str) -> String {
        format!(r"HKCU\Software\Classes\.{extension}\OpenWithProgids")
    }

    /// Спрашивает у `reg` содержимое ключа. Читаем чужими глазами,
    /// а не своей же `get_string`: иначе ошибка в чтении замаскировала бы
    /// ошибку в записи.
    fn query(args: &[&str]) -> (bool, String) {
        let output = Command::new("reg").arg("query").args(args).output();
        match output {
            // Вывод reg — в кодировке консоли, не UTF-8. Нам нужны только
            // латинские подстроки, для них lossy-разбора достаточно.
            Ok(output) => (
                output.status.success(),
                String::from_utf8_lossy(&output.stdout).into_owned(),
            ),
            Err(error) => panic!("не удалось запустить reg: {error}"),
        }
    }

    #[test]
    #[ignore = "пишет в реестр: запускать явно, через --ignored"]
    fn register_writes_the_documented_scheme_and_unregister_removes_it() {
        // Если ассоциация уже стояла до теста, восстановим её в конце:
        // забирать у человека настройку, о которой он не просил, нельзя.
        let was = state();
        let key = KEY;

        register().expect("регистрация должна пройти без прав администратора");
        assert_eq!(
            state(),
            Association::Registered,
            "после register состояние обязано стать Registered"
        );

        let (found, dump) = query(&[key, "/s"]);
        println!("{dump}");
        assert!(found, "ключ {key} должен существовать");
        assert!(dump.contains("DefaultIcon"), "нет подключа DefaultIcon");
        assert!(dump.contains("command"), "нет подключа shell/open/command");
        assert!(
            dump.contains(QUOTED_ARGUMENT),
            "аргумент %1 обязан быть в кавычках, иначе путь с пробелом разрежется"
        );
        assert!(dump.contains("FriendlyAppName"), "нет FriendlyAppName");

        for extension in EXTENSIONS {
            let progids = progids_key(extension);
            let (found, _) = query(&[&progids, "/v", PROG_ID_NAME]);
            assert!(found, "нет записи {PROG_ID_NAME} в {progids}");
        }

        unregister().expect("снятие регистрации");
        assert_eq!(
            state(),
            Association::Missing,
            "после unregister не должно остаться ничего"
        );

        let (still_there, _) = query(&[key]);
        assert!(
            !still_there,
            "после снятия регистрации ключ {key} обязан исчезнуть целиком"
        );

        for extension in EXTENSIONS {
            let progids = progids_key(extension);
            let (mine, _) = query(&[&progids, "/v", PROG_ID_NAME]);
            assert!(!mine, "своё значение осталось в {progids}");

            // А сам ключ исчезнуть НЕ должен: в нём чужие программы,
            // и снести его целиком было бы вредительством.
            let (key_alive, _) = query(&[&progids]);
            assert!(
                key_alive,
                "{progids} снесён целиком вместе с чужими записями"
            );
        }

        if was == Association::Registered {
            register().expect("восстановление прежнего состояния");
        }
    }
}
