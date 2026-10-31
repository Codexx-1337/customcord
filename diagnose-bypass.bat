@echo off
setlocal
cd /d "%~dp0"
echo Checking Discord access. This may take up to two minutes.
start "" /wait "%~dp0customcord.exe" --diagnose-bypass
echo Please send bypass-diagnostics.txt and debug.log from this folder.
pause
