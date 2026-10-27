@echo off
setlocal
cd /d "%~dp0"
opsearch-companion.exe %*
set "exit_code=%ERRORLEVEL%"
echo.
echo Companion exited with code %exit_code%. Press any key to close this window.
pause >nul
exit /b %exit_code%
