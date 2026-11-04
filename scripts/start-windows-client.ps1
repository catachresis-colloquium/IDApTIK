# SPDX-License-Identifier: AGPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
# Start one native client and return its exact PID through the caller's file.
# Looking up the first image-name match cannot distinguish two local seats.
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [Parameter(Mandatory = $true)][ValidateSet('host', 'join')][string]$Mode,
    [Parameter(Mandatory = $true)][string]$Target,
    [Parameter(Mandatory = $true)][string]$Url,
    [Parameter(Mandatory = $true)][string]$Session,
    [Parameter(Mandatory = $true)][ValidateSet('infiltrator', 'hacker')][string]$Role,
    [Parameter(Mandatory = $true)][string]$RunConfig,
    [Parameter(Mandatory = $true)][string]$PidFile
)

$ErrorActionPreference = 'Stop'

# Start-Process joins ArgumentList on Windows PowerShell 5.1. Quote each
# argument according to the native Windows command-line rules, including
# embedded quotes and trailing backslashes, before that join.
function Quote-NativeArgument([string]$Value) {
    $escaped = [regex]::Replace($Value, '(\\*)"', '$1$1\"')
    $escaped = [regex]::Replace($escaped, '(\\+)$', '$1$1')
    return '"' + $escaped + '"'
}

$clientArguments = @("--$Mode")
if ($Mode -eq 'join') { $clientArguments += $Target }
$clientArguments += @('--url', $Url, '--session', $Session, '--role', $Role, '--script', $RunConfig)
$argumentLine = ($clientArguments | ForEach-Object { Quote-NativeArgument $_ }) -join ' '
$client = Start-Process -FilePath $Executable -ArgumentList $argumentLine -NoNewWindow -PassThru
try {
    [System.IO.File]::WriteAllText($PidFile, [string]$client.Id, [System.Text.Encoding]::ASCII)
} catch {
    # Failure to record ownership must not leave an untracked game window.
    Stop-Process -Id $client.Id -ErrorAction SilentlyContinue
    throw
}
