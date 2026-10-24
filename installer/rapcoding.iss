; rapcoding installer (per-user, no admin required).
; Build from the repo root (needs Rust + cargo in PATH):
;   1. powershell -NoProfile -ExecutionPolicy Bypass -File installer\build.ps1
;   2. ISCC.exe installer\rapcoding.iss
; Output: dist\RapcodingSetup-0.3.0.exe
;
; NOTE: the installer never downloads models. It only records the user's
; choice in %APPDATA%\rapcoding\pending-models.txt; the app's GUI downloads
; them on first launch (real progress bar + bounded 30 s read timeout).
; Reason: Inno's built-in downloader has no configurable timeout, and the
; Vosk host stalls after ~16 KB on networks where it is throttled, which made
; the installer hang forever.

#define MyAppName "rapcoding"
#define MyAppVersion "0.3.0"
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
Name: "light"; Description: "Лёгкие модели (рекомендуется)"
Name: "heavy"; Description: "Тяжёлые модели (точнее)"
Name: "custom"; Description: "Выборочная установка"; Flags: iscustom

[Components]
; Both binaries are always installed; the Start Menu offers two entries:
; «интерфейс» (window with in-app log) and «консоль» (plain console).
Name: "ru_light"; Description: "Русская модель — лёгкая (~44 МБ)"; Types: light custom
Name: "ru_heavy"; Description: "Русская модель — тяжёлая (~1.8 ГБ, точнее)"; Types: heavy custom
Name: "en_light"; Description: "Английская модель — лёгкая (~39 МБ)"; Types: light custom
Name: "en_heavy"; Description: "Английская модель — тяжёлая (~1.8 ГБ, точнее)"; Types: heavy custom

[Tasks]
Name: "startmenu"; Description: "Создать ярлыки в меню «Пуск» (интерфейс и консоль)"; GroupDescription: "Ярлыки:"
; Windows 10/11 запрещает программное закрепление — попытка best-effort, не блокирует установку.
Name: "taskbar"; Description: "Закрепить окно программы в панели задач (может не сработать на Windows 10/11)"; GroupDescription: "Ярлыки:"; Flags: unchecked

[Files]
; GUI build becomes the main rapcoding.exe (window + in-app log, no console);
; the console build ships side by side under its own name.
Source: "..\target\release\rapcoding-gui.exe"; DestDir: "{app}"; DestName: "rapcoding.exe"; Flags: ignoreversion
Source: "..\target\release\rapcoding-console.exe"; DestDir: "{app}"; DestName: "rapcoding-console.exe"; Flags: ignoreversion
Source: "..\target\release\libvosk.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\libgcc_s_seh-1.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\libstdc++-6.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\libwinpthread-1.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "pin-taskbar.ps1"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; Two separate Start Menu entries: the window variant (forced --gui so the
; window opens even when gui.enabled=false in the config) and the plain console.
Name: "{group}\rapcoding (интерфейс)"; Filename: "{app}\rapcoding.exe"; Parameters: "--gui"; Tasks: startmenu
Name: "{group}\rapcoding (консоль)"; Filename: "{app}\rapcoding-console.exe"; Tasks: startmenu

[Run]
Filename: "powershell.exe"; \
    Parameters: "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File ""{app}\pin-taskbar.ps1"" -Target ""{app}\rapcoding.exe"""; \
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
