; rapcoding installer (per-user, no admin required).
; Build from the repo root (needs Rust + cargo in PATH):
;   1. powershell -NoProfile -ExecutionPolicy Bypass -File installer\build.ps1
;   2. ISCC.exe installer\rapcoding.iss
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
; Version: one checkbox — checked = GUI, unchecked = console-only.
Name: "gui"; Description: "Графический интерфейс (снимите отметку — только консоль)"; Types: gui full custom
; Models (independent checkboxes; each installs into its own directory).
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
Source: "extract-models.ps1"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: startmenu

[Run]
Filename: "powershell.exe"; \
    Parameters: "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File ""{app}\extract-models.ps1"" -SrcDir ""{tmp}\rapcoding-models"" -Dest ""{userappdata}\rapcoding\models"""; \
    StatusMsg: "Распаковка моделей распознавания…"; \
    Flags: runhidden; Check: AnyModelSelected
Filename: "powershell.exe"; \
    Parameters: "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File ""{app}\pin-taskbar.ps1"" -Target ""{app}\{#MyAppExeName}"""; \
    Tasks: taskbar; Flags: runhidden

[Code]
var
  DownloadPage: TDownloadWizardPage;
  AnyModelDownload: Boolean;
  DeleteUserData: TNewCheckBox;

function AnyModelSelected: Boolean;
begin
  Result := IsComponentSelected('ru_light') or IsComponentSelected('ru_heavy')
         or IsComponentSelected('en_light') or IsComponentSelected('en_heavy');
end;

// Component selection: IsComponentSelected at install time; in the wizard
// (interactive) the selection is only committed on the ready page, so use
// WizardIsComponentSelected there.
function ModelSelected(const id: String): Boolean;
begin
  if WizardSilent then
    Result := IsComponentSelected(id)
  else
    Result := WizardIsComponentSelected(id);
end;

procedure AddSelectedModels;
begin
  DownloadPage.Clear;
  AnyModelDownload := False;
  if ModelSelected('ru_light') then begin
    DownloadPage.Add('https://alphacephei.com/vosk/models/vosk-model-small-ru-0.22.zip', 'rapcoding-models\vosk-model-small-ru-0.22.zip', '');
    AnyModelDownload := True;
  end;
  if ModelSelected('ru_heavy') then begin
    DownloadPage.Add('https://alphacephei.com/vosk/models/vosk-model-ru-0.42.zip', 'rapcoding-models\vosk-model-ru-0.42.zip', '');
    AnyModelDownload := True;
  end;
  if ModelSelected('en_light') then begin
    DownloadPage.Add('https://alphacephei.com/vosk/models/vosk-model-small-en-us-0.15.zip', 'rapcoding-models\vosk-model-small-en-us-0.15.zip', '');
    AnyModelDownload := True;
  end;
  if ModelSelected('en_heavy') then begin
    DownloadPage.Add('https://alphacephei.com/vosk/models/vosk-model-en-us-0.22.zip', 'rapcoding-models\vosk-model-en-us-0.22.zip', '');
    AnyModelDownload := True;
  end;
end;

procedure InitializeWizard;
begin
  ForceDirectories(ExpandConstant('{tmp}\rapcoding-models'));
  DownloadPage := CreateDownloadPage('Скачивание моделей', 'Загружаются модели распознавания…', nil);
  if WizardSilent then
    AddSelectedModels;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if CurPageID = wpReady then
    AddSelectedModels;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then begin
    if AnyModelDownload then begin
      DownloadPage.Show;
      try
        DownloadPage.Download;
      finally
        DownloadPage.Hide;
      end;
    end;
  end;
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
