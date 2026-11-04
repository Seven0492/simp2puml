# Lanceur Windows de simp2puml.
# 1. Utilise un Python >= 3.11 avec Tkinter s'il existe déjà (lanceur py ou PATH).
# 2. Sinon installe uv (version fixée, sommes SHA-256 vérifiées par son installateur), puis un Python
#    géré par uv, le tout dans %LOCALAPPDATA%\simp2puml, sans droits administrateur.
# 3. Crée les raccourcis (menu Démarrer, Bureau au premier lancement), puis ouvre l'interface.
# Désinstallation : supprimer %LOCALAPPDATA%\simp2puml et les raccourcis « simp2puml ».
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$UvVersion = '0.13.7'
$PythonVersion = '3.12'

$Root = Split-Path -Parent $PSScriptRoot
$Start = Join-Path $Root 'launchers\start.pyw'
$Icon = Join-Path $Root 'src\simp2puml\assets\simp2puml.ico'
$Data = Join-Path $env:LOCALAPPDATA 'simp2puml'
$Log = Join-Path $Data 'derniere-execution.log'
$Check = 'import sys, tkinter; tkinter.Tcl(); sys.exit(sys.version_info < (3, 11))'

# Windows PowerShell 5.1 transforme la sortie d'erreur d'un programme en erreur bloquante : l'ignorer ici.
function Invoke-Quiet([scriptblock]$Block) {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $Block 2>$null } finally { $ErrorActionPreference = $previous }
}

function Fail([string]$Message) {
    Write-Host ''
    Write-Host "Erreur : $Message" -ForegroundColor Red
    exit 1
}

# Renvoie le chemin de python.exe si la commande fournit un Python >= 3.11 avec Tkinter.
function Find-Executable([string]$Command, [string[]]$Prefix) {
    try {
        Invoke-Quiet { & $Command @Prefix -c $Check } | Out-Null
        if ($LASTEXITCODE -ne 0) { return $null }
        $path = Invoke-Quiet { & $Command @Prefix -c 'import sys; print(sys.executable)' } | Select-Object -First 1
        if ($path) { return $path.Trim() }
    } catch { }
    return $null
}

function Find-Python {
    if ($env:SIMP2PUML_FORCE_MANAGED) { return $null }
    if (Get-Command py -CommandType Application -ErrorAction SilentlyContinue) {
        foreach ($version in '3.14', '3.13', '3.12', '3.11') {
            $found = Find-Executable 'py' @("-$version")
            if ($found) { return $found }
        }
    }
    foreach ($name in 'python', 'python3') {
        foreach ($command in Get-Command $name -CommandType Application -All -ErrorAction SilentlyContinue) {
            # Les alias du Microsoft Store ne sont pas de vrais interpréteurs.
            if ($command.Source -like '*\WindowsApps\*') { continue }
            $found = Find-Executable $command.Source @()
            if ($found) { return $found }
        }
    }
    return $null
}

function Install-ManagedPython {
    $env:UV_PYTHON_INSTALL_DIR = Join-Path $Data 'python'
    $env:UV_CACHE_DIR = Join-Path $Data 'cache'
    $env:UV_PYTHON_PREFERENCE = 'only-managed'
    $uv = Join-Path $Data 'bin\uv.exe'
    if (-not (Test-Path $uv)) {
        Write-Host "Première utilisation : installation de uv $UvVersion (gestionnaire Python d'Astral)…"
        New-Item -ItemType Directory -Force (Join-Path $Data 'bin') | Out-Null
        $installer = Join-Path $Data 'uv-installer.ps1'
        try {
            [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
            Invoke-WebRequest -UseBasicParsing "https://astral.sh/uv/$UvVersion/install.ps1" -OutFile $installer `
                -ErrorAction Stop
        } catch {
            Fail "téléchargement de uv impossible. Vérifiez la connexion Internet. ($($_.Exception.Message))"
        }
        $env:UV_UNMANAGED_INSTALL = Join-Path $Data 'bin'
        & powershell -NoProfile -ExecutionPolicy Bypass -File $installer | Out-Host
        Remove-Item $installer -ErrorAction SilentlyContinue
        Remove-Item Env:UV_UNMANAGED_INSTALL
        if (-not (Test-Path $uv)) { Fail 'installation de uv impossible.' }
    }
    $python = Invoke-Quiet { & $uv python find $PythonVersion } | Select-Object -First 1
    if ($LASTEXITCODE -ne 0 -or -not $python) {
        Write-Host "Installation de Python $PythonVersion pour simp2puml (une seule fois)…"
        & $uv python install $PythonVersion | Out-Host  # Progression affichée directement (stderr non capturé).
        if ($LASTEXITCODE -ne 0) { Fail 'installation de Python impossible.' }
        $python = Invoke-Quiet { & $uv python find $PythonVersion } | Select-Object -First 1
    }
    if (-not $python) { Fail "Python $PythonVersion introuvable après installation." }
    $python = $python.Trim()
    if (-not (Find-Executable $python @())) { Fail 'le Python installé ne fournit pas Tkinter.' }
    return $python
}

function Set-Shortcut([string]$Path, [string]$Target) {
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($Path)
    $link.TargetPath = $Target
    $link.Arguments = "`"$Start`""
    $link.WorkingDirectory = $Root
    $link.IconLocation = "$Icon,0"
    $link.Description = 'simp2puml : convertir des diagrammes SIMP, PlantUML et Atelier Diagrammes'
    $link.Save()
}

New-Item -ItemType Directory -Force $Data | Out-Null
# Python mémorisé : pas de recherche (plusieurs lancements de Python) tant qu'il existe encore.
$Cache = Join-Path $Data 'python.txt'
$python = $null
if (-not $env:SIMP2PUML_FORCE_MANAGED -and (Test-Path $Cache)) {
    $cached = (Get-Content $Cache -TotalCount 1 -ErrorAction SilentlyContinue)
    if ($cached -and (Test-Path $cached.Trim())) { $python = $cached.Trim() }
}
if (-not $python) {
    $python = Find-Python
    if (-not $python) { $python = Install-ManagedPython }
    Set-Content -Path $Cache -Value $python -Encoding UTF8
}
# pythonw.exe ouvre l'interface sans fenêtre de console.
$pythonw = Join-Path (Split-Path -Parent $python) 'pythonw.exe'
if (-not (Test-Path $pythonw)) { $pythonw = $python }

if (-not $env:SIMP2PUML_NO_SHORTCUTS) { try {
    $menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'simp2puml.lnk'
    $first = -not (Test-Path $menu)
    Set-Shortcut $menu $pythonw
    if ($first) {
        Set-Shortcut (Join-Path ([Environment]::GetFolderPath('Desktop')) 'simp2puml.lnk') $pythonw
        Write-Host 'Raccourcis « simp2puml » créés dans le menu Démarrer et sur le Bureau.'
    }
} catch {
    Write-Host "(raccourcis non créés : $($_.Exception.Message))"
} }

$arguments = @("`"$Start`"") + ($args | ForEach-Object { "`"$_`"" })
# Sans redirection : l'interface ne doit hériter d'aucun handle de cette console, sinon elle la garde ouverte.
$env:SIMP2PUML_LOG = $Log
Remove-Item $Log -ErrorAction SilentlyContinue
$process = Start-Process -FilePath $pythonw -ArgumentList $arguments -WorkingDirectory $Root -PassThru
$null = $process.Handle  # Conserve le handle pour lire le code de sortie.
Start-Sleep -Seconds 2
if ($process.HasExited -and $process.ExitCode -ne 0) {
    Get-Content $Log -ErrorAction SilentlyContinue | Write-Host
    Fail "l'interface s'est arrêtée au démarrage (journal : $Log)."
}
Write-Host 'simp2puml est ouvert.'
