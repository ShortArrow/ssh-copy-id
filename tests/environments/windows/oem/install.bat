@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "C:\OEM\setup-sshd.ps1" > "C:\OEM\setup-sshd.log" 2>&1
exit /b %ERRORLEVEL%
