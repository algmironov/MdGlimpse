# Что вшито в MdGlimpse, кроме его собственного кода

MdGlimpse — один статически слинкованный исполняемый файл. Внутри него,
кроме нашего кода, лежат чужие шрифты, чужие грамматики подсветки синтаксиса
и код трёх с лишним сотен крейтов. Профиль release собирается со `strip = true`,
поэтому строки с копирайтами, которые обычно живут внутри `.ttf`, из бинарника
вырезаны — все уведомления вынесены наружу, в файлы рядом с программой.

Сам MdGlimpse распространяется на условиях **MIT OR Apache-2.0**
(© 2026 algmironov): тексты в `licenses/LICENSE-MIT.txt` и
`licenses/LICENSE-APACHE.txt`, выбор условий — за вами.

Этот файл ведётся вручную и описывает то, чего нет в `Cargo.toml`: шрифты
и данные `syntect`. Автоматический список крейтов —
в [THIRD-PARTY-CRATES.md](THIRD-PARTY-CRATES.md).

---

## Шрифты

Их шесть, а не два. `install_fonts` в `src/app.rs` добавляет Inter и
JetBrains Mono через `FontInsert` с `FontPriority::Highest` — это вставка
в начало списков шрифтов, а не замена. Четыре шрифта, встроенные в egui через
крейт `epaint_default_fonts`, остаются в бинарнике, работают (эмодзи и редкие
символы рисуются именно ими) и распространяются вместе с программой.

| Шрифт | Версия | Источник | Лицензия | Текст |
|---|---|---|---|---|
| Inter | 4.001 | `assets/fonts/` | SIL OFL 1.1 | `licenses/fonts/Inter-OFL.txt` |
| JetBrains Mono NL | 2.304 | `assets/fonts/` | SIL OFL 1.1 | `licenses/fonts/JetBrainsMono-OFL.txt` |
| Ubuntu Light | 0.83 | крейт `epaint_default_fonts` | Ubuntu Font Licence 1.0 | `licenses/fonts/Ubuntu-UFL.txt` |
| Hack | 3.003 | крейт `epaint_default_fonts` | MIT + Bitstream Vera | `licenses/fonts/Hack-LICENSE.txt` |
| Noto Emoji | 1.05 | крейт `epaint_default_fonts` | SIL OFL 1.1 | `licenses/fonts/NotoEmoji-OFL.txt` |
| emoji-icon-font | 1.1 | крейт `epaint_default_fonts` | MIT | `licenses/fonts/emoji-icon-font-MIT.txt` |

Ни у Inter, ни у JetBrains Mono нет зарезервированных имён шрифта (Reserved
Font Names), так что ограничения OFL на переименование при изменении нас
не касаются. Единственные RFN во всём наборе — «Bitstream» и «Vera» в составе
Hack; мы Hack не изменяем.

### Inter 4.001

```
Copyright (c) 2016 The Inter Project Authors (https://github.com/rsms/inter)
```

Лицензия: SIL Open Font License, Version 1.1.
Полный текст: `licenses/fonts/Inter-OFL.txt`.

Сборка из релиза [v4.1](https://github.com/rsms/inter/releases/tag/v4.1);
внутренняя версия шрифта — 4.001, коммит `9221beed3`.

### JetBrains Mono NL 2.304

```
Copyright 2020 The JetBrains Mono Project Authors
(https://github.com/JetBrains/JetBrainsMono)
```

Лицензия: SIL Open Font License, Version 1.1.
Полный текст: `licenses/fonts/JetBrainsMono-OFL.txt`.

Используется вариант **NL** — «no ligatures», без лигатур. Внутри самого
`.ttf` копирайт заявлен от «The JetBrains Mono NL Project Authors» со ссылкой
на `github.com/JetBrains/JetBrainsMonoNL`, но такого репозитория не
существует: вариант NL поставляется внутри основного релиза
[v2.304](https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304).
Выше приведена каноническая строка из официального `OFL.txt` проекта.

### Ubuntu Light 0.83

```
Copyright 2011 Canonical Ltd.  Licensed under the Ubuntu Font Licence 1.0
```

Лицензия: **Ubuntu Font Licence 1.0** — не OFL, отдельный документ со своими
условиями. Полный текст: `licenses/fonts/Ubuntu-UFL.txt`.

Строка копирайта выписана здесь вручную, и это обязательное действие,
а не оформление. Условие 1 UFL требует, чтобы уведомление распространялось
вместе со шрифтом и было «easily viewed by the user». В файле `UFL.txt`,
который поставляется в крейте, копирайта Canonical нет — слово «Canonical»
там не встречается ни разу; уведомление было только в таблице `name` файла
`Ubuntu-Light.ttf`, а её вычищает `strip = true`. Без этой строки условие
не выполнено, сколько файлов лицензий ни положи рядом.

«Ubuntu» и «Canonical» — зарегистрированные товарные знаки Canonical Ltd.
UFL, в отличие от OFL, содержит прямую оговорку: лицензия не даёт никаких
прав по законодательству о товарных знаках. Начертание выполнено
Dalton Maag Ltd.

### Hack 3.003

```
Copyright (c) 2018 Source Foundry Authors
Copyright (c) 2003 by Bitstream, Inc. All Rights Reserved.
```

Двойная лицензия: работа проекта Hack — под MIT, унаследованное из Bitstream
Vera Sans Mono — под Bitstream Vera License с зарезервированными именами
шрифта «Bitstream» и «Vera». Промежуточный проект DejaVu передан
в общественное достояние. Оба текста целиком:
`licenses/fonts/Hack-LICENSE.txt`.

### Noto Emoji 1.05

```
Copyright 2013 Google Inc. All Rights Reserved.
```

Лицензия: SIL Open Font License, Version 1.1.
Полный текст: `licenses/fonts/NotoEmoji-OFL.txt`.

Копирайт, как и у Ubuntu, выписан вручную по той же причине: `OFL.txt`
из крейта `epaint_default_fonts` содержит только текст лицензии, без строки
правообладателя, а из `.ttf` её срезает strip. Пункт 2 OFL требует, чтобы
уведомление о копирайте распространялось с каждой копией шрифта.

«Noto» — товарный знак Google Inc.

### emoji-icon-font 1.1

```
MIT License
Copyright (c) 2014 John Slegers
```

Полный текст: `licenses/fonts/emoji-icon-font-MIT.txt`.
Исходный проект: <https://github.com/jslegers/emoji-icon-font>

---

## Данные подсветки синтаксиса

Крейт `syntect 5.3.0` включает в бинарник два бинарных дампа:
`default_newlines.packdump` (около 368 КБ, 75 грамматик Sublime Text)
и `default.themedump` (7 цветовых тем). Ни один инструмент, читающий
`Cargo.toml`, этого не увидит: с точки зрения cargo это просто байты внутри
крейта, лицензированного под MIT. Условия у самих данных свои.

### Грамматики

Взяты из репозитория [sublimehq/Packages](https://github.com/sublimehq/Packages).
Лицензия оттуда, целиком:

```
If not otherwise specified (see below), files in this repository fall under the following license:

    Permission to copy, use, modify, sell and distribute this
    software is granted. This software is provided "as is" without
    express or implied warranty, and with no claim as to its
    suitability for any purpose.

An exception is made for files in readable text which contain their own license information, or files where an accompanying file exists (in the same directory) with a “-license” suffix added to the base-name name of the original file, and an extension of txt, html, or similar. For example “tidy” is accompanied by “tidy-license.txt”.
```

Оговорка про «файлы со своей лицензией» задействована как минимум один раз:
пакет `Rust` в том же репозитории сопровождается собственным
`Rust/LICENSE.txt` с текстом MIT. Он приведён ниже — текст MIT в этом файле
общий и для грамматики Rust, и для тем.

### Темы

Все семь — под MIT:

| Тема | Происхождение | Копирайт |
|---|---|---|
| `base16-ocean.dark` | [kkga/spacegray](https://github.com/kkga/spacegray) | Gadzhi Kharkharov |
| `base16-ocean.light` | kkga/spacegray | Gadzhi Kharkharov |
| `base16-eighties.dark` | kkga/spacegray | Gadzhi Kharkharov |
| `base16-mocha.dark` | kkga/spacegray | Gadzhi Kharkharov |
| `InspiredGitHub` | [sethlopezme/InspiredGitHub.tmtheme](https://github.com/sethlopezme/InspiredGitHub.tmtheme) | Copyright (c) 2015 Seth Lopez |
| `Solarized (dark)` | [braver/Solarized](https://github.com/braver/Solarized) | Ethan Schoonover |
| `Solarized (light)` | braver/Solarized | Ethan Schoonover |

MdGlimpse по умолчанию использует `base16-ocean.light` и `base16-ocean.dark`.

Текст лицензии MIT, общий для тем и для грамматики Rust:

```
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

---

## Крейты

316 крейтов, их версии и полные тексты лицензий —
в [THIRD-PARTY-CRATES.md](THIRD-PARTY-CRATES.md). Это объединение двух
платформ: в конкретный бинарник попадает около 210 из них, остальные
относятся к другой ОС. Файл создаётся автоматически, править его руками
бессмысленно; как перегенерировать — написано в README, раздел
«Сборка из исходников».

Копилефтных лицензий в дереве нет. Единственное упоминание GPL —
`self_cell` под `Apache-2.0 OR GPL-2.0-only`; это выбор одного из двух,
и выбран Apache-2.0.
