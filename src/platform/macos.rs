//! macOS: честная заглушка.
//!
//! Там ассоциации живут не в конфигах, а в `Info.plist` внутри
//! `.app`-бандла и регистрируются Launch Services при установке.
//! Отдельной кнопкой «зарегистрировать» это не делается, поэтому
//! правильный ответ здесь — «не поддерживается», а не тихое ничего.

use super::{Association, Error, Result};

pub fn register() -> Result<()> {
    Err(Error::Unsupported)
}

pub fn unregister() -> Result<()> {
    Err(Error::Unsupported)
}

pub fn state() -> Association {
    Association::Unsupported
}
