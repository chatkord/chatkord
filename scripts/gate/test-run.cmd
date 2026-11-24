@rem The client's test runner on Windows (client/.cargo/config.toml): test-run.ps1 does the work.
@powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0test-run.ps1" %*
@exit /b %ERRORLEVEL%
