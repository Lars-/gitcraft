# Makes the Windows installer of a release build, in the current folder.
# Usage: packaging/windows.ps1 <gitcraft.exe> <x86_64|aarch64> <version>
param([string]$Binary, [string]$Arch, [string]$Version)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$ProgressPreference = 'SilentlyContinue'
$binary = (Resolve-Path $Binary).Path
$out = (Get-Location).Path
$msi = @{ x86_64 = 'x64'; aarch64 = 'arm64' }[$Arch]
if (-not $msi) { throw "No installer for $Arch" }
$tmp = New-Item -ItemType Directory (Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName()))
Push-Location (Join-Path $PSScriptRoot ..)
try {
  # WiX 3.14.1, which the ARM runners do not have.
  Invoke-WebRequest https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip -OutFile "$tmp\wix.zip"
  if ((Get-FileHash "$tmp\wix.zip").Hash -ne '6ac824e1642d6f7277d0ed7ea09411a508f6116ba6fae0aa5f2c7daa2ff43d31') {
    throw 'The download of WiX is not the one expected'
  }
  Expand-Archive "$tmp\wix.zip" "$tmp\wix"
  & "$tmp\wix\candle.exe" -nologo -arch $msi "-dVersion=$Version" "-dBinary=$binary" -out "$tmp\gitcraft.wixobj" packaging\gitcraft.wxs
  & "$tmp\wix\light.exe" -nologo -spdb -out "$out\gitcraft-v$Version-$msi.msi" "$tmp\gitcraft.wixobj"
} finally {
  Pop-Location
  Remove-Item -Recurse -Force $tmp
}
