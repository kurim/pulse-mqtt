#Requires -RunAsAdministrator
<#
.SYNOPSIS
Installs PulseMQTT-Rust so that it starts with admin rights at logon. Run `cargo build --release` first.
.EXAMPLE
.\install.ps1 -ReplaceOld     # install or update, and switch off the old PulseMQTT app
.\install.ps1 -Anonymous      # broker without login: everyone on the network can connect
.\install.ps1 -Uninstall
#>
param([switch]$ReplaceOld, [switch]$Anonymous, [switch]$Uninstall)
$ErrorActionPreference = 'Stop'

$name = 'PulseMQTT-Rust'
# The task runs elevated, so the program must live where only admins can change it.
$dir = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) $name
$exe = Join-Path $dir 'pulsemqtt.exe'
$loginFile = Join-Path $dir 'login.dat'

function Stop-Installed {
    if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) { Stop-ScheduledTask -TaskName $name }
    Get-Process pulsemqtt -ErrorAction SilentlyContinue | Where-Object Path -eq $exe | ForEach-Object {
        Stop-Process -InputObject $_ -Force
        $_.WaitForExit()
    }
}

if ($Uninstall) {
    Stop-Installed
    Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue
    Remove-Item $exe, $loginFile, (Join-Path $dir 'config.json') -ErrorAction SilentlyContinue
    if ((Test-Path $dir) -and -not (Get-ChildItem $dir -Force)) { Remove-Item $dir }
    "$name removed. An old PulseMQTT task that was switched off stays off: Enable-ScheduledTask -TaskName PulseMQTT"
    return
}

$source = Join-Path $PSScriptRoot 'target\release\pulsemqtt.exe'
if (-not (Test-Path $source)) { throw "$source not found. Run 'cargo build --release' first." }

# Ask for everything before changing anything.
$secret = $null
if (-not $Anonymous) {
    $keep = if (Test-Path $loginFile) { ' (Enter keeps the saved login)' } else { '' }
    $user = Read-Host "MQTT user$keep"
    if ($user) {
        $password = [Net.NetworkCredential]::new('', (Read-Host 'MQTT password' -AsSecureString)).Password
        if (-not $password) { throw 'The password must not be empty.' }
        Add-Type -AssemblyName System.Security
        $secret = [Security.Cryptography.ProtectedData]::Protect(
            [Text.Encoding]::UTF8.GetBytes("$user`n$password"), $null, 'CurrentUser')
    } elseif (-not $keep) {
        throw 'No user entered. Use -Anonymous for a broker without login.'
    }
}

Stop-Installed
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item $source $exe -Force
if ($Anonymous) { Remove-Item $loginFile -ErrorAction SilentlyContinue }
elseif ($secret) { [IO.File]::WriteAllBytes($loginFile, $secret) }

if ($ReplaceOld) {
    $old = Get-ScheduledTask -TaskName PulseMQTT -ErrorAction SilentlyContinue
    if ($old) { $old | Stop-ScheduledTask; $old | Disable-ScheduledTask | Out-Null }
    # Same process name as ours, so tell them apart by path.
    Get-Process PulseMQTT -ErrorAction SilentlyContinue | Where-Object Path -ne $exe | ForEach-Object {
        Stop-Process -InputObject $_ -Force
        $_.WaitForExit()
    }
}

$account = [Security.Principal.WindowsIdentity]::GetCurrent().Name
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $account
$trigger.Delay = 'PT15S' # give the GPU driver time to come up
Register-ScheduledTask -TaskName $name -Force -Trigger $trigger `
    -Action (New-ScheduledTaskAction -Execute $exe -Argument '--background' -WorkingDirectory $dir) `
    -Principal (New-ScheduledTaskPrincipal -UserId $account -LogonType Interactive -RunLevel Highest) `
    -Settings (New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) `
        -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
        -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)) | Out-Null

$config = Join-Path $dir 'config.json'
$mqtt = -not (Test-Path $config) -or (Get-Content $config -Raw | ConvertFrom-Json).mqtt_enabled -ne $false

Start-ScheduledTask -TaskName $name
foreach ($attempt in 1..10) {
    Start-Sleep 1
    $process = Get-Process pulsemqtt -ErrorAction SilentlyContinue | Where-Object Path -eq $exe
    if ($process -and (-not $mqtt -or
            (Get-NetTCPConnection -State Listen -OwningProcess $process.Id -ErrorAction SilentlyContinue))) {
        "$name is running and starts at every logon."
        return
    }
}
Write-Warning ("$name was installed but its broker is not listening. Is another program using port 1883 " +
    "(old PulseMQTT: run again with -ReplaceOld)? To see the error, run in this window: & '$exe'")
