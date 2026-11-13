# Install what package.mjs made on this machine, check it landed, and take the
# desktop host off again. Run by the Windows build on a throwaway machine; it
# is safe on a real one too, but it does install CarlaOS.
#
#   pwsh smoke-test.ps1 -Version 0.5.0 -Dir <folder with the installers> [-NoHost]
param(
    [Parameter(Mandatory)] [string] $Version,
    [Parameter(Mandatory)] [string] $Dir,
    [switch] $NoHost
)
$ErrorActionPreference = 'Stop'

$uninstall = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$runName = 'CarlaOS Companion'

function Invoke-Quietly([string] $File, [string[]] $Arguments) {
    Write-Host "> $File $Arguments"
    $process = Start-Process -FilePath $File -ArgumentList $Arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "$File ended with code $($process.ExitCode)." }
}

function Assert-Installed([string] $Name) {
    $entry = Get-ItemProperty -Path "$uninstall\$Name" -ErrorAction SilentlyContinue
    if (-not $entry) { throw "Windows does not list `"$Name`" as installed." }
    if ($entry.DisplayVersion -ne $Version) { throw "`"$Name`" is listed as $($entry.DisplayVersion), expected $Version." }
    Write-Host "ok: $Name $Version is installed"
    $entry
}

function Assert-AllyInstalled {
    $ally = Assert-Installed 'CarlaOS'
    # The full installer opens CarlaOS through this value, so it has to be the program.
    $program = "$($ally.DisplayIcon)".Trim('"')
    if (-not (Test-Path $program)) { throw "The Ally interface is not where Windows says it is: $program" }
    Write-Host "ok: the interface is at $program"
}

if ($NoHost) {
    Invoke-Quietly (Join-Path $Dir "CarlaOS-Ally-$Version-setup.exe") '/S'
    Assert-AllyInstalled
    return
}

# 1. The full installer puts both halves in place.
Invoke-Quietly (Join-Path $Dir "CarlaOS-Full-$Version-setup.exe") '/S'
Assert-AllyInstalled
$hostEntry = Assert-Installed 'CarlaOS Desktop Host'
$folder = $hostEntry.InstallLocation
$companion = Join-Path $folder 'carla-companion.exe'
$info = & $companion info | ConvertFrom-Json
if ($info.version -ne $Version) { throw "The companion says it is $($info.version), expected $Version." }
Write-Host "ok: the companion runs and says $Version"
if (Get-ItemProperty -Path $runKey -Name $runName -ErrorAction SilentlyContinue) {
    throw 'A fresh install must not start anything with Windows before set-up.'
}

# 2. Updating a desktop that was set up from somewhere else moves its start-up
#    entry to the installed copy.
Set-ItemProperty -Path $runKey -Name $runName -Value 'conhost.exe --headless "C:\elsewhere\carla-companion.exe" serve'
Invoke-Quietly (Join-Path $Dir "CarlaOS-Desktop-Host-$Version-setup.exe") '/S'
$startup = (Get-ItemProperty -Path $runKey -Name $runName).$runName
if ($startup -ne "conhost.exe --headless `"$companion`" serve") { throw "Start-up entry after an update: $startup" }
Write-Host 'ok: an update keeps the companion starting with Windows, from the installed copy'

# 3. Uninstalling removes the program and the start-up entry.
#    "_?=" makes the uninstaller wait instead of handing over to a copy.
Invoke-Quietly (Join-Path $folder 'uninstall.exe') '/S', "_?=$folder"
if (Test-Path $companion) { throw 'The companion is still there after uninstalling.' }
if (Test-Path "$uninstall\CarlaOS Desktop Host") { throw 'Windows still lists the desktop host after uninstalling.' }
if (Get-ItemProperty -Path $runKey -Name $runName -ErrorAction SilentlyContinue) { throw 'The start-up entry is still there after uninstalling.' }
Write-Host 'ok: the desktop host uninstalls cleanly'
