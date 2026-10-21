; rapcoding installer (per-user, no admin required).
; Build from the repo root: ISCC.exe installer\rapcoding.iss
;   [PreCompile] runs installer\build.ps1, which builds both the console and
;   GUI binaries into target\release\ (needs Rust + cargo in PATH).
; Output: dist\RapcodingSetup-0.1.0.exe

#define MyAppName "rapcoding"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "lightclove"
#define MyAppExeName "rapcoding.exe"

[Setup]
AppId={{A6B7A533-A171-46CC-BA42-96B14BB7B7E6}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\rapcoding
DefaultGroupName={#MyAppName}
UninstallDisplayIcon={app}\{#MyAppExeName}
SetupIconFile=..\assets\rapcoding.ico
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=RapcodingSetup-{#MyAppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

; Preset setup types (the first one is the default).
[Types]
Name: "gui"; Description: "С интерфейсом + лёгкие модели (рекомендуется)"
Name: "console"; Description: "Консольная + лёгкие модели"
Name: "full"; Description: "С интерфейсом + тяжёлые модели"
Name: "custom"; Description: "Выборочная установка"; Flags: iscustom

[Components]
; App version (mutually exclusive).
Name: "app_gui"; Description: "Графический интерфейс (~4.5 МБ, включает консольный режим)"; Types: gui full custom; Flags: exclusive
Name: "app_console"; Description: "Только консоль (~750 КБ, без окна)"; Types: console custom; Flags: exclusive
; Models (light or heavy, per language).
Name: "ru_light"; Description: "Русская — лёгкая (~44 МБ)"; Types: gui console custom; Check: not WizardIsComponentSelected('ru_heavy')
Name: "ru_heavy"; Description: "Русская — тяжёлая (~1.8 ГБ, точнее)"; Types: full custom; Check: not WizardIsComponentSelected('ru_light')
Name: "en_light"; Description: "Английская — лёгкая (~39 МБ)"; Types: gui console custom; Check: not WizardIsComponentSelected('en_heavy')
Name: "en_heavy"; Description: "Английская — тяжёлая (~1.8 ГБ, точнее)"; Types: full custom; Check: not WizardIsComponentSelected('en_light')

[Tasks]
Name: "startmenu"; Description: "Создать ярлык в меню «Пуск»"; GroupDescription: "Ярлыки:"
; Windows 10/11 запрещает программное закрепление — попытка best-effort, не блокирует установку.
Name: "taskbar"; Description: "Закрепить в панели задач (может не сработать на Windows 10/11)"; GroupDescription: "Ярлыки:"; Flags: unchecked

[Files]
Source: "..\target\release\rapcoding-console.exe"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Components: app_console; Flags: ignoreversion
Source: "..\target\release\rapcoding-gui.exe"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Components: app_gui; Flags: ignoreversion
Source: "..\target\release\libvosk.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\libgcc_s_seh-1.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\libstdc++-6.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\libwinpthread-1.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "pin-taskbar.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "download-models.ps1"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: startmenu

[Run]
Filename: "powershell.exe"; \
    Parameters: "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File ""{app}\download-models.ps1"" -Models ""{code:GetSelectedModels}"""; \
    StatusMsg: "Скачивание моделей распознавания… (может занять время)"; \
    Flags: runhidden; Check: AnyModelSelected
Filename: "powershell.exe"; \
    Parameters: "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File ""{app}\pin-taskbar.ps1"" -Target ""{app}\{#MyAppExeName}"""; \
    Tasks: taskbar; Flags: runhidden

[PreCompile]
Filename: "powershell.exe"; \
    Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""installer\build.ps1"""; \
    StatusMsg: "Сборка rapcoding (консоль + GUI)…"; Flags: abortonerror

[Code]
function AnyModelSelected(Param: string): Boolean;
begin
  Result := IsComponentSelected('ru_light') or IsComponentSelected('ru_heavy')
         or IsComponentSelected('en_light') or IsComponentSelected('en_heavy');
end;

function GetSelectedModels(Param: string): string;
var
  list: string;
begin
  list := '';
  if IsComponentSelected('ru_light') then list := list + 'ru-light,';
  if IsComponentSelected('ru_heavy') then list := list + 'ru-heavy,';
  if IsComponentSelected('en_light') then list := list + 'en-light,';
  if IsComponentSelected('en_heavy') then list := list + 'en-heavy,';
  if list <> '' then Delete(list, Length(list), 1);
  Result := list;
end;
