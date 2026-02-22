import sys

def patch_start_max():
    with open('scripts/start-max.ps1', 'r', encoding='utf-8') as f:
        content = f.read()

    # Apply unstaged changes
    content = content.replace(
        "Starts Kaizen core and Rust UI processes under a Windows Job Object.",
        "Starts Kaizen core and Mission Control UI processes under a Windows Job Object."
    )
    content = content.replace(
        'if ($name -eq "ui-dioxus.exe" -or $name -eq "kaizen-gateway.exe" -or $name -eq "zeroclaw-gateway.exe") {',
        r'''if (
            $name -eq "ui-dioxus.exe" -or
            $name -eq "kaizen-gateway.exe" -or
            $name -eq "zeroclaw-gateway.exe" -or
            $name -eq "kaizen max mission control.exe" -or
            $name -eq "kaizen_max_mission_control.exe"
        ) {'''
    )
    
    # Prompt changes: Add Trunk and ui-rust-native instead of ui-tauri-solid
    old_cargo = r'''        if ($name -eq "cargo.exe") {
            return ($cmd -like "*$repoRootLower*\core*") -or ($cmd -like "*$repoRootLower*\ui-dioxus*")
        }'''
    
    new_cargo = r'''        if ($name -eq "trunk.exe") {
            return ($cmd -like "*$repoRootLower*\ui-rust-native*")
        }

        if ($name -eq "cargo.exe") {
            return ($cmd -like "*$repoRootLower*\core*") -or ($cmd -like "*$repoRootLower*\ui-rust-native*")
        }

        if ($name -eq "node.exe") {
            return ($cmd -like "*$repoRootLower*\ui-rust-native*")
        }'''
    
    content = content.replace(old_cargo, new_cargo)

    content = content.replace(
        '$PSItem.Message -like "*ui-dioxus.exe*" -or',
        r'''$PSItem.Message -like "*ui-dioxus.exe*" -or
                $PSItem.Message -like "*kaizen max mission control.exe*" -or
                $PSItem.Message -like "*kaizen_max_mission_control.exe*" -or'''
    )

    content = content.replace(
        '"No recent Application Error/Windows Error Reporting events for ui-dioxus.exe in the last 10 minutes."',
        '"No recent Application Error/Windows Error Reporting events for Mission Control UI in the last 10 minutes."'
    )

    # Prompt changes: change uiDir to ui-rust-native
    content = content.replace(
        '$uiDir = Join-Path $repoRoot "ui-dioxus"',
        '$uiDir = Join-Path $repoRoot "ui-rust-native"'
    )

    content = content.replace(
        'Start-CommandProcess -Name "Kaizen Core" -WorkingDirectory $coreDir -Command "cargo run" -JobHandle $jobHandle',
        'Start-CommandProcess -Name "Kaizen Core" -WorkingDirectory $coreDir -Command "cargo run --bin kaizen-gateway" -JobHandle $jobHandle'
    )

    old_ui_exe = r'''        $uiExe = Join-Path $uiDir "target\release\ui-dioxus.exe"

        if (Test-Path $uiExe) {
            $uiProcess = Start-ExecutableProcess -Name "Kaizen MAX UI" -ExecutablePath $uiExe -WorkingDirectory $uiDir -JobHandle $jobHandle
        } else {
            Write-Host "[Kaizen MAX] Release Dioxus UI binary not found. Using cargo run." -ForegroundColor Yellow
            $uiProcess = Start-CommandProcess -Name "Kaizen MAX UI" -WorkingDirectory $uiDir -Command "cargo run" -JobHandle $jobHandle
        }'''

    # Prompt changes: fallback uses cargo tauri dev from ui-rust-native
    new_ui_exe = r'''        $uiExeCandidates = @(
            (Join-Path $uiDir "src-tauri\target\release\Kaizen MAX Mission Control.exe"),
            (Join-Path $uiDir "src-tauri\target\release\kaizen_max_mission_control.exe"),
            (Join-Path $uiDir "src-tauri\target\release\kaizen-mission-control.exe")
        )

        $uiExe = $uiExeCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1

        if ($uiExe) {
            $uiProcess = Start-ExecutableProcess -Name "Kaizen MAX UI" -ExecutablePath $uiExe -WorkingDirectory $uiDir -JobHandle $jobHandle
        } else {
            Write-Host "[Kaizen MAX] Release Mission Control binary not found. Using cargo tauri dev." -ForegroundColor Yellow
            $uiProcess = Start-CommandProcess -Name "Kaizen MAX UI" -WorkingDirectory $uiDir -Command "cargo tauri dev" -JobHandle $jobHandle
        }'''

    content = content.replace(old_ui_exe, new_ui_exe)

    with open('scripts/start-max.ps1', 'w', encoding='utf-8') as f:
        f.write(content)

def patch_validate_launch():
    with open('scripts/validate-launch.ps1', 'r', encoding='utf-8') as f:
        content = f.read()

    # 1. Update paths
    content = content.replace(
        '$uiDir = Join-Path $repoRoot "ui-dioxus"',
        '$uiDir = Join-Path $repoRoot "ui-rust-native"'
    )

    # 3. Modify fallback else branch
    old_else = r'''    } else {
        $coreProc = Start-Process -FilePath "cargo" -ArgumentList "run" -WorkingDirectory $coreDir -PassThru
        $uiProc = Start-Process -FilePath "cargo" -ArgumentList "run" -WorkingDirectory $uiDir -PassThru
        $startedProcesses.Add($coreProc)
        $startedProcesses.Add($uiProc)
    }'''
    
    new_else = r'''    } else {
        $coreProc = Start-Process -FilePath "cargo" -ArgumentList "run", "--bin", "kaizen-gateway" -WorkingDirectory $coreDir -PassThru
        $uiProc = Start-Process -FilePath "cargo" -ArgumentList "tauri", "dev" -WorkingDirectory $uiDir -PassThru
        $startedProcesses.Add($coreProc)
        $startedProcesses.Add($uiProc)
    }'''

    content = content.replace(old_else, new_else)

    # 4. Finally block
    content = content.replace(
        'Get-Process ui-dioxus -ErrorAction SilentlyContinue | Stop-Process -Force',
        r'''Get-Process "kaizen max mission control" -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-Process kaizen_max_mission_control -ErrorAction SilentlyContinue | Stop-Process -Force
    Get-Process trunk -ErrorAction SilentlyContinue | Stop-Process -Force'''
    )
    
    with open('scripts/validate-launch.ps1', 'w', encoding='utf-8') as f:
        f.write(content)

patch_start_max()
patch_validate_launch()
print("Patched successfully")
