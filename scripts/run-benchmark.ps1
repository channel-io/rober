[CmdletBinding()]
param(
    [ValidateSet("debug", "release")]
    [string]$Configuration = "release",
    [int]$Iterations = 3,
    [int]$SampleIntervalMs = 200,
    [switch]$SkipBuild,
    [switch]$SkipFileScenario,
    [switch]$SkipBrowserScenario,
    [string]$OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Script:RepoRoot = Split-Path -Parent $PSScriptRoot
$Script:BinaryPath = Join-Path $Script:RepoRoot ("target\" + $Configuration + "\rover-probe.exe")
$Script:ArtifactsRoot = Join-Path $Script:RepoRoot "target\benchmarks"
$Script:LogicalCpuCount = [Math]::Max(1, (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors)

function Write-Step {
    param([string]$Message)
    Write-Host "==> $Message"
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

function Ensure-ProbeBinary {
    if ($SkipBuild -and (Test-Path $Script:BinaryPath)) {
        return
    }

    Write-Step "building rover-probe ($Configuration)"
    Push-Location $Script:RepoRoot
    try {
        if ($Configuration -eq "release") {
            Invoke-Checked cargo build --release -p rover-probe
        }
        else {
            Invoke-Checked cargo build -p rover-probe
        }
    }
    finally {
        Pop-Location
    }

    if (-not (Test-Path $Script:BinaryPath)) {
        throw "compiled rover-probe binary was not found at $($Script:BinaryPath)"
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

function Invoke-BenchStep {
    param(
        [string]$Scenario,
        [string]$StepName,
        [string[]]$Arguments,
        [string]$WorkingDirectory,
        [string]$LogRoot,
        [int]$Iteration,
        [int]$SampleInterval
    )

    $runId = "{0}-{1}-{2}" -f $Scenario, $StepName, $Iteration
    $stdoutPath = Join-Path $LogRoot "$runId.stdout.log"
    $stderrPath = Join-Path $LogRoot "$runId.stderr.log"

    $process = Start-Process `
        -FilePath $Script:BinaryPath `
        -ArgumentList $Arguments `
        -WorkingDirectory $WorkingDirectory `
        -NoNewWindow `
        -PassThru `
        -RedirectStandardOutput $stdoutPath `
        -RedirectStandardError $stderrPath

    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $sampleCount = 0
    $peakWorkingSetMb = 0.0
    $peakPrivateMb = 0.0
    $peakCpuPercent = 0.0
    $peakProcessCount = 0
    $lastCpuByPid = @{}

    while (-not $process.HasExited) {
        $snapshot = Get-ProcessSnapshot -RootId $process.Id
        $sampleCount += 1

        $workingSet = 0.0
        $privateBytes = 0.0
        $cpuDeltaSeconds = 0.0

        foreach ($item in $snapshot) {
            $workingSet += $item.WorkingSet64
            $privateBytes += $item.PrivateMemorySize64

            if ($null -ne $item.CPU) {
                if ($lastCpuByPid.ContainsKey($item.Id)) {
                    $delta = [double]$item.CPU - [double]$lastCpuByPid[$item.Id]
                    if ($delta -gt 0) {
                        $cpuDeltaSeconds += $delta
                    }
                }

                $lastCpuByPid[$item.Id] = [double]$item.CPU
            }
        }

        $peakWorkingSetMb = [Math]::Max($peakWorkingSetMb, $workingSet / 1MB)
        $peakPrivateMb = [Math]::Max($peakPrivateMb, $privateBytes / 1MB)
        $peakProcessCount = [Math]::Max($peakProcessCount, $snapshot.Count)

        if ($SampleInterval -gt 0) {
            $cpuPercent = ($cpuDeltaSeconds / ($SampleInterval / 1000.0) / $Script:LogicalCpuCount) * 100.0
            $peakCpuPercent = [Math]::Max($peakCpuPercent, $cpuPercent)
        }

        Start-Sleep -Milliseconds $SampleInterval
        $process.Refresh()
    }

    $stopwatch.Stop()

    $stdout = if (Test-Path $stdoutPath) { Get-Content $stdoutPath -Raw } else { "" }
    $stderr = if (Test-Path $stderrPath) { Get-Content $stderrPath -Raw } else { "" }
    $stdoutPreview = ($stdout.Trim() -split "`r?`n" | Select-Object -First 4) -join " "
    $stderrPreview = ($stderr.Trim() -split "`r?`n" | Select-Object -First 4) -join " "

    [pscustomobject]@{
        scenario             = $Scenario
        step                 = $StepName
        iteration            = $Iteration
        exit_code            = $process.ExitCode
        success              = ($process.ExitCode -eq 0)
        duration_ms          = [int][Math]::Round($stopwatch.Elapsed.TotalMilliseconds)
        peak_working_set_mb  = [Math]::Round($peakWorkingSetMb, 2)
        peak_private_mb      = [Math]::Round($peakPrivateMb, 2)
        peak_cpu_percent     = [Math]::Round($peakCpuPercent, 2)
        peak_process_count   = $peakProcessCount
        sample_count         = $sampleCount
        command              = ($Arguments -join " ")
        stdout_preview       = $stdoutPreview
        stderr_preview       = $stderrPreview
        stdout_path          = $stdoutPath
        stderr_path          = $stderrPath
    }
}

function New-BenchmarkContext {
    $runStamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $runRoot = Join-Path $Script:ArtifactsRoot $runStamp
    $fileRoot = Join-Path $runRoot "file"
    New-Item -ItemType Directory -Force -Path $runRoot, $fileRoot | Out-Null

    $seedFile = Join-Path $fileRoot "seed.txt"
    @(
        "Rover benchmark seed file",
        "Timestamp: $(Get-Date -Format o)",
        "This file exists to benchmark zeroclaw file operations."
    ) | Set-Content -Path $seedFile -Encoding UTF8

    $browserFixture = Join-Path $Script:RepoRoot "benchmarks\fixtures\browser\benchmark-form.html"
    $browserUri = [System.Uri]::new($browserFixture).AbsoluteUri

    return [pscustomobject]@{
        RunRoot            = $runRoot
        FileRoot           = $fileRoot
        SeedFile           = $seedFile
        CopyFile           = (Join-Path $fileRoot "seed-copy.txt")
        MovedFile          = (Join-Path $fileRoot "seed-moved.txt")
        BrowserFixturePath = $browserFixture
        BrowserFixtureUri  = $browserUri
    }
}

function Get-ScenarioSteps {
    param($Context)

    $steps = @()

    if (-not $SkipFileScenario) {
        $steps += [pscustomobject]@{
            Scenario = "file"
            Name = "stat-seed"
            Arguments = @("file", "stat", "--path", $Context.SeedFile)
        }
        $steps += [pscustomobject]@{
            Scenario = "file"
            Name = "copy-seed"
            Arguments = @("file", "copy", "--source", $Context.SeedFile, "--destination", $Context.CopyFile)
        }
        $steps += [pscustomobject]@{
            Scenario = "file"
            Name = "move-copy"
            Arguments = @("file", "move", "--source", $Context.CopyFile, "--destination", $Context.MovedFile)
        }
        $steps += [pscustomobject]@{
            Scenario = "file"
            Name = "delete-moved"
            Arguments = @("file", "delete", "--path", $Context.MovedFile)
        }
    }

    if (-not $SkipBrowserScenario) {
        $steps += [pscustomobject]@{
            Scenario = "browser"
            Name = "open-fixture"
            Arguments = @("browser", "open", "--url", $Context.BrowserFixtureUri)
        }
        $steps += [pscustomobject]@{
            Scenario = "browser"
            Name = "read-body"
            Arguments = @("browser", "read", "--target", "body")
        }
        $steps += [pscustomobject]@{
            Scenario = "browser"
            Name = "fill-name"
            Arguments = @("browser", "fill", "--target", "#name", "--value", "Rover benchmark")
        }
        $steps += [pscustomobject]@{
            Scenario = "browser"
            Name = "fill-notes"
            Arguments = @("browser", "fill", "--target", "#notes", "--value", "No LLM required")
        }
        $steps += [pscustomobject]@{
            Scenario = "browser"
            Name = "click-submit"
            Arguments = @("browser", "click", "--target", "#submit")
        }
        $steps += [pscustomobject]@{
            Scenario = "browser"
            Name = "read-result"
            Arguments = @("browser", "read", "--target", "#result")
        }
    }

    return $steps
}

function Show-Summary {
    param([object[]]$Results)

    $summary = $Results |
        Group-Object scenario, step |
        ForEach-Object {
            $group = $_.Group
            [pscustomobject]@{
                scenario = $group[0].scenario
                step = $group[0].step
                runs = $group.Count
                success_runs = ($group | Where-Object success).Count
                avg_duration_ms = [Math]::Round((($group | Measure-Object duration_ms -Average).Average), 2)
                max_duration_ms = ($group | Measure-Object duration_ms -Maximum).Maximum
                avg_peak_working_set_mb = [Math]::Round((($group | Measure-Object peak_working_set_mb -Average).Average), 2)
                max_peak_working_set_mb = ($group | Measure-Object peak_working_set_mb -Maximum).Maximum
                avg_peak_cpu_percent = [Math]::Round((($group | Measure-Object peak_cpu_percent -Average).Average), 2)
                max_peak_cpu_percent = ($group | Measure-Object peak_cpu_percent -Maximum).Maximum
            }
        }

    Write-Step "benchmark summary"
    $summary | Sort-Object scenario, step | Format-Table -AutoSize
}

Ensure-ProbeBinary

$context = New-BenchmarkContext
$results = @()
$steps = Get-ScenarioSteps -Context $context

if ($steps.Count -eq 0) {
    throw "no benchmark steps selected"
}

Write-Step "running benchmarks into $($context.RunRoot)"

for ($iteration = 1; $iteration -le $Iterations; $iteration++) {
    foreach ($step in $steps) {
        Write-Step ("{0} / {1} (iteration {2})" -f $step.Scenario, $step.Name, $iteration)
        $results += Invoke-BenchStep `
            -Scenario $step.Scenario `
            -StepName $step.Name `
            -Arguments $step.Arguments `
            -WorkingDirectory $Script:RepoRoot `
            -LogRoot $context.RunRoot `
            -Iteration $iteration `
            -SampleInterval $SampleIntervalMs
    }
}

if (-not $OutputPath) {
    $OutputPath = Join-Path $context.RunRoot "benchmark-results.json"
}

$results | ConvertTo-Json -Depth 5 | Set-Content -Path $OutputPath -Encoding UTF8
Show-Summary -Results $results
Write-Step "saved benchmark results to $OutputPath"
