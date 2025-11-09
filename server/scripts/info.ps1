#!/usr/bin/env pwsh

# PowerShell info script for EC server
# Displays current system information including CPU temperature, power mode, and fan details
# Equivalent to info.sh but using HTTP requests to the EC server

param(
    [string]$ServerUrl = "http://127.0.0.1:8395"
)

# Function to make HTTP GET request
function Invoke-GetRequest {
    param([string]$Endpoint)
    
    try {
        $response = Invoke-RestMethod -Uri "$ServerUrl/$Endpoint" -Method Get -ContentType "application/json"
        return $response
    }
    catch {
        Write-Error "Failed to GET $Endpoint : $($_.Exception.Message)"
        return $null
    }
}

# Function to test server connectivity
function Test-ServerConnection {
    $status = Invoke-GetRequest "status"
    if ($null -eq $status) {
        Write-Error "Cannot connect to server at $ServerUrl. Make sure the server is running."
        exit 1
    }
    
    if ($status.status -ne 1) {
        Write-Error "Server is running but EC is not accessible"
        exit 1
    }
    
    # Display server status at the top
    Write-Host "Server Status: $($status.status) | Version: $($status.version)"
    Write-Host ""
}

# Test server connection first
Test-ServerConnection

# Get APU power mode and temperature
$apuMode = Invoke-GetRequest "apu/power_mode"
$temp = Invoke-GetRequest "apu/temp"

if ($null -eq $apuMode -or $null -eq $temp) {
    Write-Error "Failed to get system information"
    exit 1
}

# Display header and system info
Write-Host "+--------------------------------------------------------------+"
$tempStr = "CPU-Temp: $($temp.temperature) C"
$modeStr = "Power mode: $($apuMode.power_mode)"
Write-Host ("| {0,-26} | {1,-31} |" -f $tempStr, $modeStr)

# Display fan table header
Write-Host ""
Write-Host "+-----+-------+-------+------+------------------------+------------------------+"
Write-Host ("| {0,-3} | {1,-5} | {2,-5} | {3,-4} | {4,-22} | {5,-22} |" -f "FAN", "MODE", "LEVEL", "RPM", "RAMPUP", "RAMPDOWN")

# Get and display fan information for fans 1, 2, and 3
for ($i = 1; $i -le 3; $i++) {
    $mode = Invoke-GetRequest "fan$i/mode"
    $level = Invoke-GetRequest "fan$i/level"
    $rpm = Invoke-GetRequest "fan$i/rpm"
    $rampup = Invoke-GetRequest "fan$i/rampup_curve"
    $rampdown = Invoke-GetRequest "fan$i/rampdown_curve"
    
    if ($null -eq $mode -or $null -eq $level -or $null -eq $rpm -or $null -eq $rampup -or $null -eq $rampdown) {
        Write-Error "Failed to get information for Fan $i"
        continue
    }
    
    $rampupStr = ($rampup.curve -join ", ")
    $rampdownStr = ($rampdown.curve -join ", ")
    Write-Host ("| {0,3} | {1,-5} | {2,5} | {3,4} | {4,-22} | {5,-22} |" -f $i, $mode.mode, $level.level, $rpm.rpm, $rampupStr, $rampdownStr)
}

Write-Host "+-----+-------+-------+------+------------------------+------------------------+"

# Pause for user input before closing
Write-Host ""
Read-Host "Press Enter to continue..."