@echo off
setlocal

set "SCRIPT_DIR=%~dp0"
powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%start-max.ps1" %*
set "EXIT_CODE=%ERRORLEVEL%"

endlocal & exit /b %EXIT_CODE%
