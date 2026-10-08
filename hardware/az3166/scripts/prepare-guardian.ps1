#
# Contributors:
# Uttarkar Sopan - Feature enhancements and maintenance
# Microsoft Copilot - AI-assisted modifications
#

param(
    [string]$HostAddress
)

$ErrorActionPreference = "Stop"
$AZ3166_DIR = Resolve-Path "$PSScriptRoot/.."
$CONFIG_FILE = Join-Path $AZ3166_DIR "app\guardian\cloud_config.local.h"
$BUILD_DIR = Join-Path $AZ3166_DIR "build\guardian"

if (!(Test-Path $CONFIG_FILE)) {
    Write-Error "Missing $CONFIG_FILE. Add the board Wi-Fi SSID/password first."
    exit 1
}

if (!$HostAddress) {
    $wifiProfile = Get-NetConnectionProfile |
        Where-Object { $_.InterfaceAlias -match "Wi-Fi" -and $_.IPv4Connectivity -ne "Disconnected" } |
        Select-Object -First 1
    if (!$wifiProfile) {
        Write-Error "No connected Wi-Fi profile found. Join the same Wi-Fi as the AZ3166 and retry."
        exit 1
    }

    $wifiConfig = Get-NetIPConfiguration -InterfaceAlias $wifiProfile.InterfaceAlias
    $HostAddress = $wifiConfig.IPv4Address |
        Where-Object { $_.IPAddress -notlike "169.254.*" } |
        Select-Object -First 1 -ExpandProperty IPAddress
}

$parsedAddress = $null
if (![System.Net.IPAddress]::TryParse($HostAddress, [ref]$parsedAddress) -or
    $parsedAddress.AddressFamily -ne [System.Net.Sockets.AddressFamily]::InterNetwork) {
    Write-Error "HostAddress must be a valid IPv4 address."
    exit 1
}

$addressBytes = $parsedAddress.GetAddressBytes()
$addressMacro = "#define GUARDIAN_BRIDGE_IP IP_ADDRESS({0}, {1}, {2}, {3})" -f `
    $addressBytes[0], $addressBytes[1], $addressBytes[2], $addressBytes[3]
$configContents = [System.IO.File]::ReadAllText($CONFIG_FILE)
$pattern = '(?m)^\s*#define\s+GUARDIAN_BRIDGE_IP\s+.*$'
$matches = [regex]::Matches($configContents, $pattern)
if ($matches.Count -ne 1) {
    Write-Error "Expected exactly one GUARDIAN_BRIDGE_IP definition in cloud_config.local.h."
    exit 1
}
$configContents = [regex]::Replace($configContents, $pattern, $addressMacro)
[System.IO.File]::WriteAllText(
    $CONFIG_FILE,
    $configContents,
    [System.Text.UTF8Encoding]::new($false)
)

$armBin = "D:\Programme\arm\GNU Toolchain mingw-w64-i686-arm-none-eabi\bin"
$cmakeBin = "D:\Programme\CMake\cmake-4.0.2-windows-x86_64\bin"
$ninjaBin = "D:\Programme\Ninja\ninja-win_v1.12.1"
foreach ($toolBin in @($armBin, $cmakeBin, $ninjaBin)) {
    if (!(Test-Path $toolBin)) {
        Write-Error "Required build-tool directory does not exist: $toolBin"
        exit 1
    }
}
$env:PATH = "$armBin;$cmakeBin;$ninjaBin;$env:PATH"

Write-Host "Using host Wi-Fi IPv4 $HostAddress for the AZ3166 adapter."
Write-Host "Reconfiguring and building the Guardian firmware..."
& (Join-Path $cmakeBin "cmake.exe") -S $AZ3166_DIR -B $BUILD_DIR -G Ninja `
    "-DCMAKE_BUILD_TYPE=Release" `
    "-DAPP_CONFIG=guardian" `
    "-DCMAKE_POLICY_VERSION_MINIMUM=3.5" `
    "-DCMAKE_TOOLCHAIN_FILE=$AZ3166_DIR/cmake/arm-gcc-cortex-m4.cmake"
if ($LASTEXITCODE -ne 0) {
    throw "Guardian CMake configuration failed with exit code $LASTEXITCODE."
}

& (Join-Path $cmakeBin "cmake.exe") --build $BUILD_DIR --parallel 4
if ($LASTEXITCODE -ne 0) {
    throw "Guardian firmware build failed with exit code $LASTEXITCODE."
}

$firmware = Join-Path $BUILD_DIR "app\mxchip_threadx.bin"
if (!(Test-Path $firmware)) {
    Write-Error "Build completed but firmware image was not found at $firmware."
    exit 1
}

Write-Host "[OK] Firmware ready at $firmware"
