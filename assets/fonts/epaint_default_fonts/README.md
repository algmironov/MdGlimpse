# Лицензии шрифтов, встроенных в egui

Здесь нет ни одного `.ttf`. Сами файлы шрифтов лежат внутри крейта
`epaint_default_fonts 0.36.1` и попадают в `mdglimpse.exe` через
`include_bytes!` при сборке — забрать их сюда значило бы завести вторую
копию тех же байтов, которая однажды разойдётся с первой.

Тексты лицензий, наоборот, скопированы: в собранном бинарнике их нет
(`strip = true` в профиле release), а раздавать шрифты без условий нельзя.

Почему эти шрифты вообще в списке. `install_fonts` в `src/app.rs`
добавляет Inter и JetBrains Mono через `FontInsert` с
`FontPriority::Highest` — это вставка в начало списка, а не замена.
Встроенные в egui шрифты остаются в бинарнике и остаются в раздаче,
поэтому их шесть, а не два.

## Что откуда

| Файл | Шрифт | Лицензия |
|---|---|---|
| `UFL.txt` | Ubuntu Light 0.83 | Ubuntu Font Licence 1.0 |
| `OFL.txt` | Noto Emoji 1.05 | SIL OFL 1.1 |
| `Hack-Regular.txt` | Hack 3.003 | MIT + Bitstream Vera |
| `emoji-icon-font-mit-license.txt` | emoji-icon-font 1.1 | MIT |

Имена оставлены такими же, как в крейте, чтобы копии сверялись по хешу,
а не глазами. Понятные имена (`Ubuntu-UFL.txt` и прочие) появляются
только при установке — их подставляет `DestName` в `installer/mdglimpse.iss`.

## Копирайтов в этих файлах нет, и это важно

Ни в `UFL.txt`, ни в `OFL.txt` нет строки правообладателя — только тело
лицензии. Проверено: слов «Canonical» и «Google» в них не встречается
ни разу, а «Copyright» попадается лишь в обороте «Copyright Holder(s)»
внутри самих условий.

Настоящие уведомления живут в таблице `name` соответствующих `.ttf`,
а её срезает `strip = true`. Между тем условие 1 UFL требует, чтобы
уведомление распространялось вместе со шрифтом и было «easily viewed
by the user»; пункт 2 OFL требует того же. Поэтому строки выписаны
вручную в `THIRD-PARTY.md` — без них условия не выполнены, сколько
файлов лицензий ни положи.

Извлечены прямо из шрифтов:

```
Ubuntu-Light.ttf      Copyright 2011 Canonical Ltd.  Licensed under the Ubuntu Font Licence 1.0
NotoEmoji-Regular.ttf Copyright 2013 Google Inc. All Rights Reserved.
Hack-Regular.ttf      Copyright (c) 2018 Source Foundry Authors / Copyright (c) 2003 by Bitstream, Inc. All Rights Reserved.
```

## Сверка с крейтом

```sh
cd ~/.cargo/registry/src/index.crates.io-*/epaint_default_fonts-0.36.1/fonts
sha256sum UFL.txt OFL.txt Hack-Regular.txt emoji-icon-font-mit-license.txt
```

Ожидаемые значения для 0.36.1:

```
2f0015108d68627bd788d313f529c21ff4da2c2c42a5e1f3883acc83480f9002  UFL.txt
6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2  OFL.txt
47c0cccbeec7e8614548cc485588b28149e7874188df5f41b36efebcee285c87  Hack-Regular.txt
b9d2c1d909aa149996fd4c91dcb92b2362a04431640c1d200959da94caf8cde1  emoji-icon-font-mit-license.txt
```

При подъёме версии eframe сверьте суммы заново и проверьте, не изменился
ли набор шрифтов в `epaint_default_fonts/src/lib.rs`: список
`include_bytes!` там короткий, но молчаливый — новый шрифт приедет
без единого предупреждения.
