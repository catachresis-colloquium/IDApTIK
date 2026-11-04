# SPDX-License-Identifier: AGPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
# Executes the real PowerShell helper with a simulated native process API.
# This checks arguments and exact PID ownership; it is not a Windows GUI test.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$helper = Join-Path $PSScriptRoot '../scripts/start-windows-client.ps1'
$fixture = Join-Path ([System.IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
[System.IO.Directory]::CreateDirectory($fixture) | Out-Null
$global:IdaptikWindowsFixture = @{ NextId = 4100; Starts = @(); Stops = @(); FailLaunch = $false }

function Assert-Equal($Expected, $Actual, [string]$Message) {
    if ($Expected -cne $Actual) { throw "$Message; expected [$Expected], got [$Actual]" }
}

function Start-Process {
    [CmdletBinding()]
    param([string]$FilePath, [string]$ArgumentList, [switch]$NoNewWindow, [switch]$PassThru)
    if ($global:IdaptikWindowsFixture.FailLaunch) { throw 'fixture native launch failed' }
    Assert-Equal $true ([bool]$PassThru) 'the launcher must obtain the exact process object'
    Assert-Equal $true ([bool]$NoNewWindow) 'the launcher must retain the current log/console'
    $global:IdaptikWindowsFixture.NextId += 1
    $global:IdaptikWindowsFixture.Starts += @{ File = $FilePath; Arguments = $ArgumentList }
    return [pscustomobject]@{ Id = $global:IdaptikWindowsFixture.NextId }
}

function Stop-Process {
    [CmdletBinding()]
    param([int]$Id)
    $global:IdaptikWindowsFixture.Stops += $Id
}

try {
    $probe = & $helper -PolicyCheck
    Assert-Equal 'IDAPTIK_WINDOWS_HELPER_ALLOWED' $probe 'the actual helper policy probe returns its marker'
    Assert-Equal 0 $global:IdaptikWindowsFixture.Starts.Count 'the policy probe starts no client'
    Assert-Equal 0 $global:IdaptikWindowsFixture.Stops.Count 'the policy probe stops no client'
    Assert-Equal 0 @(Get-ChildItem -LiteralPath $fixture).Count 'the policy probe writes no ownership record'

    $options = @{
        Executable = 'C:\checkout with spaces\idaptik-bevy.exe'
        Mode = 'host'
        Target = '127.0.0.1'
        Url = 'ws://127.0.0.1:4123/voice/websocket'
        Session = 'ghost-lobby'
        Role = 'infiltrator'
        RunConfig = 'C:\script with "quotes".json'
        PidFile = (Join-Path $fixture 'host.pid')
    }
    & $helper @options
    Assert-Equal '4101' ([System.IO.File]::ReadAllText($options.PidFile)) 'record the newly created host PID'
    Assert-Equal $options.Executable $global:IdaptikWindowsFixture.Starts[0].File 'preserve the executable path'
    $expected = '"--host" "--url" "ws://127.0.0.1:4123/voice/websocket" "--session" "ghost-lobby" "--role" "infiltrator" "--script" "C:\script with \"quotes\".json"'
    Assert-Equal $expected $global:IdaptikWindowsFixture.Starts[0].Arguments 'quote native arguments including embedded quotes'

    $options.Mode = 'join'
    $options.Target = '127.0.0.1:4123'
    $options.Role = 'hacker'
    $options.RunConfig = 'C:\folder with spaces\'
    $options.PidFile = Join-Path $fixture 'join.pid'
    & $helper @options
    Assert-Equal '4102' ([System.IO.File]::ReadAllText($options.PidFile)) 'the second local client owns a different PID'
    Assert-Equal '4101' ([System.IO.File]::ReadAllText((Join-Path $fixture 'host.pid'))) 'joining must preserve host ownership'
    $expected = '"--join" "127.0.0.1:4123" "--url" "ws://127.0.0.1:4123/voice/websocket" "--session" "ghost-lobby" "--role" "hacker" "--script" "C:\folder with spaces\\"'
    Assert-Equal $expected $global:IdaptikWindowsFixture.Starts[1].Arguments 'preserve join target and a trailing backslash'
    Assert-Equal 0 $global:IdaptikWindowsFixture.Stops.Count 'starting another seat must not stop the host'

    $options.PidFile = Join-Path $fixture 'missing-directory/client.pid'
    $failed = $false
    try { & $helper @options } catch { $failed = $true }
    Assert-Equal $true $failed 'failure to persist ownership must be visible'
    Assert-Equal 1 $global:IdaptikWindowsFixture.Stops.Count 'clean up exactly the unrecorded client'
    Assert-Equal 4103 $global:IdaptikWindowsFixture.Stops[0] 'cleanup must not select an earlier image-name match'

    $options.PidFile = Join-Path $fixture 'failed-launch.pid'
    $global:IdaptikWindowsFixture.FailLaunch = $true
    $failed = $false
    try { & $helper @options } catch { $failed = $true }
    Assert-Equal $true $failed 'a failed native launch must fail the helper'
    Assert-Equal $false ([System.IO.File]::Exists($options.PidFile)) 'no ownership record for a process that did not start'
    Assert-Equal 1 $global:IdaptikWindowsFixture.Stops.Count 'a failed launch must not kill another seat'
    Write-Output 'Windows client contract fixtures: PASS (native process API simulated)'
} finally {
    Remove-Item -LiteralPath $fixture -Recurse -Force
    Remove-Variable -Scope Global -Name IdaptikWindowsFixture
}
