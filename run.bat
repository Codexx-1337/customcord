@echo off
setlocal
cd /d "%~dp0"

if exist "customcord.exe" (
    start "" "customcord.exe"
) else if exist "target\release\customcord.exe" (
    start "" "target\release\customcord.exe"
) else if exist "target\debug\customcord.exe" (
    start "" "target\debug\customcord.exe"
) else (
    echo [Customcord] Building...
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
    cargo run --release
)
