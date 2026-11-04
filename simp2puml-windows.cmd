@echo off
rem Double-cliquez pour lancer simp2puml. Installe Python au besoin, sans droits administrateur.
setlocal
rem Chemin rapide : Python deja trouve lors d'un lancement precedent, sans demarrer PowerShell.
set "SIMP2PUML_DATA=%LOCALAPPDATA%\simp2puml"
if not defined SIMP2PUML_FORCE_MANAGED if exist "%SIMP2PUML_DATA%\pythonw-cmd.txt" (
    set /p SIMP2PUML_PYW=<"%SIMP2PUML_DATA%\pythonw-cmd.txt"
)
if defined SIMP2PUML_PYW if exist "%SIMP2PUML_PYW%" (
    set "SIMP2PUML_LOG=%SIMP2PUML_DATA%\derniere-execution.log"
    start "" "%SIMP2PUML_PYW%" "%~dp0launchers\start.pyw" %*
    exit /b 0
)
rem Premier lancement (ou Python disparu) : recherche ou installation par PowerShell.
rem PSModulePath vide : Windows PowerShell retrouve ses propres modules (lancement depuis PowerShell 7).
set "PSModulePath="
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0launchers\bootstrap.ps1" %*
if errorlevel 1 (
    echo.
    pause
    exit /b 1
)
