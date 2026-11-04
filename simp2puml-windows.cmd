@echo off
rem Double-cliquez pour lancer simp2puml. Installe Python au besoin, sans droits administrateur.
rem PSModulePath vide : Windows PowerShell retrouve ses propres modules (lancement depuis PowerShell 7).
set "PSModulePath="
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0launchers\bootstrap.ps1" %*
if errorlevel 1 (
    echo.
    pause
    exit /b 1
)
