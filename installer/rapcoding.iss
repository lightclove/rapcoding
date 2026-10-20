; rapcoding installer (per-user, no admin required).
; Build: ISCC.exe installer\rapcoding.iss   (output: dist\RapcodingSetup-0.1.0.exe)

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

[Tasks]
Name: "startmenu"; Description: "Создать ярлык в меню «Пуск»"; GroupDescription: "Ярлыки:"
; Windows 10/11 запрещает программное закрепление — попытка best-effort, не блокирует установку.
Name: "taskbar"; Description: "Закрепить в панели задач (может не сработать на Windows 10/11)"; GroupDescription: "Ярлыки:"; Flags: unchecked

[Files]
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
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
