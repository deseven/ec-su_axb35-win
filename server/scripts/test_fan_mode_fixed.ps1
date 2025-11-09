#!/usr/bin/env pwsh

# PowerShell test script for EC server fan mode testing
# Similar to test_fan_mode_fixed.sh but using HTTP requests

param(
    [string]$ServerUrl = "http://127.0.0.1:8395",
    [int]$DelaySeconds = 5
)

# Initialize arrays to store RPM values and original settings
$fan1_rpms = @{}
$fan2_rpms = @{}
$fan3_rpms = @{}
$original_modes = @{}
$original_levels = @{}

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

# Function to make HTTP POST request
function Invoke-PostRequest {
    param(
        [string]$Endpoint,
        [hashtable]$Body
    )
    
    try {
        $jsonBody = $Body | ConvertTo-Json
        $response = Invoke-RestMethod -Uri "$ServerUrl/$Endpoint" -Method Post -Body $jsonBody -ContentType "application/json"
        return $response
    }
    catch {
        Write-Error "Failed to POST $Endpoint : $($_.Exception.Message)"
        return $null
    }
}

# Function to test server connectivity
function Test-ServerConnection {
    Write-Host "Testing server connection..." -ForegroundColor Yellow
    
    $status = Invoke-GetRequest "status"
    if ($null -eq $status) {
        Write-Error "Cannot connect to server at $ServerUrl. Make sure the server is running."
        exit 1
    }
    
    if ($status.status -eq 1) {
        Write-Host "Server is running (EC firmware version: $($status.version))" -ForegroundColor Green
    } else {
        Write-Error "Server is running but EC is not accessible"
        exit 1
    }
}

# Test server connection first
Test-ServerConnection

Write-Host ""
Write-Host "Saving original fan settings..." -ForegroundColor Cyan

# Save original fan settings
for ($i = 1; $i -le 3; $i++) {
    $mode_response = Invoke-GetRequest "fan$i/mode"
    $level_response = Invoke-GetRequest "fan$i/level"
    
    if ($null -ne $mode_response -and $null -ne $level_response) {
        $original_modes[$i] = $mode_response.mode
        $original_levels[$i] = $level_response.level
        Write-Host "Fan ${i}: mode=$($original_modes[$i]), level=$($original_levels[$i])"
    } else {
        Write-Error "Failed to get original settings for Fan $i"
        exit 1
    }
}

Write-Host "------------------------" -ForegroundColor Gray

Write-Host "Setting fans to fixed mode..." -ForegroundColor Cyan

# Set all fans to fixed mode
for ($i = 1; $i -le 3; $i++) {
    $result = Invoke-PostRequest "fan$i/mode" @{ mode = "fixed" }
    if ($null -eq $result) {
        Write-Error "Failed to set Fan $i to fixed mode"
        exit 1
    }
    Write-Host "Fan $i set to fixed mode" -ForegroundColor Green
}

Write-Host ""

# Test each fan level from 0 to 5
for ($level = 0; $level -le 5; $level++) {
    Write-Host "Setting fans to level $level..." -ForegroundColor Yellow
    
    # Set all fans to current level
    for ($i = 1; $i -le 3; $i++) {
        $result = Invoke-PostRequest "fan$i/level" @{ level = $level }
        if ($null -eq $result) {
            Write-Error "Failed to set Fan $i to level $level"
            exit 1
        }
    }
    
    Write-Host "Waiting $DelaySeconds seconds for fans to adjust..." -ForegroundColor Gray
    Start-Sleep -Seconds $DelaySeconds
    
    # Read RPM values
    for ($i = 1; $i -le 3; $i++) {
        $rpm_response = Invoke-GetRequest "fan$i/rpm"
        if ($null -ne $rpm_response) {
            switch ($i) {
                1 { $fan1_rpms[$level] = $rpm_response.rpm }
                2 { $fan2_rpms[$level] = $rpm_response.rpm }
                3 { $fan3_rpms[$level] = $rpm_response.rpm }
            }
            Write-Host "Fan $i RPM: $($rpm_response.rpm)"
        } else {
            Write-Error "Failed to get RPM for Fan $i"
            exit 1
        }
    }
    
    Write-Host "------------------------" -ForegroundColor Gray
}

Write-Host ""
Write-Host "Restoring original fan settings..." -ForegroundColor Cyan

# Restore original fan settings
for ($i = 1; $i -le 3; $i++) {
    # Restore mode
    $mode_result = Invoke-PostRequest "fan$i/mode" @{ mode = $original_modes[$i] }
    if ($null -eq $mode_result) {
        Write-Error "Failed to restore Fan $i mode"
        continue
    }
    
    # Restore level
    $level_result = Invoke-PostRequest "fan$i/level" @{ level = $original_levels[$i] }
    if ($null -eq $level_result) {
        Write-Error "Failed to restore Fan $i level"
        continue
    }
    
    Write-Host "Fan ${i} restored to: mode=$($original_modes[$i]), level=$($original_levels[$i])" -ForegroundColor Green
}

Write-Host ""

# Generate report
Write-Host "==========================================="
Write-Host "  Level    Fan 1    Fan 2    Fan 3  "
Write-Host "==========================================="

for ($level = 0; $level -le 5; $level++) {
    $fan1_rpm = if ($fan1_rpms.ContainsKey($level)) { $fan1_rpms[$level].ToString().PadLeft(4) } else { "----" }
    $fan2_rpm = if ($fan2_rpms.ContainsKey($level)) { $fan2_rpms[$level].ToString().PadLeft(4) } else { "----" }
    $fan3_rpm = if ($fan3_rpms.ContainsKey($level)) { $fan3_rpms[$level].ToString().PadLeft(4) } else { "----" }
    
    $line = "    $level      $fan1_rpm     $fan2_rpm     $fan3_rpm"
    Write-Host $line
    Write-Host "-----------------------------------------"
}

Write-Host ""
Write-Host "Fan RPM test completed." -ForegroundColor Green

# Pause for user input before closing
Write-Host ""
Read-Host "Press Enter to continue..."