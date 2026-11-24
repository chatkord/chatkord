; The Windows setup's recipe, compiled by scripts/package/windows.sh, which passes Version and
; exports the channel's names (client/channel/channels/<channel>.env). Per user, no
; administrator; Restart Manager closes a running window on install, upgrade and uninstall.

#define Name GetEnv("CHANNEL_PRODUCT")
#define Pkg GetEnv("CHANNEL_FILE")
#define AppUserModelId GetEnv("CHANNEL_APP_USER_MODEL_ID")
; Inno reads a doubled brace as one, so the GUID's own brace is written twice.
#define SetupId "{" + GetEnv("CHANNEL_SETUP_ID")
#if Name == "" || Pkg == "" || AppUserModelId == "" || SetupId == "{"
  #error The channel's names are exported by scripts/package/windows.sh
#endif
#ifndef Version
  #error Version is passed by scripts/package/windows.sh
#endif
#define Exe Pkg + ".exe"

[Setup]
; One install's identity across every version: an upgrade replaces it, never sits beside it. Never changed.
AppId={#SetupId}
AppName={#Name}
AppVersion={#Version}
AppVerName={#Name} {#Version}
PrivilegesRequired=lowest
DefaultDirName={autopf}\{#Name}
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
SetupArchitecture=x64
ArchitecturesAllowed=x64compatible
CloseApplications=yes
RestartApplications=no
SetupIconFile=..\..\client\shell\assets\icon\chatkord.ico
UninstallDisplayIcon={app}\{#Exe}
UninstallDisplayName={#Name}
WizardStyle=modern
OutputDir=..\..\build\package
OutputBaseFilename={#Pkg}-setup-{#Version}

[Files]
Source: "..\..\client\target\release\shell.exe"; DestDir: "{app}"; DestName: "{#Exe}"; Flags: ignoreversion
Source: "..\..\build\deps\bin\*.dll"; DestDir: "{app}"; Flags: ignoreversion
; Every carried project's licence, our own and the notices the About page reads, which scripts/package/notices.sh
; put together and checked whole.
Source: "..\..\build\package\licenses\*"; DestDir: "{app}\licenses"; Flags: ignoreversion recursesubdirs
; The recipe that made what the setup carries, for anyone who rebuilds it, as the GPL of FFmpeg and x264 asks.
Source: "..\..\client\channel\channels\*.env"; DestDir: "{app}\recipe\channels"; Flags: ignoreversion
Source: "..\..\scripts\package\channel.sh"; DestDir: "{app}\recipe"; Flags: ignoreversion
Source: "..\..\scripts\package\windows.sh"; DestDir: "{app}\recipe"; Flags: ignoreversion
Source: "..\..\scripts\package\windows.iss"; DestDir: "{app}\recipe"; Flags: ignoreversion
Source: "..\..\scripts\package\budget.sh"; DestDir: "{app}\recipe"; Flags: ignoreversion
Source: "..\..\scripts\deps\ffmpeg-windows.sh"; DestDir: "{app}\recipe"; Flags: ignoreversion
Source: "..\..\scripts\deps\ffmpeg-pin.sh"; DestDir: "{app}\recipe"; Flags: ignoreversion
Source: "..\..\scripts\deps\codecs-pin.sh"; DestDir: "{app}\recipe"; Flags: ignoreversion
Source: "..\..\scripts\deps\source.sh"; DestDir: "{app}\recipe"; Flags: ignoreversion
Source: "..\..\scripts\deps\webrtc.sh"; DestDir: "{app}\recipe"; Flags: ignoreversion
Source: "..\..\patches\webrtc\*"; DestDir: "{app}\recipe\webrtc-patches"; Flags: ignoreversion recursesubdirs
Source: "..\..\patches\ffmpeg\*"; DestDir: "{app}\recipe\ffmpeg-patches"; Flags: ignoreversion recursesubdirs

[Icons]
; The AppUserModelID is the channel's, platform::notify's: a toast takes its name and icon from the shortcut
; carrying it.
Name: "{autoprograms}\{#Name}"; Filename: "{app}\{#Exe}"; AppUserModelID: "{#AppUserModelId}"

[Registry]
; The entry Settings › Startup writes and Task Manager's switch for it (platform::startup), removed at uninstall
; whether or not the person's settings go too, so Windows never starts a program that is gone; never created here.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "{#Name}"; ValueType: none; Flags: uninsdeletevalue dontcreatekey
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"; ValueName: "{#Name}"; ValueType: none; Flags: uninsdeletevalue dontcreatekey

[Run]
Filename: "{app}\{#Exe}"; Description: "Open {#Name}"; Flags: nowait postinstall skipifsilent
; The window's own update runs this setup unseen once the window has ended, which is opened again here.
Filename: "{app}\{#Exe}"; Flags: nowait; Check: IsUpdate

[CustomMessages]
ForgetAsk=Also remove your settings and sign-ins?
ForgetFailed=Some of your settings or sign-ins could not be removed.
StillRunning={#Name} could not be closed. Close it and try again.

[Code]
const
  CCH_RM_SESSION_KEY = 32;
  ERROR_SUCCESS = 0;

function RmStartSession(out Session: DWORD; Flags: DWORD; Key: String): DWORD;
  external 'RmStartSession@rstrtmgr.dll stdcall uninstallonly';
// A `var` string is passed as the address of its characters' address: the one-file array the call reads.
function RmRegisterResources(Session: DWORD; Files: UINT; var Names: String; Applications: UINT; Processes: DWORD;
  Services: UINT; ServiceNames: DWORD): DWORD;
  external 'RmRegisterResources@rstrtmgr.dll stdcall uninstallonly';
function RmShutdown(Session: DWORD; Flags: DWORD; Status: DWORD): DWORD;
  external 'RmShutdown@rstrtmgr.dll stdcall uninstallonly';
function RmEndSession(Session: DWORD): DWORD;
  external 'RmEndSession@rstrtmgr.dll stdcall uninstallonly';

// Started by the window's own update, which passes `/update`; a person's own silent install opens nothing.
function IsUpdate(): Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/update') = 0 then
      Result := True;
end;

// The uninstaller has no Restart Manager of its own: it asks Windows to close the window as Setup does, unforced.
function ClosedWindow(): Boolean;
var
  Session: DWORD;
  Key, Path: String;
begin
  Key := StringOfChar(#0, CCH_RM_SESSION_KEY + 1);
  if RmStartSession(Session, 0, Key) <> ERROR_SUCCESS then
  begin
    Result := False;
    Exit;
  end;
  Path := ExpandConstant('{app}\{#Exe}');
  Result := (RmRegisterResources(Session, 1, Path, 0, 0, 0, 0) = ERROR_SUCCESS)
    and (RmShutdown(Session, 0, 0) = ERROR_SUCCESS);
  RmEndSession(Session);
end;

// After the person confirmed and before a file goes, so a cancelled uninstall leaves the window open.
procedure CurUninstallStepChanged(Step: TUninstallStep);
var
  Code: Integer;
begin
  if Step <> usUninstall then
    Exit;
  if not ClosedWindow() then
  begin
    SuppressibleMsgBox(CustomMessage('StillRunning'), mbError, MB_OK, IDOK);
    Abort;
  end;
  if (not UninstallSilent) and
    (MsgBox(CustomMessage('ForgetAsk'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
    if not Exec(ExpandConstant('{app}\{#Exe}'), '--forget', '', SW_HIDE, ewWaitUntilTerminated, Code) or (Code <> 0) then
      MsgBox(CustomMessage('ForgetFailed'), mbError, MB_OK);
end;
