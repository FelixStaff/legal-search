; LegalSearch — per-user installer (no administrator rights required).
;
; Build with scripts\build-all.ps1 (it prepares every input below). Manual:
;   ISCC.exe installer\LegalSearch.iss
;
; Installs to %LOCALAPPDATA%\LegalSearch and registers the Chrome Native
; Messaging host under HKCU only. Nothing else on the system is modified:
; no services, no scheduled tasks, no startup entries, no firewall rules.

#include "build\config.iss"
#define AppName "LegalSearch"
#define AppVersion "1.1.0-rc2"
#define FileVersion "1.1.0"
#define Stage "..\dist\stage"

[Setup]
AppId={{6E0F1C2A-4B7D-4E0B-9C51-3A8F2D7B9E41}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=LegalSearch
VersionInfoVersion={#FileVersion}
VersionInfoDescription=LegalSearch - local semantic search in PDFs
DefaultDirName={localappdata}\{#AppName}
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=no
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=LegalSearchSetup
SetupIconFile=legalsearch.ico
UninstallDisplayIcon={app}\legalsearch.ico
UninstallDisplayName={#AppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Close a running LegalSearch.exe (open side panel) before replacing files.
CloseApplications=force
RestartApplications=no
SetupLogging=no

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"

[Messages]
en.FinishedLabel=LegalSearch was installed successfully.%n%nLast step: turn on the LegalSearch extension in Google Chrome. The guide that opens next shows you how in about a minute.

[Files]
Source: "{#Stage}\bin\*"; DestDir: "{app}\bin"; Flags: ignoreversion recursesubdirs
Source: "{#Stage}\model\*"; DestDir: "{app}\model"; Flags: ignoreversion
Source: "{#Stage}\extension\*"; DestDir: "{app}\extension"; Flags: ignoreversion recursesubdirs
Source: "{#Stage}\licenses\*"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "legalsearch.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "GUIDE.template.html"; DestDir: "{tmp}"; Flags: dontcopy

[Dirs]
Name: "{app}\data"
Name: "{app}\logs"

[Registry]
; The ONLY registry entry (besides Windows' own uninstall entry): the Chrome
; Native Messaging host registration, per user.
Root: HKCU; Subkey: "Software\Google\Chrome\NativeMessagingHosts\{#HostName}"; ValueType: string; ValueName: ""; ValueData: "{app}\{#HostName}.json"; Flags: uninsdeletekey

[Run]
Filename: "{app}\GUIDE.html"; Description: "Open the guide to turn on LegalSearch in Chrome"; Flags: postinstall shellexec skipifsilent nowait

[UninstallDelete]
; Files the installer or the program created (not the cache; see [Code]).
Type: files; Name: "{app}\{#HostName}.json"
Type: files; Name: "{app}\GUIDE.html"
Type: filesandordirs; Name: "{app}\logs"

[Code]
function JsonEscape(const S: String): String;
begin
  Result := S;
  StringChangeEx(Result, '\', '\\', True);
  StringChangeEx(Result, '"', '\"', True);
end;

procedure WriteHostManifest();
var
  Json: String;
begin
  { allowed_origins restricts the engine to the LegalSearch extension only. }
  Json :=
    '{' + #13#10 +
    '  "name": "{#HostName}",' + #13#10 +
    '  "description": "LegalSearch local engine",' + #13#10 +
    '  "path": "' + JsonEscape(ExpandConstant('{app}\bin\LegalSearch.exe')) + '",' + #13#10 +
    '  "type": "stdio",' + #13#10 +
    '  "allowed_origins": ["chrome-extension://{#ExtensionId}/"]' + #13#10 +
    '}' + #13#10;
  if not SaveStringToFile(ExpandConstant('{app}\{#HostName}.json'), Json, False) then
    RaiseException('The local engine could not be registered.');
end;

procedure WriteGuide();
var
  Html: AnsiString;
  S: String;
begin
  ExtractTemporaryFile('GUIDE.template.html');
  if LoadStringFromFile(ExpandConstant('{tmp}\GUIDE.template.html'), Html) then
  begin
    S := UTF8Decode(Html);
    StringChangeEx(S, '{{EXTENSION_PATH}}', ExpandConstant('{app}\extension'), True);
    StringChangeEx(S, '{{WEBSTORE_URL}}', '{#WebStoreUrl}', True);
    SaveStringToFile(ExpandConstant('{app}\GUIDE.html'), UTF8Encode(S), False);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    WriteHostManifest();
    WriteGuide();
  end;
end;

procedure StopEngine();
var
  Code: Integer;
begin
  { LegalSearch.exe only runs while a Chrome side panel is open. Stop it so its
    files can be removed; nothing else is ever terminated. }
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM LegalSearch.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

procedure CleanUninstallerTempCopy();
var
  Dir, Temp, Cmd: String;
  Code: Integer;
begin
  { Inno Setup runs the uninstaller from a temporary copy
    (%TEMP%\is-XXXX-uninstall.tmp\_unins.tmp) that cannot delete itself while
    running, and would otherwise stay until another Inno uninstall runs. Schedule
    its removal in a hidden background command that waits for it to exit.
    Only that exact folder, derived from our own path, is ever targeted. }
  Dir := RemoveBackslash(ExtractFileDir(ParamStr(0)));
  Temp := RemoveBackslash(GetTempDir());
  if (CompareText(RemoveBackslash(ExtractFileDir(Dir)), Temp) <> 0) or
     (Pos('IS-', Uppercase(ExtractFileName(Dir))) <> 1) or
     (Pos('-UNINSTALL.TMP', Uppercase(ExtractFileName(Dir))) = 0) then
    exit;
  Cmd := '/d /c for /l %i in (1,1,60) do (rd /s /q "' + Dir + '" 2>nul & if not exist "' + Dir +
    '" exit) & ping -n 2 127.0.0.1 >nul';
  Exec(ExpandConstant('{sys}\cmd.exe'), Cmd, Temp, SW_HIDE, ewNoWait, Code);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DeleteCache: Boolean;
begin
  if CurUninstallStep = usDone then
    CleanUninstallerTempCopy();
  if CurUninstallStep = usUninstall then
    StopEngine();
  if CurUninstallStep = usPostUninstall then
  begin
    { Inno Setup has no checkbox page during uninstall, so we ask with a Yes/No
      dialog (default: No = keep). Silent uninstalls delete the cache because it
      is fully regenerable, unless /KEEPCACHE=1 is passed. See docs/installation.md. }
    if ExpandConstant('{param:KEEPCACHE|0}') = '1' then
      DeleteCache := False
    else if UninstallSilent() then
      DeleteCache := True
    else
      DeleteCache := SuppressibleMsgBox('Also delete indexed documents and the local cache?' + #13#10 + #13#10 +
        'If you choose "No", they are kept for a future reinstall.',
        mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) = IDYES;
    if DeleteCache then
      DelTree(ExpandConstant('{app}\data'), True, True, True);
    { Remove the application folder if nothing (else) is left in it. }
    RemoveDir(ExpandConstant('{app}'));
  end;
end;
