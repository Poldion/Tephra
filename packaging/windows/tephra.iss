; Installer for Windows (Inno Setup 6). Built by the release workflow:
;
;   iscc /DAppVersion=0.3.1 /DSourceDir=<folder with the programs> /DOutputDir=<folder> packaging\windows\tephra.iss
;
; Installs for the current user without administrator rights: the app, the command line `tephra.exe`
; and the sync server `tephra-server.exe`.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\dist\Tephra"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

[Setup]
; Identifies the app for updates and uninstalling. Never change it.
AppId={{907C05A4-7F72-48D2-BF12-640B73787A50}
AppName=Tephra
AppVersion={#AppVersion}
AppVerName=Tephra {#AppVersion}
AppPublisher=Pold
AppPublisherURL=https://github.com/Poldion/Tephra
AppSupportURL=https://github.com/Poldion/Tephra/issues
AppUpdatesURL=https://github.com/Poldion/Tephra/releases
DefaultDirName={autopf}\Tephra
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile={#SourceDir}\LICENSE
SetupIconFile=..\..\app\windows\runner\resources\app_icon.ico
UninstallDisplayIcon={app}\tephra-app.exe
UninstallDisplayName=Tephra
OutputDir={#OutputDir}
OutputBaseFilename=Tephra-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ChangesEnvironment=yes
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[CustomMessages]
english.AddToPath=Add the command line "tephra" to the PATH
german.AddToPath=Kommandozeile "tephra" in den PATH aufnehmen

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "addtopath"; Description: "{cm:AddToPath}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{autoprograms}\Tephra"; Filename: "{app}\tephra-app.exe"
Name: "{autodesktop}\Tephra"; Filename: "{app}\tephra-app.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; \
  Tasks: addtopath; Check: NeedsAddPath(ExpandConstant('{app}'))

[Run]
Filename: "{app}\tephra-app.exe"; Description: "{cm:LaunchProgram,Tephra}"; Flags: nowait postinstall skipifsilent
; When the app updates itself it runs the installer silently with /RELAUNCH=1 and wants to be started again.
Filename: "{app}\tephra-app.exe"; Flags: nowait skipifnotsilent; Check: Relaunch

[Code]
function Relaunch: Boolean;
begin
  Result := ExpandConstant('{param:RELAUNCH|0}') = '1';
end;

// True if the folder is not yet part of the PATH of the user.
function NeedsAddPath(Dir: string): Boolean;
var
  Path: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Path) then
    Path := '';
  Result := Pos(';' + Lowercase(Dir) + ';', ';' + Lowercase(Path) + ';') = 0;
end;

// Takes the folder out of the PATH of the user again.
procedure RemovePath(Dir: string);
var
  Path, Padded: string;
  P: Integer;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Path) then
    exit;
  Padded := ';' + Path + ';';
  P := Pos(';' + Lowercase(Dir) + ';', Lowercase(Padded));
  if P = 0 then
    exit;
  Delete(Padded, P, Length(Dir) + 1);
  Path := Copy(Padded, 2, Length(Padded) - 2);
  RegWriteExpandStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Path);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemovePath(ExpandConstant('{app}'));
end;
