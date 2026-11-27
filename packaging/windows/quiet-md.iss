#ifndef AppVersion
  #define AppVersion "0.1.14"
#endif
#ifndef AppSource
  #define AppSource "..\..\target\dist"
#endif

[Setup]
AppId={{59DC98EE-A136-40C9-A7BD-EB0CC50E37C2}
AppName=Quiet.md
AppVersion={#AppVersion}
AppPublisher=Quiet.md
AppPublisherURL=https://github.com/s1dashu/quiet-md
DefaultDirName={localappdata}\Programs\quiet.md
DisableDirPage=no
DisableProgramGroupPage=yes
DisableWelcomePage=yes
DisableReadyPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir=..\..\dist\windows
OutputBaseFilename=quiet-md-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\assets\branding\app-icon.ico
UninstallDisplayIcon={app}\quiet_md.exe
WizardStyle=modern light
WizardSmallImageFile=wizard-logo.bmp
WizardSmallImageBackColor=$FFFFFF
WizardImageStretch=no
WizardSizePercent=105,105
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
ChangesAssociations=yes
; The quiet command on the user's PATH (task addtopath): running programs hear of it.
ChangesEnvironment=yes
SetupLogging=yes
#ifdef Signed
SignTool=quietmd
SignedUninstaller=yes
SignedUninstallerDir={#AppSource}
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
SelectDirLabel3=Choose an install folder. Your notes are stored separately.
SelectTasksLabel2=Make Quiet.md easier to reach.
FinishedLabel=Quiet.md is ready. A quiet place for your notes.

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked
Name: "addtopath"; Description: "Add the quiet command to PATH (for the terminal and AI agents)"

[Files]
Source: "{#AppSource}\quiet_md.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#AppSource}\quiet.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#AppSource}\vcruntime140.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"
Source: "..\..\THIRD_PARTY.md"; DestDir: "{app}"

[Icons]
Name: "{userprograms}\Quiet.md"; Filename: "{app}\quiet_md.exe"; WorkingDir: "{app}"
Name: "{userdesktop}\Quiet.md"; Filename: "{app}\quiet_md.exe"; WorkingDir: "{app}"; Tasks: desktopicon

; "Open with" for Markdown files and folders (not the default app): they are edited in place.
[Registry]
Root: HKA; Subkey: "Software\Classes\QuietMd.Markdown"; ValueType: string; ValueName: ""; ValueData: "Markdown"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\QuietMd.Markdown\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\quiet_md.exe,0"
Root: HKA; Subkey: "Software\Classes\QuietMd.Markdown\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\quiet_md.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\.md\OpenWithProgids"; ValueType: string; ValueName: "QuietMd.Markdown"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.markdown\OpenWithProgids"; ValueType: string; ValueName: "QuietMd.Markdown"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\quiet_md.exe\SupportedTypes"; ValueType: string; ValueName: ".md"; ValueData: ""; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\quiet_md.exe\SupportedTypes"; ValueType: string; ValueName: ".markdown"; ValueData: ""
; Folders: "Open with Quiet.md" in the Explorer's context menu, listed under 本机文件.
Root: HKA; Subkey: "Software\Classes\Directory\shell\QuietMd"; ValueType: string; ValueName: ""; ValueData: "Open with Quiet.md"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\shell\QuietMd"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\quiet_md.exe,0"
Root: HKA; Subkey: "Software\Classes\Directory\shell\QuietMd\command"; ValueType: string; ValueName: ""; ValueData: """{app}\quiet_md.exe"" ""%1"""
; quietmd:// links (`quiet open`): they only navigate, see crates/core/src/app_url.rs.
Root: HKA; Subkey: "Software\Classes\quietmd"; ValueType: string; ValueName: ""; ValueData: "URL:Quiet.md link"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\quietmd"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\quietmd\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\quiet_md.exe,0"
Root: HKA; Subkey: "Software\Classes\quietmd\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\quiet_md.exe"" ""%1"""

[Run]
Filename: "{app}\quiet_md.exe"; Description: "Launch Quiet.md"; Flags: nowait postinstall skipifsilent
; The in-app updater installs silently with /UPDATE=1 and expects the app back afterwards.
Filename: "{app}\quiet_md.exe"; Flags: nowait; Check: IsUpdate

[Code]
function IsUpdate: Boolean;
begin
  Result := ExpandConstant('{param:UPDATE|0}') = '1';
end;

// The install folder on the current user's PATH (HKCU\Environment), once however
// often it is installed; taken off again on uninstall, or when the task is unticked.
const EnvKey = 'Environment';

function PathPos(Paths, Dir: String): Integer;
begin
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Paths) + ';');
end;

procedure AddToPath(Dir: String);
var Paths: String;
begin
  if not RegQueryStringValue(HKCU, EnvKey, 'Path', Paths) then Paths := '';
  if PathPos(Paths, Dir) > 0 then exit;
  if (Paths <> '') and (Paths[Length(Paths)] <> ';') then Paths := Paths + ';';
  RegWriteExpandStringValue(HKCU, EnvKey, 'Path', Paths + Dir);
end;

procedure RemoveFromPath(Dir: String);
var Paths: String; At: Integer;
begin
  if not RegQueryStringValue(HKCU, EnvKey, 'Path', Paths) then exit;
  At := PathPos(Paths, Dir);
  if At = 0 then exit;
  Paths := ';' + Paths + ';';
  Delete(Paths, At, Length(Dir) + 1);
  RegWriteExpandStringValue(HKCU, EnvKey, 'Path', Copy(Paths, 2, Length(Paths) - 2));
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    if WizardIsTaskSelected('addtopath') then AddToPath(ExpandConstant('{app}')) else RemoveFromPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then RemoveFromPath(ExpandConstant('{app}'));
end;

procedure InitializeWizard;
begin
  WizardForm.Caption := 'Quiet.md';
end;
