; Installeur Windows de zrepo-rs (Inno Setup 6).
;
; Mêmes choix que install/install.ps1 : installation pour l'utilisateur courant,
; sans droits administrateur, dans %LOCALAPPDATA%\Programs\zrepo-rs, raccourci
; du menu Démarrer, ajout facultatif au PATH utilisateur. En plus : une entrée
; « Applications installées » pour désinstaller proprement.
;
; Compilation (depuis la racine du dépôt) :
;   iscc /DAppVersion=0.3.1 /DSourceExe=target\release\zrepo-rs.exe packaging\windows\zrepo-rs.iss
; Le setup est écrit dans dist\.
;
; La désinstallation garde la configuration (%APPDATA%\zrepo-rs), le cache et
; le jeton GitHub du Gestionnaire d'identification, comme une réinstallation
; les retrouverait.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\zrepo-rs.exe"
#endif

[Setup]
; Ne jamais changer AppId : il relie les mises à jour et la désinstallation.
AppId={{C934C8B8-C267-4E05-8A64-E3FF08C97084}
AppName=zrepo-rs
AppVersion={#AppVersion}
AppVerName=zrepo-rs {#AppVersion}
AppPublisher=jpierre-amar
AppPublisherURL=https://github.com/jpierre-amar/zrepo-rs
AppSupportURL=https://github.com/jpierre-amar/zrepo-rs/issues
AppUpdatesURL=https://github.com/jpierre-amar/zrepo-rs/releases
DefaultDirName={localappdata}\Programs\zrepo-rs
DisableDirPage=auto
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
CloseApplications=yes
LicenseFile=..\..\LICENSE
SetupIconFile=..\zrepo-rs.ico
UninstallDisplayIcon={app}\zrepo-rs.exe
UninstallDisplayName=zrepo-rs
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
OutputDir=..\..\dist
OutputBaseFilename=zrepo-rs-setup-x86_64

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
french.AddToPath=Ajouter zrepo-rs au PATH (commande « zrepo-rs » dans les terminaux)
english.AddToPath=Add zrepo-rs to PATH (« zrepo-rs » command in terminals)
french.DesktopIcon=Créer une icône sur le Bureau
english.DesktopIcon=Create a desktop icon
french.RunSetup=Lancer l'assistant de configuration
english.RunSetup=Run the setup wizard
french.Description=zrepo-rs - lanceur de projets
english.Description=zrepo-rs - project launcher

[Tasks]
Name: "addtopath"; Description: "{cm:AddToPath}"
Name: "desktopicon"; Description: "{cm:DesktopIcon}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "zrepo-rs.exe"; Flags: ignoreversion
; Nom propre au produit, comme install.ps1 : ne pas écraser le LICENSE d'un
; autre outil installé dans le même dossier.
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE-zrepo-rs"; Flags: ignoreversion

[Icons]
; Même raccourci que install.ps1 : une installation remplace l'autre.
Name: "{userprograms}\zrepo-rs"; Filename: "{app}\zrepo-rs.exe"; WorkingDir: "{%USERPROFILE}"; Comment: "{cm:Description}"
Name: "{userdesktop}\zrepo-rs"; Filename: "{app}\zrepo-rs.exe"; WorkingDir: "{%USERPROFILE}"; Comment: "{cm:Description}"; Tasks: desktopicon

[Run]
Filename: "{app}\zrepo-rs.exe"; Parameters: "setup"; WorkingDir: "{%USERPROFILE}"; Description: "{cm:RunSetup}"; Flags: nowait postinstall skipifsilent

[Code]
const
  EnvKey = 'Environment';

{ Le dossier figure-t-il déjà dans le PATH (sans tenir compte de la casse ni
  d'une barre oblique finale) ? }
function PathHas(const Paths, Dir: string): Boolean;
var
  Rest, Item: string;
  P: Integer;
begin
  Result := False;
  Rest := Paths + ';';
  while Rest <> '' do
  begin
    P := Pos(';', Rest);
    Item := RemoveBackslashUnlessRoot(Trim(Copy(Rest, 1, P - 1)));
    Delete(Rest, 1, P);
    if (Item <> '') and (CompareText(Item, RemoveBackslashUnlessRoot(Dir)) = 0) then
    begin
      Result := True;
      Exit;
    end;
  end;
end;

procedure AddToPath(const Dir: string);
var
  Paths: string;
begin
  if not RegQueryStringValue(HKCU, EnvKey, 'Path', Paths) then
    Paths := '';
  if PathHas(Paths, Dir) then
    Exit;
  if (Paths <> '') and (Paths[Length(Paths)] <> ';') then
    Paths := Paths + ';';
  { REG_EXPAND_SZ : les entrées en %VAR% restent dynamiques. }
  RegWriteExpandStringValue(HKCU, EnvKey, 'Path', Paths + Dir);
end;

procedure RemoveFromPath(const Dir: string);
var
  Paths, Rest, Item, Kept: string;
  P: Integer;
begin
  if not RegQueryStringValue(HKCU, EnvKey, 'Path', Paths) then
    Exit;
  Kept := '';
  Rest := Paths + ';';
  while Rest <> '' do
  begin
    P := Pos(';', Rest);
    Item := Copy(Rest, 1, P - 1);
    Delete(Rest, 1, P);
    if (Trim(Item) <> '') and
       (CompareText(RemoveBackslashUnlessRoot(Trim(Item)), RemoveBackslashUnlessRoot(Dir)) <> 0) then
    begin
      if Kept <> '' then
        Kept := Kept + ';';
      Kept := Kept + Item;
    end;
  end;
  if Kept <> Paths then
    RegWriteExpandStringValue(HKCU, EnvKey, 'Path', Kept);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    AddToPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromPath(ExpandConstant('{app}'));
end;
