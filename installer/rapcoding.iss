; rapcoding installer (per-user, no admin required).
; Build from the repo root (needs Rust + cargo in PATH):
;   1. powershell -NoProfile -ExecutionPolicy Bypass -File installer\build.ps1
;   2. ISCC.exe installer\rapcoding.iss
; Output: dist\RapcodingSetup-0.1.0.exe
;
; NOTE: the installer never downloads models. It only records the user's
; choice in %APPDATA%\rapcoding\pending-models.txt; the app's GUI downloads
; them on first launch (real progress bar + bounded 30 s read timeout).
; Reason: Inno's built-in downloader has no configurable timeout, and the
; Vosk host stalls after ~16 KB on networks where it is throttled, which made
; the installer hang forever.

#define MyAppName "rapcoding"
#define MyAppVersion "0.2.0"
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
; Always offer the language choice instead of silently reusing a previous one.
ShowLanguageDialog=yes
UsePreviousLanguage=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Types]
Name: "gui"; Description: "С интерфейсом + лёгкие модели (рекомендуется)"
Name: "console"; Description: "Консольная + лёгкие модели"
Name: "full"; Description: "С интерфейсом + тяжёлые модели"
Name: "custom"; Description: "Выборочная установка"; Flags: iscustom

[Components]
; Version: one checkbox — checked = GUI, unchecked = console-only.
Name: "gui"; Description: "Графический интерфейс (снимите отметку — только консоль)"; Types: gui full custom
; Models: only recorded here; downloaded by the app on first launch.
Name: "ru_light"; Description: "Русская модель — лёгкая (~44 МБ)"; Types: gui console custom
Name: "ru_heavy"; Description: "Русская модель — тяжёлая (~1.8 ГБ, точнее)"; Types: full custom
Name: "en_light"; Description: "Английская модель — лёгкая (~39 МБ)"; Types: gui console custom
Name: "en_heavy"; Description: "Английская модель — тяжёлая (~1.8 ГБ, точнее)"; Types: full custom

[Tasks]
Name: "startmenu"; Description: "Создать ярлык в меню «Пуск»"; GroupDescription: "Ярлыки:"
; Windows 10/11 запрещает программное закрепление — попытка best-effort, не блокирует установку.
Name: "taskbar"; Description: "Закрепить в панели задач (может не сработать на Windows 10/11)"; GroupDescription: "Ярлыки:"; Flags: unchecked

[Files]
Source: "..\target\release\rapcoding-gui.exe"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Check: IsComponentSelected('gui'); Flags: ignoreversion
Source: "..\target\release\rapcoding-console.exe"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Check: not IsComponentSelected('gui'); Flags: ignoreversion
Source: "..\target\release\libvosk.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\libgcc_s_seh-1.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\libstdc++-6.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\libwinpthread-1.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "pin-taskbar.ps1"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: startmenu

[Run]
Filename: "powershell.exe"; \
    Parameters: "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File ""{app}\pin-taskbar.ps1"" -Target ""{app}\{#MyAppExeName}"""; \
    Tasks: taskbar; Flags: runhidden

[Code]
var
  DeleteUserData: TNewCheckBox;

// Record the picked models for the app to download on first launch.
procedure WritePendingModels;
var
  names: String;
begin
  names := '';
  if IsComponentSelected('ru_light') then
    names := names + 'vosk-model-small-ru-0.22' + #13#10;
  if IsComponentSelected('ru_heavy') then
    names := names + 'vosk-model-ru-0.42' + #13#10;
  if IsComponentSelected('en_light') then
    names := names + 'vosk-model-small-en-us-0.15' + #13#10;
  if IsComponentSelected('en_heavy') then
    names := names + 'vosk-model-en-us-0.22' + #13#10;
  if names = '' then
    exit;
  ForceDirectories(ExpandConstant('{userappdata}\rapcoding'));
  SaveStringToFile(ExpandConstant('{userappdata}\rapcoding\pending-models.txt'),
    names, False);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    WritePendingModels;
end;

procedure InitializeUninstallProgressForm();
begin
  DeleteUserData := TNewCheckBox.Create(UninstallProgressForm);
  DeleteUserData.Parent := UninstallProgressForm;
  DeleteUserData.Caption := 'Удалить пользовательские данные и все скачанные модели';
  DeleteUserData.Left := ScaleX(16);
  DeleteUserData.Top := UninstallProgressForm.CancelButton.Top - ScaleY(44);
  DeleteUserData.Width := UninstallProgressForm.ClientWidth - ScaleX(32);
  DeleteUserData.Height := ScaleY(20);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then begin
    if DeleteUserData.Checked then begin
      if DirExists(ExpandConstant('{userappdata}\rapcoding')) then
        DelTree(ExpandConstant('{userappdata}\rapcoding'), True, True, True);
    end;
  end;
end;
