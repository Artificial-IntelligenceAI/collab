# collab — installer.
#
# Deliberately a readable script rather than a compiled setup program. A
# compiled one would have meant shipping a second copy of the .NET runtime,
# roughly 130 MB, to do work that amounts to copying a folder and making two
# shortcuts. This is also inspectable by whoever runs it, which a setup.exe
# handed over the internet is not.
#
# Installs per-user, so it never asks for an administrator.
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$dest = Join-Path $env:LOCALAPPDATA 'Programs\Collab'

Write-Host ''
Write-Host '  collab' -ForegroundColor Cyan
Write-Host '  messages between two machines while you work on the same thing'
Write-Host ''

if (-not (Test-Path (Join-Path $here 'Collab.exe'))) {
  Write-Host '  Collab.exe is not next to this script.' -ForegroundColor Red
  Write-Host '  Extract the whole zip first, then run Install.cmd from inside it.'
  Read-Host '  Press Enter to close'
  exit 1
}

# A running copy holds its own file open, so stop it before replacing anything.
Get-Process Collab, collab -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 700

Write-Host "  installing to $dest"
New-Item -ItemType Directory -Force -Path $dest, (Join-Path $dest 'bin') | Out-Null
Copy-Item (Join-Path $here 'Collab.exe')     $dest -Force
Copy-Item (Join-Path $here 'bin\collab.exe') (Join-Path $dest 'bin') -Force
if (Test-Path (Join-Path $here 'collab.png')) { Copy-Item (Join-Path $here 'collab.png') $dest -Force }

# Shortcuts. The app makes its own Start Menu entry on first run because
# Windows will not attribute a toast to an unregistered program, but making it
# here too means it is there before the app has ever been opened.
$shell = New-Object -ComObject WScript.Shell
foreach ($dir in @(
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'),
    [Environment]::GetFolderPath('Desktop'))) {
  $lnk = $shell.CreateShortcut((Join-Path $dir 'collab.lnk'))
  $lnk.TargetPath = Join-Path $dest 'Collab.exe'
  $lnk.WorkingDirectory = $dest
  $lnk.Description = 'collab — messages from the other machine'
  $lnk.Save()
}
Write-Host '  shortcuts added to the Start Menu and the Desktop'

# So it appears in Installed Apps like anything else, and can be removed there.
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Collab'
New-Item -Path $key -Force | Out-Null
Set-ItemProperty $key DisplayName     'collab'
Set-ItemProperty $key DisplayIcon     (Join-Path $dest 'Collab.exe')
Set-ItemProperty $key InstallLocation $dest
Set-ItemProperty $key Publisher       'Tankun Sriket'
Set-ItemProperty $key NoModify        1 -Type DWord
Set-ItemProperty $key NoRepair        1 -Type DWord
Set-ItemProperty $key UninstallString ("powershell -ExecutionPolicy Bypass -File `"" + (Join-Path $dest 'Uninstall.ps1') + "`"")
Copy-Item (Join-Path $here 'Uninstall.ps1') $dest -Force

Write-Host ''
Write-Host '  done.' -ForegroundColor Green
Write-Host '  Next: open collab, press the # button, and paste the invite the other person sent you.'
Write-Host ''
Start-Process (Join-Path $dest 'Collab.exe')
Start-Sleep -Seconds 2
