# Generate the real provider master file.
#
# Does two things:
# 1. Copy DEEPSEEK_API_KEY from the user-scope env into the current process
# 2. Run migrate-to-master.mjs
#
# Why this wrapper: Git Bash does not inherit user-scope environment variables,
# so Node would miss the deepseek entry without it.
#
# NOTE: this file is ASCII-only on purpose. Windows PowerShell reads .ps1 as
# ANSI unless there is a BOM, which mangles Chinese characters. Keep it ASCII.
#
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File scripts/migrate.ps1

$ErrorActionPreference = 'Stop'

$key = [Environment]::GetEnvironmentVariable('DEEPSEEK_API_KEY', 'User')
if ($key) {
    $env:DEEPSEEK_API_KEY = $key
    Write-Output "DEEPSEEK_API_KEY loaded into process (length $($key.Length))"
} else {
    Write-Output "WARNING: user env does not have DEEPSEEK_API_KEY"
}

Set-Location (Split-Path -Parent $PSScriptRoot)
node scripts/migrate-to-master.mjs
