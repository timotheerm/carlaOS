# CarlaOS Windows helper.
#
# It does two things and decides nothing:
#   scan   reads what is installed and how Windows is set up.
#   apply  carries out the operations it is handed, and for each one reports the
#          state it found just before, so the change can be undone exactly.
#
# What may be changed is decided in crates/core/src/debloat. Keep this file
# compatible with Windows PowerShell 5.1, which is what every Windows 11 has.

param(
    [Parameter(Mandatory = $true)][ValidateSet('scan', 'apply')][string]$Mode,
    [Parameter(Mandatory = $true)][string]$Request,
    [Parameter(Mandatory = $true)][string]$Result
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Write-Answer($value) {
    $json = ConvertTo-Json -InputObject $value -Depth 8 -Compress
    [IO.File]::WriteAllText($Result, $json, (New-Object Text.UTF8Encoding($false)))
}

# ---------- registry ----------
# Paths look like HKCU\Software\... or HKLM\SOFTWARE\... and always mean the
# 64-bit view. The .NET API is used so names are never treated as wildcards.

function Open-Hive([string]$path) {
    $cut = $path.IndexOf('\')
    if ($cut -lt 1) { throw "Not a registry path: $path" }
    $name = $path.Substring(0, $cut)
    if ($name -eq 'HKCU') { $hive = [Microsoft.Win32.RegistryHive]::CurrentUser }
    elseif ($name -eq 'HKLM') { $hive = [Microsoft.Win32.RegistryHive]::LocalMachine }
    else { throw "Unsupported registry hive: $name" }
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($hive, [Microsoft.Win32.RegistryView]::Registry64)
    return @{ Base = $base; Sub = $path.Substring($cut + 1) }
}

function Test-ValueName($key, [string]$name) {
    foreach ($n in $key.GetValueNames()) {
        if ([string]::Equals($n, $name, [StringComparison]::OrdinalIgnoreCase)) { return $true }
    }
    return $false
}

# Returns @{ KeyExists; Value } where Value is $null or @{ kind; data }.
function Read-Value([string]$path, [string]$name) {
    $h = Open-Hive $path
    $key = $h.Base.OpenSubKey($h.Sub, $false)
    if ($null -eq $key) { return @{ KeyExists = $false; Value = $null } }
    try {
        if (-not (Test-ValueName $key $name)) { return @{ KeyExists = $true; Value = $null } }
        $kind = [string]$key.GetValueKind($name)
        $raw = $key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        if ($kind -eq 'DWord') {
            $value = @{ kind = 'dword'; data = [BitConverter]::ToUInt32([BitConverter]::GetBytes([int]$raw), 0) }
        }
        elseif ($kind -eq 'String') { $value = @{ kind = 'string'; data = [string]$raw } }
        elseif ($kind -eq 'ExpandString') { $value = @{ kind = 'expand_string'; data = [string]$raw } }
        else { $value = @{ kind = 'other' } }
        return @{ KeyExists = $true; Value = $value }
    }
    finally { $key.Close() }
}

function Write-Value([string]$path, [string]$name, $value) {
    $h = Open-Hive $path
    $key = $h.Base.CreateSubKey($h.Sub)
    try {
        $kind = [string]$value.kind
        if ($kind -eq 'dword') {
            $number = [BitConverter]::ToInt32([BitConverter]::GetBytes([uint32]$value.data), 0)
            $key.SetValue($name, $number, [Microsoft.Win32.RegistryValueKind]::DWord)
        }
        elseif ($kind -eq 'string') {
            $key.SetValue($name, [string]$value.data, [Microsoft.Win32.RegistryValueKind]::String)
        }
        elseif ($kind -eq 'expand_string') {
            $key.SetValue($name, [string]$value.data, [Microsoft.Win32.RegistryValueKind]::ExpandString)
        }
        else { throw "Unsupported value type: $kind" }
    }
    finally { $key.Close() }
}

function Remove-Value([string]$path, [string]$name, [bool]$dropEmptyKey) {
    $h = Open-Hive $path
    $key = $h.Base.OpenSubKey($h.Sub, $true)
    if ($null -eq $key) { return }
    try {
        $key.DeleteValue($name, $false)
        $empty = ($key.ValueCount -eq 0) -and ($key.SubKeyCount -eq 0)
    }
    finally { $key.Close() }
    if ($dropEmptyKey -and $empty) { $h.Base.DeleteSubKey($h.Sub, $false) }
}

# Returns the bytes of a binary value, or $null.
function Read-Bytes([string]$path, [string]$name) {
    $h = Open-Hive $path
    $key = $h.Base.OpenSubKey($h.Sub, $false)
    if ($null -eq $key) { return $null }
    try {
        if (-not (Test-ValueName $key $name)) { return $null }
        $raw = $key.GetValue($name)
        if ($raw -is [byte[]]) { return , $raw }
        return $null
    }
    finally { $key.Close() }
}

function Write-Bytes([string]$path, [string]$name, [byte[]]$bytes) {
    $h = Open-Hive $path
    $key = $h.Base.CreateSubKey($h.Sub)
    try { $key.SetValue($name, $bytes, [Microsoft.Win32.RegistryValueKind]::Binary) }
    finally { $key.Close() }
}

# ---------- scan ----------

# sc.exe's spelling of a service's start type, or $null for drivers and the like.
function Get-StartName($service) {
    $mode = [string]$service.StartMode
    if ($mode -eq 'Auto') {
        if ($service.DelayedAutoStart) { return 'delayed-auto' }
        return 'auto'
    }
    if ($mode -eq 'Manual') { return 'demand' }
    if ($mode -eq 'Disabled') { return 'disabled' }
    return $null
}

# Windows records "may this start with Windows" as a binary value whose first
# byte is even for yes and odd for no. No record means yes.
function Test-StartupEnabled([string]$approvedKey, [string]$name) {
    $bytes = Read-Bytes $approvedKey $name
    if ($null -eq $bytes -or $bytes.Length -lt 1) { return $true }
    return (($bytes[0] % 2) -eq 0)
}

function Get-Inventory($request) {
    $errors = New-Object System.Collections.ArrayList

    $packages = @()
    try {
        $packages = @(foreach ($p in @(Get-AppxPackage)) {
                [ordered]@{
                    name          = [string]$p.Name
                    framework     = [bool]$p.IsFramework
                    non_removable = [bool]$p.NonRemovable
                    signature     = [string]$p.SignatureKind
                }
            })
    }
    catch { [void]$errors.Add("Installed apps could not be read: $($_.Exception.Message)") }

    $services = @()
    try {
        $services = @(foreach ($s in @(Get-CimInstance -ClassName Win32_Service)) {
                $start = Get-StartName $s
                if ($null -ne $start) {
                    [ordered]@{
                        name         = [string]$s.Name
                        display_name = [string]$s.DisplayName
                        start        = $start
                        running      = ([string]$s.State -eq 'Running')
                    }
                }
            })
    }
    catch { [void]$errors.Add("Services could not be read: $($_.Exception.Message)") }

    $startup = @()
    try {
        $startup = @(foreach ($source in @($request.startup)) {
                if ($source.run) {
                    $h = Open-Hive ([string]$source.run)
                    $key = $h.Base.OpenSubKey($h.Sub, $false)
                    if ($null -ne $key) {
                        try {
                            foreach ($name in $key.GetValueNames()) {
                                if ($name -eq '') { continue }
                                [ordered]@{
                                    location = [string]$source.location
                                    name     = [string]$name
                                    command  = [string]$key.GetValue($name, '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                                    enabled  = (Test-StartupEnabled ([string]$source.approved) $name)
                                }
                            }
                        }
                        finally { $key.Close() }
                    }
                }
                elseif ($source.folder) {
                    $folder = [Environment]::GetFolderPath([string]$source.folder)
                    if ($folder -and (Test-Path -LiteralPath $folder)) {
                        foreach ($file in @(Get-ChildItem -LiteralPath $folder -File -Force)) {
                            if ($file.Name -eq 'desktop.ini') { continue }
                            [ordered]@{
                                location = [string]$source.location
                                name     = [string]$file.Name
                                command  = [string]$file.FullName
                                enabled  = (Test-StartupEnabled ([string]$source.approved) $file.Name)
                            }
                        }
                    }
                }
            })
    }
    catch { [void]$errors.Add("Start-up programs could not be read: $($_.Exception.Message)") }

    $settings = @()
    try {
        $settings = @(foreach ($s in @($request.settings)) {
                $found = Read-Value ([string]$s.path) ([string]$s.name)
                [ordered]@{ path = [string]$s.path; name = [string]$s.name; value = $found.Value }
            })
    }
    catch { [void]$errors.Add("Windows settings could not be read: $($_.Exception.Message)") }

    $programs = @()
    try {
        $roots = @(
            'HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
            'HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
            'HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall'
        )
        $programs = @(foreach ($root in $roots) {
                $h = Open-Hive $root
                $key = $h.Base.OpenSubKey($h.Sub, $false)
                if ($null -eq $key) { continue }
                try {
                    foreach ($subName in $key.GetSubKeyNames()) {
                        $sub = $key.OpenSubKey($subName, $false)
                        if ($null -eq $sub) { continue }
                        try {
                            $display = [string]$sub.GetValue('DisplayName', '')
                            $hidden = [string]$sub.GetValue('SystemComponent', '0')
                            if ($display -ne '' -and $hidden -ne '1') {
                                [ordered]@{ name = $display; publisher = [string]$sub.GetValue('Publisher', '') }
                            }
                        }
                        finally { $sub.Close() }
                    }
                }
                finally { $key.Close() }
            })
    }
    catch { [void]$errors.Add("Installed programs could not be read: $($_.Exception.Message)") }

    return [ordered]@{
        packages = $packages
        services = $services
        startup  = $startup
        settings = $settings
        programs = $programs
        errors   = @($errors)
    }
}

# ---------- apply ----------

function Test-Installed([string]$name) {
    return (@(Get-AppxPackage -Name $name).Count -gt 0)
}

# Each branch returns one hashtable: the state found before the change.
function Invoke-Op($op) {
    $kind = [string]$op.op

    if ($kind -eq 'appx_remove') {
        $name = [string]$op.name
        if ($name -notmatch '^[A-Za-z0-9._-]+$') { throw 'Not a package name.' }
        $found = @(Get-AppxPackage -Name $name)
        if ($found.Count -eq 0) { throw 'It is not installed.' }
        $manifest = $null
        if ($found[0].InstallLocation) { $manifest = Join-Path ([string]$found[0].InstallLocation) 'AppxManifest.xml' }
        foreach ($p in $found) { Remove-AppxPackage -Package $p.PackageFullName | Out-Null }
        return @{ manifest = $manifest }
    }

    if ($kind -eq 'appx_restore') {
        $name = [string]$op.name
        if ($name -notmatch '^[A-Za-z0-9._-]+$') { throw 'Not a package name.' }
        if (Test-Installed $name) { return @{} }
        # First choice: Windows usually keeps the files of a built-in app.
        if ($op.manifest -and (Test-Path -LiteralPath ([string]$op.manifest))) {
            try { Add-AppxPackage -Register ([string]$op.manifest) -DisableDevelopmentMode | Out-Null } catch { }
        }
        # Second choice: the Microsoft Store.
        if (-not (Test-Installed $name) -and $op.store_id) {
            $storeId = [string]$op.store_id
            if ($storeId -notmatch '^[A-Za-z0-9]+$') { throw 'Not a Store product id.' }
            $winget = Get-Command winget.exe -ErrorAction SilentlyContinue
            if ($null -eq $winget) { throw 'App Installer (winget) is missing, so it cannot be reinstalled automatically. Reinstall it from the Microsoft Store.' }
            & $winget.Source install --id $storeId --source msstore --accept-package-agreements --accept-source-agreements --disable-interactivity | Out-Null
        }
        if (-not (Test-Installed $name)) { throw 'Windows could not bring it back. Reinstall it from the Microsoft Store.' }
        return @{}
    }

    if ($kind -eq 'service_set') {
        $name = [string]$op.name
        if ($name -notmatch '^[A-Za-z0-9_. -]+$') { throw 'Not a service name.' }
        $start = [string]$op.start
        if (@('auto', 'delayed-auto', 'demand', 'disabled') -notcontains $start) { throw "Unknown start type: $start" }
        $service = Get-CimInstance -ClassName Win32_Service -Filter "Name='$name'"
        if ($null -eq $service) { throw 'This service does not exist here.' }
        $wasStart = Get-StartName $service
        if ($null -eq $wasStart) { throw 'This service has a start type CarlaOS cannot restore, so it was left alone.' }
        $wasRunning = ([string]$service.State -eq 'Running')
        & sc.exe config $name start= $start | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Windows refused to change it (sc.exe code $LASTEXITCODE)." }
        # Stopping or starting right now is a bonus; the start type is what lasts.
        if ($op.running -eq $false -and $wasRunning) { Stop-Service -Name $name -Force -ErrorAction SilentlyContinue | Out-Null }
        if ($op.running -eq $true -and -not $wasRunning) { Start-Service -Name $name -ErrorAction SilentlyContinue | Out-Null }
        return @{ start = $wasStart; running = $wasRunning }
    }

    if ($kind -eq 'startup_disable') {
        $was = Read-Bytes ([string]$op.key) ([string]$op.name)
        $data = $null
        if ($null -ne $was) { $data = [Convert]::ToBase64String($was) }
        # 03 00 00 00 followed by the time, exactly as Task Manager writes it.
        $bytes = [byte[]](@(3, 0, 0, 0) + [BitConverter]::GetBytes([DateTime]::UtcNow.ToFileTimeUtc()))
        Write-Bytes ([string]$op.key) ([string]$op.name) $bytes
        return @{ data = $data }
    }

    if ($kind -eq 'startup_restore') {
        if ($op.data) { Write-Bytes ([string]$op.key) ([string]$op.name) ([Convert]::FromBase64String([string]$op.data)) }
        else { Remove-Value ([string]$op.key) ([string]$op.name) $false }
        return @{}
    }

    if ($kind -eq 'reg_set') {
        $was = Read-Value ([string]$op.path) ([string]$op.name)
        if ($null -ne $was.Value -and $was.Value.kind -eq 'other') {
            throw 'The existing value has a type CarlaOS cannot restore, so it was left alone.'
        }
        if ($null -ne $op.value) { Write-Value ([string]$op.path) ([string]$op.name) $op.value }
        else { Remove-Value ([string]$op.path) ([string]$op.name) ([bool]$op.drop_empty_key) }
        return @{ value = $was.Value; key_existed = $was.KeyExists }
    }

    throw "Unknown operation: $kind"
}

# ---------- main ----------

try {
    $requestData = [IO.File]::ReadAllText($Request, [Text.Encoding]::UTF8) | ConvertFrom-Json
    if ($Mode -eq 'scan') {
        Write-Answer (Get-Inventory $requestData)
    }
    else {
        $results = @(foreach ($op in @($requestData.ops)) {
                try {
                    # Keep only the hashtable, whatever else a command printed.
                    $before = @(Invoke-Op $op) | Where-Object { $_ -is [hashtable] } | Select-Object -Last 1
                    if ($null -eq $before) { $before = @{} }
                    [ordered]@{ id = $op.id; ok = $true; before = $before }
                }
                catch {
                    [ordered]@{ id = $op.id; ok = $false; error = [string]$_.Exception.Message }
                }
            })
        Write-Answer ([ordered]@{ results = $results })
    }
    exit 0
}
catch {
    [Console]::Error.WriteLine([string]$_.Exception.Message)
    exit 1
}
