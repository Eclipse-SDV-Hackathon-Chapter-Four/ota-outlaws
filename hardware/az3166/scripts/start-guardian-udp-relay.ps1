#
# Contributors:
# Uttarkar Sopan - Feature enhancements and maintenance
# Microsoft Copilot - AI-assisted modifications
#

param(
    [int]$Port = 30502
)

$ErrorActionPreference = "Stop"

$wifiProfile = Get-NetConnectionProfile |
    Where-Object { $_.InterfaceAlias -match "Wi-Fi" -and $_.IPv4Connectivity -ne "Disconnected" } |
    Select-Object -First 1
if (!$wifiProfile) {
    Write-Error "No connected Wi-Fi profile found."
    exit 1
}

$wifiConfig = Get-NetIPConfiguration -InterfaceAlias $wifiProfile.InterfaceAlias
$wifiAddress = $wifiConfig.IPv4Address |
    Where-Object { $_.IPAddress -notlike "169.254.*" } |
    Select-Object -First 1 -ExpandProperty IPAddress
if (!$wifiAddress) {
    Write-Error "Could not find an IPv4 address on the connected Wi-Fi adapter."
    exit 1
}

$wslAddresses = (& wsl.exe -e hostname -I) -split '\s+' |
    Where-Object { $_ -match '^\d{1,3}(\.\d{1,3}){3}$' }
$wslAddress = $wslAddresses | Select-Object -First 1
if (!$wslAddress) {
    Write-Error "Could not determine the WSL IPv4 address. Is the WSL distribution running?"
    exit 1
}

$firewallRule = Get-NetFirewallRule -PolicyStore ActiveStore `
    -DisplayName "AZ3166 OTA Outlaws UDP 30502" -ErrorAction SilentlyContinue
if (!$firewallRule -or $firewallRule.Enabled -ne "True") {
    Write-Error "The inbound UDP firewall rule is missing or disabled. Add it from Administrator PowerShell first."
    exit 1
}

$listener = $null
$sender = $null
try {
    $bindAddress = [System.Net.IPAddress]::Parse($wifiAddress)
    $listener = [System.Net.Sockets.UdpClient]::new(
        [System.Net.IPEndPoint]::new($bindAddress, $Port)
    )
    $listener.Client.ReceiveTimeout = 100
    $sender = [System.Net.Sockets.UdpClient]::new()
    $target = [System.Net.IPEndPoint]::new(
        [System.Net.IPAddress]::Parse($wslAddress),
        $Port
    )
    $boardAddress = $null
    $responseTask = $sender.ReceiveAsync()

    Write-Host "Forwarding UDP $wifiAddress`:$Port -> WSL $wslAddress`:$Port"
    Write-Host "Campaign responses are forwarded back to the most recent board sender."
    Write-Host "Keep this PowerShell window open. Press Ctrl+C to stop the relay."
    while ($true) {
        try {
            $remote = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 0)
            $data = $listener.Receive([ref]$remote)
            $boardAddress = $remote
            [void]$sender.Send($data, $data.Length, $target)
            Write-Host ("Forwarded {0} bytes from {1}" -f $data.Length, $remote)
        }
        catch [System.Net.Sockets.SocketException] {
            if ($_.Exception.SocketErrorCode -ne
                [System.Net.Sockets.SocketError]::TimedOut) {
                throw
            }
        }

        if ($responseTask.IsCompleted) {
            $response = $responseTask.GetAwaiter().GetResult()
            if ($boardAddress) {
                [void]$listener.Send($response.Buffer, $response.Buffer.Length, $boardAddress)
                Write-Host ("Forwarded {0} response bytes to {1}" -f `
                    $response.Buffer.Length, $boardAddress)
            }
            $responseTask = $sender.ReceiveAsync()
        }
    }
}
finally {
    if ($listener) {
        $listener.Dispose()
    }
    if ($sender) {
        $sender.Dispose()
    }
}
