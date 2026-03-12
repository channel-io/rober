Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Step {
    param([string]$Message)

    Write-Host "==> $Message"
}

function New-BenchmarkEnvironment {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ScriptRoot,
        [Parameter(Mandatory = $true)]
        [string]$Configuration,
        [Parameter(Mandatory = $true)]
        [string]$ArtifactsFolder
    )

    $repoRoot = Split-Path -Parent $ScriptRoot

    [pscustomobject]@{
        RepoRoot        = $repoRoot
        BinaryPath      = Join-Path $repoRoot ("target\" + $Configuration + "\rover-probe.exe")
        ArtifactsRoot   = Join-Path $repoRoot ("target\" + $ArtifactsFolder)
        LogicalCpuCount = [Math]::Max(1, (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors)
    }
}

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [Parameter(ValueFromRemainingArguments = $true)]
        [string[]]$Arguments
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "command failed ($LASTEXITCODE): $FilePath $($Arguments -join ' ')"
    }
}

function Format-CommandArgument {
    param([string]$Value)

    if ($Value -match '[\s"]') {
        return '"' + ($Value -replace '"', '\"') + '"'
    }

    return $Value
}

function Ensure-ProbeBinary {
    param(
        [Parameter(Mandatory = $true)]
        [pscustomobject]$Environment,
        [switch]$SkipBuild
    )

    if ($SkipBuild -and (Test-Path $Environment.BinaryPath)) {
        return
    }

    Write-Step "building rover-probe"
    Push-Location $Environment.RepoRoot
    try {
        if ($Environment.BinaryPath -like '*\release\*') {
            Invoke-Checked cargo build --release -p rover-probe
        }
        else {
            Invoke-Checked cargo build -p rover-probe
        }
    }
    finally {
        Pop-Location
    }

    if (-not (Test-Path $Environment.BinaryPath)) {
        throw "compiled rover-probe binary was not found at $($Environment.BinaryPath)"
    }
}

function Get-DescendantProcessIds {
    param([int]$RootId)

    $seen = [System.Collections.Generic.HashSet[int]]::new()
    $queue = [System.Collections.Generic.Queue[int]]::new()
    $queue.Enqueue($RootId)

    while ($queue.Count -gt 0) {
        $current = $queue.Dequeue()
        if (-not $seen.Add($current)) {
            continue
        }

        $children = Get-CimInstance Win32_Process -Filter "ParentProcessId = $current" -ErrorAction SilentlyContinue
        foreach ($child in $children) {
            $queue.Enqueue([int]$child.ProcessId)
        }
    }

    return $seen
}

function Get-ProcessSnapshot {
    param([int]$RootId)

    $ids = Get-DescendantProcessIds -RootId $RootId
    $snapshot = @()

    foreach ($id in $ids) {
        $process = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($process) {
            $snapshot += $process
        }
    }

    return $snapshot
}

function Get-ProcessCpuSeconds {
    param($Process)

    if ($null -eq $Process) {
        return $null
    }

    $cpuProperty = $Process.PSObject.Properties["CPU"]
    if ($cpuProperty -and $null -ne $cpuProperty.Value) {
        return [double]$cpuProperty.Value
    }

    $totalProcessorTime = $Process.PSObject.Properties["TotalProcessorTime"]
    if ($totalProcessorTime -and $null -ne $totalProcessorTime.Value) {
        return [double]$totalProcessorTime.Value.TotalSeconds
    }

    return $null
}

function Resolve-ProcessExitCode {
    param(
        [AllowNull()]
        $ExitCode,
        [string]$Stdout,
        [string]$Stderr
    )

    if ($null -ne $ExitCode) {
        return [int]$ExitCode
    }

    if ($Stdout -match '(?m)^status:\s+success$') {
        return 0
    }

    if ($Stdout -match '(?m)^status:\s+not_implemented$') {
        return 2
    }

    if ($Stdout -match '(?m)^status:\s+error$') {
        return 1
    }

    if ($Stderr -match '(?m)^error\[') {
        return 1
    }

    return -1
}

function Read-LogContent {
    param([string]$Path)

    if (-not (Test-Path $Path)) {
        return ""
    }

    $content = Get-Content $Path -Raw
    if ($null -eq $content) {
        return ""
    }

    return [string]$content
}

function Measure-ProcessTreeUntilExit {
    param(
        [Parameter(Mandatory = $true)]
        [System.Diagnostics.Process]$Process,
        [Parameter(Mandatory = $true)]
        [int]$SampleIntervalMs,
        [Parameter(Mandatory = $true)]
        [int]$LogicalCpuCount
    )

    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $sampleCount = 0
    $peakWorkingSetMb = 0.0
    $peakPrivateMb = 0.0
    $peakCpuPercent = 0.0
    $peakProcessCount = 0
    $lastCpuByPid = @{}

    do {
        $Process.Refresh()
        $snapshot = @(Get-ProcessSnapshot -RootId $Process.Id)
        if ($snapshot.Count -eq 0) {
            $snapshot = @($Process)
        }

        $sampleCount += 1
        $workingSet = 0.0
        $privateBytes = 0.0
        $cpuDeltaSeconds = 0.0

        foreach ($item in $snapshot) {
            $workingSet += [double]$item.WorkingSet64
            $privateBytes += [double]$item.PrivateMemorySize64

            $cpuSeconds = Get-ProcessCpuSeconds -Process $item
            if ($null -ne $cpuSeconds) {
                if ($lastCpuByPid.ContainsKey($item.Id)) {
                    $delta = [double]$cpuSeconds - [double]$lastCpuByPid[$item.Id]
                    if ($delta -gt 0) {
                        $cpuDeltaSeconds += $delta
                    }
                }

                $lastCpuByPid[$item.Id] = [double]$cpuSeconds
            }
        }

        $peakWorkingSetMb = [Math]::Max($peakWorkingSetMb, $workingSet / 1MB)
        $peakPrivateMb = [Math]::Max($peakPrivateMb, $privateBytes / 1MB)
        $peakProcessCount = [Math]::Max($peakProcessCount, $snapshot.Count)

        if ($SampleIntervalMs -gt 0) {
            $cpuPercent = ($cpuDeltaSeconds / ($SampleIntervalMs / 1000.0) / $LogicalCpuCount) * 100.0
            $peakCpuPercent = [Math]::Max($peakCpuPercent, $cpuPercent)
        }

        if ($Process.HasExited) {
            break
        }

        Start-Sleep -Milliseconds $SampleIntervalMs
    } while ($true)

    $stopwatch.Stop()
    $Process.WaitForExit()
    $Process.Refresh()

    [pscustomobject]@{
        duration_ms         = [int][Math]::Round($stopwatch.Elapsed.TotalMilliseconds)
        peak_working_set_mb = [Math]::Round($peakWorkingSetMb, 2)
        peak_private_mb     = [Math]::Round($peakPrivateMb, 2)
        peak_cpu_percent    = [Math]::Round($peakCpuPercent, 2)
        peak_process_count  = $peakProcessCount
        sample_count        = $sampleCount
        raw_exit_code       = $Process.ExitCode
    }
}

function Invoke-MeasuredProcess {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [Parameter(Mandatory = $true)]
        [string[]]$Arguments,
        [Parameter(Mandatory = $true)]
        [string]$WorkingDirectory,
        [Parameter(Mandatory = $true)]
        [string]$StdoutPath,
        [Parameter(Mandatory = $true)]
        [string]$StderrPath,
        [Parameter(Mandatory = $true)]
        [int]$SampleIntervalMs,
        [Parameter(Mandatory = $true)]
        [int]$LogicalCpuCount,
        [hashtable]$Metadata = @{}
    )

    $argumentList = @($Arguments | ForEach-Object { Format-CommandArgument $_ })
    $process = Start-Process `
        -FilePath $FilePath `
        -ArgumentList $argumentList `
        -WorkingDirectory $WorkingDirectory `
        -NoNewWindow `
        -PassThru `
        -RedirectStandardOutput $StdoutPath `
        -RedirectStandardError $StderrPath

    $measurement = Measure-ProcessTreeUntilExit `
        -Process $process `
        -SampleIntervalMs $SampleIntervalMs `
        -LogicalCpuCount $LogicalCpuCount

    $stdout = Read-LogContent -Path $StdoutPath
    $stderr = Read-LogContent -Path $StderrPath
    $exitCode = Resolve-ProcessExitCode -ExitCode $measurement.raw_exit_code -Stdout $stdout -Stderr $stderr
    $stdoutPreview = ($stdout.Trim() -split "`r?`n" | Select-Object -First 4) -join " "
    $stderrPreview = ($stderr.Trim() -split "`r?`n" | Select-Object -First 4) -join " "

    $record = [ordered]@{}
    foreach ($key in $Metadata.Keys) {
        $record[$key] = $Metadata[$key]
    }

    $record["exit_code"] = $exitCode
    $record["success"] = ($exitCode -eq 0)
    $record["duration_ms"] = $measurement.duration_ms
    $record["peak_working_set_mb"] = $measurement.peak_working_set_mb
    $record["peak_private_mb"] = $measurement.peak_private_mb
    $record["peak_cpu_percent"] = $measurement.peak_cpu_percent
    $record["peak_process_count"] = $measurement.peak_process_count
    $record["sample_count"] = $measurement.sample_count
    $record["stdout_preview"] = $stdoutPreview
    $record["stderr_preview"] = $stderrPreview
    $record["stdout_path"] = $StdoutPath
    $record["stderr_path"] = $StderrPath

    return [pscustomobject]$record
}

function Save-BenchmarkResults {
    param(
        [Parameter(Mandatory = $true)]
        [object[]]$Results,
        [Parameter(Mandatory = $true)]
        [string]$OutputPath
    )

    $Results | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputPath -Encoding UTF8
}

function Show-BenchmarkSummary {
    param(
        [Parameter(Mandatory = $true)]
        [object[]]$Results,
        [string[]]$GroupBy = @("scenario", "step")
    )

    Write-Step "benchmark summary"

    if (@($Results).Count -eq 0) {
        Write-Host "No results"
        return
    }

    $summary = $Results |
        Group-Object $GroupBy |
        ForEach-Object {
            $group = $_.Group
            $row = [ordered]@{}
            foreach ($name in $GroupBy) {
                $row[$name] = $group[0].$name
            }

            $row["runs"] = @($group).Count
            $row["success_runs"] = @($group | Where-Object { $_.success }).Count
            $row["avg_duration_ms"] = [Math]::Round((($group | Measure-Object duration_ms -Average).Average), 2)
            $row["max_duration_ms"] = ($group | Measure-Object duration_ms -Maximum).Maximum
            $row["avg_peak_working_set_mb"] = [Math]::Round((($group | Measure-Object peak_working_set_mb -Average).Average), 2)
            $row["max_peak_working_set_mb"] = ($group | Measure-Object peak_working_set_mb -Maximum).Maximum
            $row["avg_peak_cpu_percent"] = [Math]::Round((($group | Measure-Object peak_cpu_percent -Average).Average), 2)
            $row["max_peak_cpu_percent"] = ($group | Measure-Object peak_cpu_percent -Maximum).Maximum

            [pscustomobject]$row
        }

    $summary | Sort-Object $GroupBy | Format-Table -AutoSize
}
