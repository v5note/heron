@echo off
rem cmd.exe / Explorer entry point for scripts\dev.ps1 (the Windows mirror of scripts/dev.sh).
rem It bypasses the default execution policy so no setup is needed:
rem   scripts\dev.cmd dev
rem   scripts\dev.cmd check
rem   scripts\dev.cmd cli scan

setlocal
set "PS=powershell"
where pwsh >nul 2>nul && set "PS=pwsh"
"%PS%" -NoProfile -ExecutionPolicy Bypass -File "%~dp0dev.ps1" %*
endlocal & exit /b %ERRORLEVEL%
