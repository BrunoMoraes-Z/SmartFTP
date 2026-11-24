; SmartFTP installer (Inno Setup 6).
; Build: iscc /DAppVersion=0.2.0 /DSourceExe=..\target\release\smartftp.exe installer\smartftp.iss
; or `just installer`. The release workflow builds it for every tag.
;
; Per-user install (no admin prompt) into %LOCALAPPDATA%\Programs\SmartFTP, so the
; in-app updater can replace the executable without elevation. User settings live in
; %APPDATA%\SmartFTP and are kept on uninstall unless the user asks to remove them.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\target\release\smartftp.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\dist"
#endif

#define AppName "SmartFTP"
#define AppExe "smartftp.exe"
#define AppUrl "https://github.com/BrunoMoraes-Z/SmartFTP"

[Setup]
; Never change AppId: it identifies the installation for upgrades and uninstall.
AppId={{6F2C1E5B-8E3A-4B7D-9C41-2A7D5E9F3B18}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Bruno Moraes
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=commandline dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=SmartFTP-{#AppVersion}-setup
SetupIconFile=..\assets\app.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
ShowLanguageDialog=auto
UsePreviousLanguage=no

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ptbr"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"

[CustomMessages]
en.RemoveSettings=Also remove your SmartFTP settings and saved sites?
ptbr.RemoveSettings=Remover também as configurações e os sites salvos do SmartFTP?
es.RemoveSettings=¿Eliminar también la configuración y los sitios guardados de SmartFTP?

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent runasoriginaluser
; started by the in-app updater (/UPDATE=1): reopen SmartFTP, as the user rather than as admin
Filename: "{app}\{#AppExe}"; Flags: nowait runasoriginaluser; Check: IsUpdate

[UninstallDelete]
; the self-updater may leave a renamed copy of the previous executable behind
Type: files; Name: "{app}\*.old"
Type: dirifempty; Name: "{app}"

[Code]
function IsUpdate: Boolean;
begin
  Result := ExpandConstant('{param:UPDATE|0}') = '1';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if (CurUninstallStep = usPostUninstall) and not UninstallSilent then
    if MsgBox(CustomMessage('RemoveSettings'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      DelTree(ExpandConstant('{userappdata}\SmartFTP'), True, True, True);
end;
