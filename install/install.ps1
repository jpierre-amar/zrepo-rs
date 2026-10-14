# Installeur de zrepo-rs pour Windows (PowerShell 5.1 ou plus recent).
#
#   irm https://github.com/jpierre-amar/zrepo-rs/releases/latest/download/install.ps1 | iex
#
# Avec des options :
#   & ([scriptblock]::Create((irm https://github.com/jpierre-amar/zrepo-rs/releases/latest/download/install.ps1))) -Yes -NoSetup
#
# Parametres : -Yes, -Dir <chemin>, -Version <vX.Y.Z>, -NoSetup
# Variables  : ZREPO_RS_INSTALL_DIR, ZREPO_RS_VERSION
#
# Ce fichier est volontairement en ASCII pur : PowerShell 5.1 lit les scripts
# sans BOM en ANSI, et 'irm | iex' ne gere pas toujours l'UTF-8. Les accents des
# messages sont ecrits en sequences \uXXXX et decodes a l'execution (fonction Fr).
# Fr ne decode que le modele ; les valeurs (chemins avec \) passent par -f.

param(
    [switch]$Yes,
    [string]$Dir = '',
    [string]$Version = '',
    [switch]$NoSetup
)

function Install-Zrepo {
    [CmdletBinding()]
    param([switch]$Yes, [string]$Dir, [string]$Version, [switch]$NoSetup)

    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest bien plus rapide sans barre
    Set-StrictMode -Version 2.0

    $Repo = 'jpierre-amar/zrepo-rs'
    $GitHub = "https://github.com/$Repo"
    $Target = 'x86_64-pc-windows-msvc'
    $Archive = "zrepo-rs-$Target.zip"

    function Fr([string]$s, [object[]]$a) {
        $t = [regex]::Unescape($s)
        if ($a) { return ($t -f $a) }
        return $t
    }
    function Step([string]$s) { Write-Host '==> ' -ForegroundColor Blue -NoNewline; Write-Host $s -ForegroundColor White }
    function Ok([string]$s)   { Write-Host '  OK ' -ForegroundColor Green -NoNewline; Write-Host $s }
    function Warn([string]$s) { Write-Host '  !  ' -ForegroundColor Yellow -NoNewline; Write-Host $s }
    function Info([string]$s) { Write-Host "    $s" }

    # TLS 1.2 explicite (PowerShell 5.1 utilise TLS 1.0 par defaut sur certains systemes).
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    # ----------------------------------------------------------- plateforme --
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw (Fr 'Windows 32 bits n\u2019est pas pris en charge : zrepo-rs exige Windows x64.')
    }
    $isArm = ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') -or ($env:PROCESSOR_ARCHITEW6432 -eq 'ARM64')
    if (-not $isArm) {
        try {
            # 12 = ARM64 ; visible meme quand PowerShell tourne en emulation x64.
            $cpu = Get-CimInstance -ClassName Win32_Processor -ErrorAction Stop | Select-Object -First 1
            if ($cpu.Architecture -eq 12) { $isArm = $true }
        } catch { }
    }
    if ($isArm) {
        throw (Fr 'Windows sur ARM n\u2019est pas encore pris en charge (seul x86_64 est publi\u00e9). Vous pouvez compiler zrepo-rs vous-m\u00eame : cargo install --git https://github.com/jpierre-amar/zrepo-rs zrepo-rs')
    }

    if (-not $Version) { $Version = $env:ZREPO_RS_VERSION }
    if ($Version -eq 'latest') { $Version = '' }
    if ($Version -and -not $Version.StartsWith('v')) { $Version = "v$Version" }
    if ($Version) { $BaseUrl = "$GitHub/releases/download/$Version" } else { $BaseUrl = "$GitHub/releases/latest/download" }

    $label = $Version
    if (-not $label) { $label = Fr 'derni\u00e8re version' }
    Write-Host ''
    Write-Host "zrepo-rs - installation ($Target, $label)" -ForegroundColor White
    Write-Host ''

    # ------------------------------------------------------- dossier cible --
    $DefaultDir = Join-Path $env:LOCALAPPDATA 'Programs\zrepo-rs'
    $InstallDir = $Dir
    if (-not $InstallDir) { $InstallDir = $env:ZREPO_RS_INSTALL_DIR }
    if (-not $InstallDir) {
        $InstallDir = $DefaultDir
        if (-not $Yes -and [Environment]::UserInteractive -and $Host.Name -ne 'ServerRemoteHost') {
            try {
                $answer = Read-Host (Fr 'Dossier d''installation [{0}]' $DefaultDir)
                if ($answer) { $InstallDir = $answer.Trim().Trim('"') }
            } catch { }
        }
    }
    $InstallDir = [Environment]::ExpandEnvironmentVariables($InstallDir)
    if ($InstallDir.StartsWith('~')) { $InstallDir = $HOME + $InstallDir.Substring(1) }
    $InstallDir = [IO.Path]::GetFullPath($InstallDir).TrimEnd('\')
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $Exe = Join-Path $InstallDir 'zrepo-rs.exe'

    $Tmp = Join-Path ([IO.Path]::GetTempPath()) ('zrepo-rs-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $Tmp | Out-Null
    try {
        # ---------------------------------------------------- telechargement --
        $zip = Join-Path $Tmp $Archive
        $sumFile = "$zip.sha256"
        if ($env:ZREPO_RS_LOCAL_ARCHIVE) {
            Step ('Archive locale : ' + $env:ZREPO_RS_LOCAL_ARCHIVE)
            Copy-Item -LiteralPath $env:ZREPO_RS_LOCAL_ARCHIVE -Destination $zip
            if (Test-Path -LiteralPath "$($env:ZREPO_RS_LOCAL_ARCHIVE).sha256") {
                Copy-Item -LiteralPath "$($env:ZREPO_RS_LOCAL_ARCHIVE).sha256" -Destination $sumFile
            }
        } else {
            Step (Fr 'T\u00e9l\u00e9chargement de {0}' $Archive)
            Info "$BaseUrl/$Archive"
            try {
                Invoke-WebRequest -UseBasicParsing -Uri "$BaseUrl/$Archive" -OutFile $zip
                Invoke-WebRequest -UseBasicParsing -Uri "$BaseUrl/$Archive.sha256" -OutFile $sumFile
            } catch {
                throw (Fr 'T\u00e9l\u00e9chargement impossible ({0}). La version existe-t-elle ?' $_.Exception.Message)
            }
        }

        if (Test-Path -LiteralPath $sumFile) {
            Step (Fr 'V\u00e9rification de la somme SHA-256')
            $expected = ((Get-Content -LiteralPath $sumFile -TotalCount 1) -split '\s+')[0].ToLowerInvariant()
            $actual = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
            if (-not $expected -or $expected -ne $actual) {
                throw (Fr 'Somme SHA-256 incorrecte (attendu {0}, obtenu {1}). Installation annul\u00e9e.' @($expected, $actual))
            }
            Ok (Fr 'somme v\u00e9rifi\u00e9e')
        } else {
            Warn (Fr 'aucun fichier .sha256 pour l\u2019archive locale : v\u00e9rification saut\u00e9e')
        }

        $x = Join-Path $Tmp 'x'
        Expand-Archive -LiteralPath $zip -DestinationPath $x -Force
        $src = Get-ChildItem -LiteralPath $x -Recurse -Filter 'zrepo-rs.exe' | Select-Object -First 1
        if (-not $src) { throw (Fr 'zrepo-rs.exe absent de l\u2019archive') }

        # ------------------------------------------------------ installation --
        Step "Installation dans $InstallDir"
        # Un exe en cours d'execution ne peut pas etre ecrase, mais il peut etre renomme.
        $old = "$Exe.old"
        $hadOld = Test-Path -LiteralPath $Exe
        if (Test-Path -LiteralPath $Exe) {
            Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
            Move-Item -LiteralPath $Exe -Destination $old -Force
        }
        try {
            Copy-Item -LiteralPath $src.FullName -Destination $Exe -Force
        } catch {
            # Restaure la version precedente si la copie de la mise a jour echoue.
            if ($hadOld -and (Test-Path -LiteralPath $old)) {
                Move-Item -LiteralPath $old -Destination $Exe -Force
            }
            throw
        }
        try { Unblock-File -LiteralPath $Exe -ErrorAction SilentlyContinue } catch { }
        Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
        $lic = Get-ChildItem -LiteralPath $x -Recurse -Filter 'LICENSE' | Select-Object -First 1
        # Nom propre au nouveau produit : ne jamais ecraser le LICENSE d'un ancien zrepo
        # si l'utilisateur choisit volontairement le meme dossier d'installation.
        if ($lic) { Copy-Item -LiteralPath $lic.FullName -Destination (Join-Path $InstallDir 'LICENSE-zrepo-rs') -Force }
        Ok $Exe

        # Icone : celle integree a zrepo-rs.exe (rien d'autre a telecharger ni a verifier).
        $iconPath = "$Exe,0"

        # --------------------------------------------------------------- PATH --
        # Lecture brute (sans expansion des %VAR%) pour ne pas figer les entrees existantes.
        $envKey = Get-Item -LiteralPath 'HKCU:\Environment'
        $rawPath = [string]$envKey.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $entries = @($rawPath -split ';' | Where-Object { $_ -ne '' })
        $present = $false
        foreach ($e in $entries) {
            $exp = [Environment]::ExpandEnvironmentVariables($e).TrimEnd('\')
            if ($exp -ieq $InstallDir) { $present = $true; break }
        }
        if ($present) {
            Ok (Fr '{0} est d\u00e9j\u00e0 dans le PATH utilisateur' $InstallDir)
        } else {
            $newPath = (@($entries) + $InstallDir) -join ';'
            if ($rawPath -match '%') {
                # Conserve le type REG_EXPAND_SZ, puis notifie le systeme via SetEnvironmentVariable.
                Set-ItemProperty -LiteralPath 'HKCU:\Environment' -Name 'Path' -Value $newPath -Type ExpandString
                [Environment]::SetEnvironmentVariable('ZREPO_RS_PATH_REFRESH', '1', 'User')
                [Environment]::SetEnvironmentVariable('ZREPO_RS_PATH_REFRESH', $null, 'User')
            } else {
                [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
            }
            $env:Path = "$env:Path;$InstallDir"
            Ok (Fr '{0} ajout\u00e9 au PATH utilisateur (nouveaux terminaux)' $InstallDir)
        }

        # ---------------------------------------------------- menu Demarrer --
        $programs = [Environment]::GetFolderPath('Programs')
        $lnk = Join-Path $programs 'zrepo-rs.lnk'
        try {
            $shell = New-Object -ComObject WScript.Shell
            $sc = $shell.CreateShortcut($lnk)
            $sc.TargetPath = $Exe
            $sc.WorkingDirectory = $HOME
            $sc.IconLocation = $iconPath
            $sc.Description = 'zrepo-rs - lanceur de projets'
            $sc.Save()
            [void][Runtime.InteropServices.Marshal]::ReleaseComObject($shell)
            Ok (Fr 'raccourci du menu D\u00e9marrer : {0}' $lnk)
            Info (Fr 'Raccourci clavier : clic droit sur ce raccourci > Propri\u00e9t\u00e9s > Touche de raccourci.')
        } catch {
            Warn (Fr 'raccourci du menu D\u00e9marrer non cr\u00e9\u00e9 : {0}' $_.Exception.Message)
        }
    } finally {
        Remove-Item -LiteralPath $Tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host ''
    Write-Host (Fr 'zrepo-rs est install\u00e9.') -ForegroundColor Green
    if ($NoSetup) {
        Info 'Configurez-le avec : zrepo-rs setup'
    } else {
        Step (Fr 'Lancement de l\u2019assistant : zrepo-rs setup')
        try {
            Start-Process -FilePath $Exe -ArgumentList 'setup' -WorkingDirectory $HOME
        } catch {
            Warn (Fr 'impossible de lancer l\u2019assistant : {0}. Lancez \u00ab zrepo-rs setup \u00bb.' $_.Exception.Message)
        }
    }
}

# Pas de 'exit' : avec 'irm | iex', cela fermerait la fenetre PowerShell.
try {
    Install-Zrepo -Yes:$Yes -Dir $Dir -Version $Version -NoSetup:$NoSetup
} catch {
    Write-Host ''
    Write-Host ('erreur : ' + $_.Exception.Message) -ForegroundColor Red
    # Une erreur terminating ne ferme pas une console interactive, contrairement
    # a `exit`, mais rend bien l'echec visible aux scripts et a la CI.
    throw
}
