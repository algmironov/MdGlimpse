; Установщик MdGlimpse для Inno Setup 6.
;
; Собирать так (из корня проекта):
;   cargo build --release
;   iscc installer\mdglimpse.iss
; Готовый файл появится в installer\output\.
;
; Установка идёт БЕЗ прав администратора: в {localappdata}, ключи только
; в HKCU. Запроса UAC пользователь не увидит.
;
; Про соседство с проектом-близнецом на .NET, который называется MdView:
; всё, чем этот установщик помечает систему, обязано быть своим —
; AppId, каталог установки, ProgId, имя в «Установка и удаление программ»
; и каталог настроек в [UninstallDelete]. Совпадение любого из них
; означает, что один проект затрёт или снесёт другой.

#define SourceExe "..\target\release\mdglimpse.exe"

#if !FileExists(AddBackslash(SourcePath) + SourceExe)
  #error Сначала соберите релиз: cargo build --release
#endif

; Версию не дублируем руками — вынимаем из метаданных .exe, которые
; build.rs берёт из CARGO_PKG_VERSION. Один источник правды на всё.
#define AppVersion GetVersionNumbersString(AddBackslash(SourcePath) + SourceExe)
#define ProgId "MdGlimpse.markdown"

[Setup]
; GUID сгенерирован заново, а не отредактирован из прежнего. Это не
; придирка: по AppId Inno решает, установка это или обновление. Совпади
; он с AppId близнеца — установщик одного счёл бы себя обновлением
; другого и снёс бы его файлы вместе с записью об удалении.
AppId={{78285252-ED40-4F0C-802E-BE9633C8CB6F}
AppName=MdGlimpse
AppVersion={#AppVersion}
AppVerName=MdGlimpse {#AppVersion}
VersionInfoVersion={#AppVersion}
AppPublisher=algmironov
DefaultDirName={localappdata}\MdGlimpse
DefaultGroupName=MdGlimpse
DisableProgramGroupPage=yes
; lowest — ключевая строка всего файла: без неё Inno попросит админа.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=output
OutputBaseFilename=mdglimpse-{#AppVersion}-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
SetupIconFile=..\assets\mdglimpse.ico
UninstallDisplayIcon={app}\mdglimpse.exe
; Программа 64-битная, и ставить её на 32-битную систему незачем.
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
; Отдельная галка, а не молчаливое действие: запись в реестр — это
; вмешательство в систему, и человек должен на него согласиться.
Name: "associate"; Description: "Добавить MdGlimpse в «Открыть с помощью» для .md и .markdown"; GroupDescription: "Ассоциация файлов:"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme

; Уведомления о том, что вшито в .exe. Оба — Markdown, и оба открываются
; самой программой: отдельного ярлыка им не нужно.
; THIRD-PARTY.md ведётся руками, THIRD-PARTY-CRATES.md генерирует
; cargo-about. Если второго файла нет, iscc остановится вот на этой
; строке — единственное напоминание перегенерировать список перед
; релизом, которое невозможно пролистать.
Source: "..\THIRD-PARTY.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\THIRD-PARTY-CRATES.md"; DestDir: "{app}"; Flags: ignoreversion

; Собственная лицензия проекта — двойная, поэтому файла два. DestName
; дописывает .txt: файл без расширения Windows открывать не умеет,
; а двойной щелчок — единственный способ прочитать его, который придёт
; в голову большинству.
Source: "..\LICENSE-MIT"; DestDir: "{app}\licenses"; DestName: "LICENSE-MIT.txt"; Flags: ignoreversion
Source: "..\LICENSE-APACHE"; DestDir: "{app}\licenses"; DestName: "LICENSE-APACHE.txt"; Flags: ignoreversion

; Шесть шрифтов, вшитых в .exe. Два наших и четыре из egui: install_fonts
; добавляет наши через FontInsert с приоритетом Highest, то есть поверх
; встроенных, а не вместо них, — раздаются все шесть, и OFL с UFL требуют
; сопровождать каждую копию текстом лицензии.
;
; Перечислены поимённо, а не маской "*.txt". Маска, под которую ничего
; не попало, — это молча пустой каталог лицензий в установленной
; программе; отсутствующий файл в явной строке останавливает компиляцию.
;
; DestName у четырёх последних: в общем каталоге имена OFL.txt и UFL.txt
; не говорят, к какому шрифту относятся. В репозитории они, наоборот,
; названы как в крейте — чтобы сверять копии по хешу, а не глазами.
Source: "..\assets\fonts\Inter-OFL.txt"; DestDir: "{app}\licenses\fonts"; Flags: ignoreversion
Source: "..\assets\fonts\JetBrainsMono-OFL.txt"; DestDir: "{app}\licenses\fonts"; Flags: ignoreversion
Source: "..\assets\fonts\epaint_default_fonts\UFL.txt"; DestDir: "{app}\licenses\fonts"; DestName: "Ubuntu-UFL.txt"; Flags: ignoreversion
Source: "..\assets\fonts\epaint_default_fonts\OFL.txt"; DestDir: "{app}\licenses\fonts"; DestName: "NotoEmoji-OFL.txt"; Flags: ignoreversion
Source: "..\assets\fonts\epaint_default_fonts\Hack-Regular.txt"; DestDir: "{app}\licenses\fonts"; DestName: "Hack-LICENSE.txt"; Flags: ignoreversion
Source: "..\assets\fonts\epaint_default_fonts\emoji-icon-font-mit-license.txt"; DestDir: "{app}\licenses\fonts"; DestName: "emoji-icon-font-MIT.txt"; Flags: ignoreversion

[Icons]
Name: "{group}\MdGlimpse"; Filename: "{app}\mdglimpse.exe"
Name: "{group}\Удалить MdGlimpse"; Filename: "{uninstallexe}"

[Registry]
; ---- создаётся только при выбранной галке ----
Root: HKCU; Subkey: "Software\Classes\{#ProgId}"; ValueType: string; ValueData: "Документ Markdown"; Flags: uninsdeletekey; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\{#ProgId}"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "MdGlimpse"; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\{#ProgId}\DefaultIcon"; ValueType: string; ValueData: "{app}\mdglimpse.exe,0"; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\{#ProgId}\shell\open\command"; ValueType: string; ValueData: """{app}\mdglimpse.exe"" ""%1"""; Tasks: associate
; Значение пустое: смысл несёт его имя. Удаляется именно значение,
; а не ключ — в нём перечислены и чужие программы.
Root: HKCU; Subkey: "Software\Classes\.md\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.markdown\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate

; ---- ничего не создаётся, но удаляется при деинсталляции ----
; Галку могли не поставить, а потом зарегистрироваться из меню самой
; программы. Тогда ключи есть, а установщик про них не знает. dontcreatekey
; ровно для этого случая: запись существует только ради строки удаления.
Root: HKCU; Subkey: "Software\Classes\{#ProgId}"; Flags: dontcreatekey uninsdeletekey; Tasks: not associate
Root: HKCU; Subkey: "Software\Classes\.md\OpenWithProgids"; ValueType: none; ValueName: "{#ProgId}"; Flags: dontcreatekey deletevalue uninsdeletevalue; Tasks: not associate
Root: HKCU; Subkey: "Software\Classes\.markdown\OpenWithProgids"; ValueType: none; ValueName: "{#ProgId}"; Flags: dontcreatekey deletevalue uninsdeletevalue; Tasks: not associate

[Run]
Filename: "{app}\mdglimpse.exe"; Description: "Запустить MdGlimpse"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Настройки, которые пишет eframe: %APPDATA%\mdglimpse\data\app.ron.
; Имя каталога — первый аргумент run_native, и оно обязано совпадать
; с ним буква в букву. Прежняя версия сносила здесь {userappdata}\MdView
; и вместе со своими настройками уносила настройки проекта-близнеца,
; потому что каталог у них был общий.
Type: filesandordirs; Name: "{userappdata}\mdglimpse"
