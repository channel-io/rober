[CmdletBinding()]
param(
    [ValidateSet("single", "parallel", "all")]
    [string]$Suite = "all",
    [ValidateSet("easy", "medium", "hard")]
    [string]$Difficulty = "easy",
    [ValidateSet("debug", "release")]
    [string]$Configuration = "release",
    [int]$SampleIntervalMs = 50,
    [switch]$SkipBuild,
    [switch]$SkipFileScenario,
    [switch]$SkipBrowserScenario,
    [string]$OutputPath,
    [switch]$ControllerMode,
    [string]$ScenarioPath,
    [switch]$WorkerMode,
    [string]$WorkerConfigPath,
    [switch]$MemoryHogMode,
    [string]$MemoryHogConfigPath
)

. (Join-Path $PSScriptRoot "benchmark-common.ps1")

function Save-JsonFile {
    param(
        [Parameter(Mandatory = $true)]
        $Value,
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $Value | ConvertTo-Json -Depth 8 | Set-Content -Path $Path -Encoding UTF8
}

function Load-JsonFile {
    param([string]$Path)

    Get-Content $Path -Raw | ConvertFrom-Json
}

function Get-PowerShellExecutable {
    $command = Get-Command powershell.exe -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    return "powershell"
}

function Get-StressProfile {
    param([string]$Difficulty)

    switch ($Difficulty) {
        "easy" {
            return [pscustomobject]@{
                FilePayloadBytes    = 1MB
                BrowserPayloadBytes = 256
                SingleDurationMs    = 15000
                ParallelDurationMs  = 20000
                WorkerCount         = 2
                MemoryPressureMb    = 256
                IncludeList         = $false
                UseDeepPaths        = $false
            }
        }
        "medium" {
            return [pscustomobject]@{
                FilePayloadBytes    = 16MB
                BrowserPayloadBytes = 8KB
                SingleDurationMs    = 30000
                ParallelDurationMs  = 30000
                WorkerCount         = 3
                MemoryPressureMb    = 512
                IncludeList         = $true
                UseDeepPaths        = $false
            }
        }
        "hard" {
            return [pscustomobject]@{
                FilePayloadBytes    = 64MB
                BrowserPayloadBytes = 64KB
                SingleDurationMs    = 60000
                ParallelDurationMs  = 45000
                WorkerCount         = 4
                MemoryPressureMb    = 1024
                IncludeList         = $true
                UseDeepPaths        = $true
            }
        }
        default {
            throw "unsupported difficulty: $Difficulty"
        }
    }
}

function New-StressContext {
    param([pscustomobject]$Environment)

    $runStamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $runRoot = Join-Path $Environment.ArtifactsRoot $runStamp
    New-Item -ItemType Directory -Force -Path $runRoot | Out-Null

    [pscustomobject]@{
        RunRoot            = $runRoot
        BrowserFixturePath = Join-Path $Environment.RepoRoot "benchmarks\fixtures\browser\benchmark-form.html"
    }
}

function Get-StressScenarios {
    param(
        [pscustomobject]$Context,
        [pscustomobject]$Environment,
        [string]$Suite,
        [string]$Difficulty
    )

    $profile = Get-StressProfile -Difficulty $Difficulty
    $requestedSuites = switch ($Suite) {
        "single" { @("single") }
        "parallel" { @("parallel") }
        default { @("single", "parallel") }
    }

    $scenarios = @()
    foreach ($requestedSuite in $requestedSuites) {
        foreach ($kind in @("file", "browser")) {
            if ($kind -eq "file" -and $SkipFileScenario) {
                continue
            }

            if ($kind -eq "browser" -and $SkipBrowserScenario) {
                continue
            }

            $targetDurationMs = if ($requestedSuite -eq "single") {
                $profile.SingleDurationMs
            }
            else {
                $profile.ParallelDurationMs
            }

            $payloadSizeBytes = if ($kind -eq "file") {
                $profile.FilePayloadBytes
            }
            else {
                $profile.BrowserPayloadBytes
            }

            $workerCount = if ($requestedSuite -eq "single") { 1 } else { $profile.WorkerCount }
            $memoryPressureMb = if ($requestedSuite -eq "parallel") {
                $profile.MemoryPressureMb
            }
            else {
                0
            }

            $scenarioRoot = Join-Path $Context.RunRoot ("{0}-{1}-{2}" -f $kind, $requestedSuite, $Difficulty)
            New-Item -ItemType Directory -Force -Path $scenarioRoot | Out-Null

            $scenarios += [pscustomobject]@{
                scenario           = $kind
                step               = "{0}-stress" -f $requestedSuite
                suite              = $requestedSuite
                difficulty         = $Difficulty
                worker_count       = $workerCount
                target_duration_ms = $targetDurationMs
                payload_size_bytes = [int64]$payloadSizeBytes
                memory_pressure_mb = $memoryPressureMb
                include_list       = $profile.IncludeList
                use_deep_paths     = $profile.UseDeepPaths
                repo_root          = $Environment.RepoRoot
                binary_path        = $Environment.BinaryPath
                scenario_root      = $scenarioRoot
                browser_fixture    = $Context.BrowserFixturePath
            }
        }
    }

    return $scenarios
}

function Start-StressChildProcess {
    param(
        [string]$FilePath,
        [string[]]$Arguments,
        [string]$WorkingDirectory,
        [string]$StdoutPath,
        [string]$StderrPath
    )

    $argumentList = @($Arguments | ForEach-Object { Format-CommandArgument $_ })
    return Start-Process `
        -FilePath $FilePath `
        -ArgumentList $argumentList `
        -WorkingDirectory $WorkingDirectory `
        -NoNewWindow `
        -PassThru `
        -RedirectStandardOutput $StdoutPath `
        -RedirectStandardError $StderrPath
}

function New-AsciiPayload {
    param(
        [int]$SizeBytes,
        [string]$Prefix
    )

    $payload = [System.Text.StringBuilder]::new($SizeBytes)
    [void]$payload.Append($Prefix)
    while ($payload.Length -lt $SizeBytes) {
        [void]$payload.Append("x")
    }

    return $payload.ToString(0, $SizeBytes)
}

function New-AsciiFile {
    param(
        [string]$Path,
        [int64]$SizeBytes,
        [string]$Prefix
    )

    $directory = Split-Path -Parent $Path
    if ($directory) {
        New-Item -ItemType Directory -Force -Path $directory | Out-Null
    }

    $encoding = [System.Text.UTF8Encoding]::new($false)
    $writer = [System.IO.StreamWriter]::new($Path, $false, $encoding)
    try {
        $seed = if ([string]::IsNullOrEmpty($Prefix)) { "x" } else { $Prefix }
        $written = 0L
        while ($written -lt $SizeBytes) {
            $remaining = [int][Math]::Min($seed.Length, ($SizeBytes - $written))
            $writer.Write($seed.Substring(0, $remaining))
            $written += $remaining
        }
    }
    finally {
        $writer.Dispose()
    }
}

function Get-JsonPropertyValue {
    param(
        $Object,
        [string]$PropertyName
    )

    if ($null -eq $Object) {
        return $null
    }

    $property = $Object.PSObject.Properties[$PropertyName]
    if ($property) {
        return $property.Value
    }

    return $null
}

function Write-WorkerLogLine {
    param(
        [string]$Path,
        [string]$Message
    )

    Add-Content -Path $Path -Value $Message -Encoding UTF8
}

function Invoke-ProbeJson {
    param(
        [string]$BinaryPath,
        [string]$WorkingDirectory,
        [string[]]$Arguments,
        [string]$LogPath
    )

    $rawOutput = @()
    Push-Location $WorkingDirectory
    try {
        $rawOutput = & $BinaryPath @("--json") @Arguments 2>&1
        $exitCode = $LASTEXITCODE
    }
    finally {
        Pop-Location
    }

    $outputText = (@($rawOutput | ForEach-Object { $_.ToString() }) -join [Environment]::NewLine).Trim()
    if ($null -eq $outputText) {
        $outputText = ""
    }

    if ($exitCode -ne 0) {
        Write-WorkerLogLine -Path $LogPath -Message ("FAIL " + ($Arguments -join " "))
        if ($outputText) {
            Write-WorkerLogLine -Path $LogPath -Message $outputText
        }
        throw "probe command failed ($exitCode): $($Arguments -join ' ')"
    }

    $parsed = $outputText | ConvertFrom-Json
    Write-WorkerLogLine `
        -Path $LogPath `
        -Message ("OK {0} {1} {2} :: {3}" -f $parsed.adapter, $parsed.action, $parsed.status, ($Arguments -join " "))

    [pscustomobject]@{
        exit_code = $exitCode
        raw       = $outputText
        parsed    = $parsed
    }
}

function Initialize-FileWorkerAssets {
    param($Config)

    $baseRoot = if ($Config.use_deep_paths) {
        Join-Path $Config.worker_root "file\alpha\beta\gamma\delta\epsilon"
    }
    else {
        Join-Path $Config.worker_root "file"
    }

    New-Item -ItemType Directory -Force -Path $baseRoot | Out-Null

    $seedFile = Join-Path $baseRoot "seed.txt"
    $copyFile = Join-Path $baseRoot "seed-copy.txt"
    $movedFile = Join-Path $baseRoot "seed-moved.txt"

    if (-not (Test-Path $seedFile)) {
        New-AsciiFile `
            -Path $seedFile `
            -SizeBytes ([int64]$Config.payload_size_bytes) `
            -Prefix ("worker-{0}|" -f $Config.worker_id)
    }

    [pscustomobject]@{
        BaseRoot  = $baseRoot
        SeedFile  = $seedFile
        CopyFile  = $copyFile
        MovedFile = $movedFile
    }
}

function Initialize-BrowserWorkerAssets {
    param($Config)

    $browserRoot = Join-Path $Config.worker_root "browser"
    $fixtureRoot = Join-Path $browserRoot "fixture"
    $downloadRoot = Join-Path $browserRoot "downloads"
    New-Item -ItemType Directory -Force -Path $fixtureRoot, $downloadRoot | Out-Null

    $fixturePath = Join-Path $fixtureRoot "benchmark-form.html"
    Copy-Item -Path $Config.browser_fixture -Destination $fixturePath -Force

    [pscustomobject]@{
        FixturePath  = $fixturePath
        FixtureUrl   = [System.Uri]::new($fixturePath).AbsoluteUri
        DownloadRoot = $downloadRoot
    }
}

function Invoke-FileWorker {
    param($Config)

    $assets = Initialize-FileWorkerAssets -Config $Config
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $iterations = 0
    $resolvedAdapters = New-Object System.Collections.Generic.List[string]
    $lastOpenLength = 0
    $lastListCount = 0

    while ($stopwatch.Elapsed.TotalMilliseconds -lt $Config.target_duration_ms) {
        $iterations += 1

        $stat = Invoke-ProbeJson `
            -BinaryPath $Config.binary_path `
            -WorkingDirectory $Config.worker_root `
            -Arguments @("file", "stat", "--path", $assets.SeedFile) `
            -LogPath $Config.worker_log_path
        $resolvedAdapters.Add($stat.parsed.adapter)

        $open = Invoke-ProbeJson `
            -BinaryPath $Config.binary_path `
            -WorkingDirectory $Config.worker_root `
            -Arguments @("file", "open", "--path", $assets.SeedFile) `
            -LogPath $Config.worker_log_path
        $resolvedAdapters.Add($open.parsed.adapter)

        $contents = Get-JsonPropertyValue -Object $open.parsed.structured_output -PropertyName "contents"
        $lastOpenLength = if ($null -ne $contents) { $contents.Length } else { 0 }
        if ($lastOpenLength -ne [int]$Config.payload_size_bytes) {
            throw "opened file length mismatch for worker $($Config.worker_id)"
        }

        if ($Config.include_list -and (($iterations -eq 1) -or (($iterations % 3) -eq 0))) {
            $listed = Invoke-ProbeJson `
                -BinaryPath $Config.binary_path `
                -WorkingDirectory $Config.worker_root `
                -Arguments @("file", "list", "--path", $assets.BaseRoot) `
                -LogPath $Config.worker_log_path
            $resolvedAdapters.Add($listed.parsed.adapter)
            $entries = @(Get-JsonPropertyValue -Object $listed.parsed.structured_output -PropertyName "entries")
            $lastListCount = $entries.Count
            if ($lastListCount -lt 1) {
                throw "list operation returned no entries for worker $($Config.worker_id)"
            }
        }

        $copy = Invoke-ProbeJson `
            -BinaryPath $Config.binary_path `
            -WorkingDirectory $Config.worker_root `
            -Arguments @("file", "copy", "--source", $assets.SeedFile, "--destination", $assets.CopyFile) `
            -LogPath $Config.worker_log_path
        $resolvedAdapters.Add($copy.parsed.adapter)

        $move = Invoke-ProbeJson `
            -BinaryPath $Config.binary_path `
            -WorkingDirectory $Config.worker_root `
            -Arguments @("file", "move", "--source", $assets.CopyFile, "--destination", $assets.MovedFile) `
            -LogPath $Config.worker_log_path
        $resolvedAdapters.Add($move.parsed.adapter)

        $delete = Invoke-ProbeJson `
            -BinaryPath $Config.binary_path `
            -WorkingDirectory $Config.worker_root `
            -Arguments @("file", "delete", "--path", $assets.MovedFile) `
            -LogPath $Config.worker_log_path
        $resolvedAdapters.Add($delete.parsed.adapter)

        if (Test-Path $assets.MovedFile) {
            throw "moved file still exists after delete for worker $($Config.worker_id)"
        }
    }

    return [pscustomobject]@{
        worker_id          = $Config.worker_id
        command            = "file stress loop"
        success            = $true
        exit_code          = 0
        iterations         = $iterations
        resolved_adapter   = (@($resolvedAdapters | Select-Object -Unique) -join ",")
        final_result       = $null
        payload_size_bytes = [int64]$Config.payload_size_bytes
        target_duration_ms = [int]$Config.target_duration_ms
        last_open_length   = $lastOpenLength
        last_list_count    = $lastListCount
        stdout_path        = $Config.worker_log_path
        stderr_path        = $Config.worker_error_path
    }
}

function Invoke-BrowserWorker {
    param($Config)

    $assets = Initialize-BrowserWorkerAssets -Config $Config
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $iterations = 0
    $resolvedAdapters = New-Object System.Collections.Generic.List[string]
    $lastResult = ""

    while ($stopwatch.Elapsed.TotalMilliseconds -lt $Config.target_duration_ms) {
        $iterations += 1
        $name = "worker-{0}-iteration-{1}" -f $Config.worker_id, $iterations
        $notes = New-AsciiPayload `
            -SizeBytes ([int]$Config.payload_size_bytes) `
            -Prefix ("notes-{0}-{1}|" -f $Config.worker_id, $iterations)
        $expectedResult = "Submitted:{0}|{1}" -f $name, $notes
        $downloadPath = Join-Path $assets.DownloadRoot ("fixture-{0}.html" -f $iterations)
        $notesPath = Join-Path $Config.worker_root ("browser\notes-{0}.txt" -f $iterations)
        Set-Content -Path $notesPath -Value $notes -Encoding UTF8 -NoNewline

        $open = Invoke-ProbeJson `
            -BinaryPath $Config.binary_path `
            -WorkingDirectory $Config.worker_root `
            -Arguments @("browser", "open", "--url", $assets.FixtureUrl) `
            -LogPath $Config.worker_log_path
        $resolvedAdapters.Add($open.parsed.adapter)

        $body = Invoke-ProbeJson `
            -BinaryPath $Config.binary_path `
            -WorkingDirectory $Config.worker_root `
            -Arguments @("browser", "read", "--target", "body") `
            -LogPath $Config.worker_log_path
        $resolvedAdapters.Add($body.parsed.adapter)

        $bodyText = Get-JsonPropertyValue -Object $body.parsed.structured_output -PropertyName "text"
        if ($bodyText -notlike "*Waiting for input*") {
            throw "browser body did not contain expected placeholder for worker $($Config.worker_id)"
        }

        $fillName = Invoke-ProbeJson `
            -BinaryPath $Config.binary_path `
            -WorkingDirectory $Config.worker_root `
            -Arguments @("browser", "fill", "--target", "#name", "--value", $name) `
            -LogPath $Config.worker_log_path
        $resolvedAdapters.Add($fillName.parsed.adapter)

        $fillNotes = Invoke-ProbeJson `
            -BinaryPath $Config.binary_path `
            -WorkingDirectory $Config.worker_root `
            -Arguments @("browser", "fill", "--target", "#notes", "--value-file", $notesPath) `
            -LogPath $Config.worker_log_path
        $resolvedAdapters.Add($fillNotes.parsed.adapter)

        $click = Invoke-ProbeJson `
            -BinaryPath $Config.binary_path `
            -WorkingDirectory $Config.worker_root `
            -Arguments @("browser", "click", "--target", "#submit") `
            -LogPath $Config.worker_log_path
        $resolvedAdapters.Add($click.parsed.adapter)

        $readResult = Invoke-ProbeJson `
            -BinaryPath $Config.binary_path `
            -WorkingDirectory $Config.worker_root `
            -Arguments @("browser", "read", "--target", "#result") `
            -LogPath $Config.worker_log_path
        $resolvedAdapters.Add($readResult.parsed.adapter)

        $download = Invoke-ProbeJson `
            -BinaryPath $Config.binary_path `
            -WorkingDirectory $Config.worker_root `
            -Arguments @("browser", "download", "--url", $assets.FixtureUrl, "--destination", $downloadPath) `
            -LogPath $Config.worker_log_path
        $resolvedAdapters.Add($download.parsed.adapter)

        $actualResult = Get-JsonPropertyValue -Object $readResult.parsed.structured_output -PropertyName "text"
        if ($actualResult -ne $expectedResult) {
            throw "browser result mismatch for worker $($Config.worker_id)"
        }

        $bytesCopied = Get-JsonPropertyValue -Object $download.parsed.structured_output -PropertyName "bytes_copied"
        if (($bytesCopied -le 0) -or (-not (Test-Path $downloadPath))) {
            throw "browser download validation failed for worker $($Config.worker_id)"
        }

        $lastResult = $actualResult
    }

    return [pscustomobject]@{
        worker_id          = $Config.worker_id
        command            = "browser stress loop"
        success            = $true
        exit_code          = 0
        iterations         = $iterations
        resolved_adapter   = (@($resolvedAdapters | Select-Object -Unique) -join ",")
        final_result       = $lastResult
        payload_size_bytes = [int64]$Config.payload_size_bytes
        target_duration_ms = [int]$Config.target_duration_ms
        stdout_path        = $Config.worker_log_path
        stderr_path        = $Config.worker_error_path
    }
}

function Invoke-WorkerMode {
    param([string]$WorkerConfigPath)

    $config = Load-JsonFile -Path $WorkerConfigPath
    New-Item -ItemType Directory -Force -Path $config.worker_root | Out-Null
    New-Item -ItemType File -Force -Path $config.worker_log_path, $config.worker_error_path | Out-Null

    try {
        $result = if ($config.scenario -eq "file") {
            Invoke-FileWorker -Config $config
        }
        else {
            Invoke-BrowserWorker -Config $config
        }

        Save-JsonFile -Value $result -Path $config.result_path
        exit 0
    }
    catch {
        Write-WorkerLogLine -Path $config.worker_error_path -Message $_.Exception.Message
        $failure = [pscustomobject]@{
            worker_id          = $config.worker_id
            command            = "{0} stress loop" -f $config.scenario
            success            = $false
            exit_code          = 1
            iterations         = 0
            resolved_adapter   = ""
            final_result       = $null
            payload_size_bytes = [int64]$config.payload_size_bytes
            target_duration_ms = [int]$config.target_duration_ms
            stdout_path        = $config.worker_log_path
            stderr_path        = $config.worker_error_path
            error              = $_.Exception.Message
        }
        Save-JsonFile -Value $failure -Path $config.result_path
        exit 1
    }
}

function Touch-MemoryChunks {
    param(
        [System.Collections.Generic.List[byte[]]]$Chunks,
        [int]$StrideBytes
    )

    foreach ($chunk in $Chunks) {
        for ($index = 0; $index -lt $chunk.Length; $index += $StrideBytes) {
            $chunk[$index] = [byte](($chunk[$index] + 1) % 255)
        }
    }
}

function Invoke-MemoryHogMode {
    param([string]$MemoryHogConfigPath)

    $config = Load-JsonFile -Path $MemoryHogConfigPath
    $chunks = New-Object 'System.Collections.Generic.List[byte[]]'
    $remainingBytes = [int64]$config.memory_pressure_mb * 1MB
    $chunkSizeBytes = 16MB

    while ($remainingBytes -gt 0) {
        $nextChunkSize = [int][Math]::Min($remainingBytes, $chunkSizeBytes)
        $chunk = New-Object byte[] $nextChunkSize
        $chunks.Add($chunk)
        $remainingBytes -= $nextChunkSize
    }

    Touch-MemoryChunks -Chunks $chunks -StrideBytes 4096

    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    while ($stopwatch.Elapsed.TotalMilliseconds -lt $config.duration_ms) {
        Touch-MemoryChunks -Chunks $chunks -StrideBytes 65536
        Start-Sleep -Milliseconds 250
    }
}

function Invoke-ControllerMode {
    param([string]$ScenarioPath)

    $scenario = Load-JsonFile -Path $ScenarioPath
    $powerShellExe = Get-PowerShellExecutable
    $workerProcesses = @()
    $memoryHogProcess = $null
    $workersRoot = Join-Path $scenario.scenario_root "workers"
    New-Item -ItemType Directory -Force -Path $workersRoot | Out-Null

    if ([int]$scenario.memory_pressure_mb -gt 0) {
        $memoryRoot = Join-Path $scenario.scenario_root "memory-hog"
        New-Item -ItemType Directory -Force -Path $memoryRoot | Out-Null
        $memoryConfigPath = Join-Path $memoryRoot "memory-hog.json"
        $memoryStdoutPath = Join-Path $memoryRoot "memory-hog.stdout.log"
        $memoryStderrPath = Join-Path $memoryRoot "memory-hog.stderr.log"

        Save-JsonFile `
            -Value @{
                memory_pressure_mb = [int]$scenario.memory_pressure_mb
                duration_ms        = ([int]$scenario.target_duration_ms + 1500)
            } `
            -Path $memoryConfigPath

        $memoryHogProcess = Start-StressChildProcess `
            -FilePath $powerShellExe `
            -Arguments @(
                "-ExecutionPolicy", "Bypass",
                "-File", $PSCommandPath,
                "-MemoryHogMode",
                "-MemoryHogConfigPath", $memoryConfigPath
            ) `
            -WorkingDirectory $scenario.repo_root `
            -StdoutPath $memoryStdoutPath `
            -StderrPath $memoryStderrPath
    }

    for ($workerId = 1; $workerId -le [int]$scenario.worker_count; $workerId++) {
        $workerRoot = Join-Path $workersRoot ("worker-{0}" -f $workerId)
        New-Item -ItemType Directory -Force -Path $workerRoot | Out-Null

        $workerConfigPath = Join-Path $workerRoot "worker.json"
        $workerStdoutPath = Join-Path $workerRoot "worker.stdout.log"
        $workerStderrPath = Join-Path $workerRoot "worker.stderr.log"
        $workerCommandLogPath = Join-Path $workerRoot "worker.command.log"
        $workerErrorLogPath = Join-Path $workerRoot "worker.error.log"
        $workerResultPath = Join-Path $workerRoot "worker-result.json"

        Save-JsonFile `
            -Value @{
                scenario           = $scenario.scenario
                suite              = $scenario.suite
                difficulty         = $scenario.difficulty
                worker_id          = $workerId
                worker_root        = $workerRoot
                repo_root          = $scenario.repo_root
                binary_path        = $scenario.binary_path
                browser_fixture    = $scenario.browser_fixture
                target_duration_ms = [int]$scenario.target_duration_ms
                payload_size_bytes = [int64]$scenario.payload_size_bytes
                include_list       = [bool]$scenario.include_list
                use_deep_paths     = [bool]$scenario.use_deep_paths
                worker_log_path    = $workerCommandLogPath
                worker_error_path  = $workerErrorLogPath
                result_path        = $workerResultPath
            } `
            -Path $workerConfigPath

        $workerProcess = Start-StressChildProcess `
            -FilePath $powerShellExe `
            -Arguments @(
                "-ExecutionPolicy", "Bypass",
                "-File", $PSCommandPath,
                "-WorkerMode",
                "-WorkerConfigPath", $workerConfigPath
            ) `
            -WorkingDirectory $scenario.repo_root `
            -StdoutPath $workerStdoutPath `
            -StderrPath $workerStderrPath

        $workerProcesses += [pscustomobject]@{
            worker_id       = $workerId
            process         = $workerProcess
            result_path     = $workerResultPath
            command_log     = $workerCommandLogPath
            error_log       = $workerErrorLogPath
        }
    }

    foreach ($entry in $workerProcesses) {
        $entry.process.WaitForExit()
        $entry.process.Refresh()
    }

    if ($memoryHogProcess) {
        $memoryHogProcess.WaitForExit()
        $memoryHogProcess.Refresh()
    }

    $workerResults = @()
    foreach ($entry in $workerProcesses) {
        if (Test-Path $entry.result_path) {
            $workerResult = Load-JsonFile -Path $entry.result_path
        }
        else {
            $workerResult = [pscustomobject]@{
                worker_id        = $entry.worker_id
                command          = "{0} stress loop" -f $scenario.scenario
                success          = $false
                exit_code        = 1
                iterations       = 0
                resolved_adapter = ""
                final_result     = $null
                stdout_path      = $entry.command_log
                stderr_path      = $entry.error_log
            }
        }

        $workerResults += $workerResult
    }

    $hasFailures = @($workerResults | Where-Object { -not $_.success }).Count -gt 0
    $controllerResult = [pscustomobject]@{
        scenario                 = $scenario.scenario
        suite                    = $scenario.suite
        difficulty               = $scenario.difficulty
        success                  = (-not $hasFailures)
        exit_code                = if ($hasFailures) { 1 } else { 0 }
        background_process_count = if ([int]$scenario.memory_pressure_mb -gt 0) { 1 } else { 0 }
        worker_count             = [int]$scenario.worker_count
        workers                  = $workerResults
    }

    Save-JsonFile `
        -Value $controllerResult `
        -Path (Join-Path $scenario.scenario_root "controller-result.json")

    exit ([int]$controllerResult.exit_code)
}

function New-StressResultRows {
    param(
        $Scenario,
        $MeasuredController,
        $ControllerResult,
        [string]$ControllerStdoutPath,
        [string]$ControllerStderrPath
    )

    $rows = @()
    $workers = @($ControllerResult.workers)

    if ($workers.Count -eq 0) {
        $workers = @(
            [pscustomobject]@{
                worker_id        = 0
                command          = "{0} stress loop" -f $Scenario.scenario
                success          = $false
                exit_code        = 1
                iterations       = 0
                resolved_adapter = ""
                final_result     = $null
                stdout_path      = $ControllerStdoutPath
                stderr_path      = $ControllerStderrPath
            }
        )
    }

    foreach ($worker in $workers) {
        $stdoutPath = if ($worker.stdout_path) { $worker.stdout_path } else { $ControllerStdoutPath }
        $stderrPath = if ($worker.stderr_path) { $worker.stderr_path } else { $ControllerStderrPath }
        $stdout = Read-LogContent -Path $stdoutPath
        $stderr = Read-LogContent -Path $stderrPath

        $rows += [pscustomobject]@{
            scenario                 = $Scenario.scenario
            step                     = $Scenario.step
            iteration                = 1
            suite                    = $Scenario.suite
            difficulty               = $Scenario.difficulty
            resolved_adapter         = [string]$worker.resolved_adapter
            worker_id                = [int]$worker.worker_id
            target_duration_ms       = [int]$Scenario.target_duration_ms
            payload_size_bytes       = [int64]$Scenario.payload_size_bytes
            memory_pressure_mb       = [int]$Scenario.memory_pressure_mb
            background_process_count = [int]$ControllerResult.background_process_count
            command                  = [string]$worker.command
            loop_iterations          = [int]$worker.iterations
            final_result             = $worker.final_result
            exit_code                = [int]$worker.exit_code
            success                  = [bool]$worker.success
            duration_ms              = [int]$MeasuredController.duration_ms
            peak_working_set_mb      = [double]$MeasuredController.peak_working_set_mb
            peak_private_mb          = [double]$MeasuredController.peak_private_mb
            peak_cpu_percent         = [double]$MeasuredController.peak_cpu_percent
            peak_process_count       = [int]$MeasuredController.peak_process_count
            sample_count             = [int]$MeasuredController.sample_count
            stdout_preview           = ($stdout.Trim() -split "`r?`n" | Select-Object -First 4) -join " "
            stderr_preview           = ($stderr.Trim() -split "`r?`n" | Select-Object -First 4) -join " "
            stdout_path              = $stdoutPath
            stderr_path              = $stderrPath
        }
    }

    return $rows
}

function Get-MinimumPeakWorkingSetMb {
    param([string]$Difficulty)

    switch ($Difficulty) {
        "easy" { return 128 }
        "medium" { return 256 }
        "hard" { return 512 }
        default { return 0 }
    }
}

function Apply-StressAssertions {
    param(
        [object[]]$Rows,
        $Scenario
    )

    $minimumWorkingSetMb = Get-MinimumPeakWorkingSetMb -Difficulty $Scenario.difficulty

    foreach ($row in $Rows) {
        $messages = @()

        if ([string]::IsNullOrWhiteSpace($row.resolved_adapter)) {
            $messages += "resolved_adapter was empty"
        }

        if ([int]$row.sample_count -le 5) {
            $messages += "sample_count did not exceed 5"
        }

        if ($Scenario.suite -eq "parallel") {
            if ([int]$row.peak_process_count -lt ([int]$Scenario.worker_count + 1)) {
                $messages += "peak_process_count was below worker_count + 1"
            }

            if ([double]$row.peak_working_set_mb -lt $minimumWorkingSetMb) {
                $messages += "peak_working_set_mb was below ${minimumWorkingSetMb}MB"
            }
        }

        if ($messages.Count -gt 0) {
            $row.success = $false
            $row.exit_code = 1
            $existing = if ([string]::IsNullOrWhiteSpace($row.stderr_preview)) {
                @()
            }
            else {
                @($row.stderr_preview)
            }
            $row.stderr_preview = (($existing + ($messages -join "; ")) -join " | ")
        }
    }

    return $Rows
}

if ($MemoryHogMode) {
    Invoke-MemoryHogMode -MemoryHogConfigPath $MemoryHogConfigPath
    exit 0
}

if ($WorkerMode) {
    Invoke-WorkerMode -WorkerConfigPath $WorkerConfigPath
}

if ($ControllerMode) {
    Invoke-ControllerMode -ScenarioPath $ScenarioPath
}

$environment = New-BenchmarkEnvironment `
    -ScriptRoot $PSScriptRoot `
    -Configuration $Configuration `
    -ArtifactsFolder "stress-benchmarks"

Ensure-ProbeBinary -Environment $environment -SkipBuild:$SkipBuild

$context = New-StressContext -Environment $environment
$scenarios = Get-StressScenarios `
    -Context $context `
    -Environment $environment `
    -Suite $Suite `
    -Difficulty $Difficulty

if ($scenarios.Count -eq 0) {
    throw "no stress scenarios selected"
}

$powerShellExe = Get-PowerShellExecutable
$results = @()
Write-Step "running stress benchmarks into $($context.RunRoot)"

foreach ($scenario in $scenarios) {
    Write-Step ("{0} / {1} / {2}" -f $scenario.scenario, $scenario.suite, $scenario.difficulty)

    $scenarioConfigPath = Join-Path $scenario.scenario_root "scenario.json"
    $controllerStdoutPath = Join-Path $scenario.scenario_root "controller.stdout.log"
    $controllerStderrPath = Join-Path $scenario.scenario_root "controller.stderr.log"
    $controllerResultPath = Join-Path $scenario.scenario_root "controller-result.json"

    Save-JsonFile -Value $scenario -Path $scenarioConfigPath

    $measuredController = Invoke-MeasuredProcess `
        -FilePath $powerShellExe `
        -Arguments @(
            "-ExecutionPolicy", "Bypass",
            "-File", $PSCommandPath,
            "-ControllerMode",
            "-ScenarioPath", $scenarioConfigPath
        ) `
        -WorkingDirectory $environment.RepoRoot `
        -StdoutPath $controllerStdoutPath `
        -StderrPath $controllerStderrPath `
        -SampleIntervalMs $SampleIntervalMs `
        -LogicalCpuCount $environment.LogicalCpuCount `
        -Metadata @{
            scenario = $scenario.scenario
            step     = $scenario.step
            suite    = $scenario.suite
        }

    if (Test-Path $controllerResultPath) {
        $controllerResult = Load-JsonFile -Path $controllerResultPath
    }
    else {
        $controllerResult = [pscustomobject]@{
            background_process_count = if ([int]$scenario.memory_pressure_mb -gt 0) { 1 } else { 0 }
            workers                  = @()
        }
    }

    $scenarioRows = New-StressResultRows `
        -Scenario $scenario `
        -MeasuredController $measuredController `
        -ControllerResult $controllerResult `
        -ControllerStdoutPath $controllerStdoutPath `
        -ControllerStderrPath $controllerStderrPath

    $results += Apply-StressAssertions `
        -Rows $scenarioRows `
        -Scenario $scenario
}

if (-not $OutputPath) {
    $OutputPath = Join-Path $context.RunRoot "stress-benchmark-results.json"
}

Save-BenchmarkResults -Results $results -OutputPath $OutputPath
Show-BenchmarkSummary -Results $results -GroupBy @("scenario", "step", "difficulty")
Write-Step "saved stress benchmark results to $OutputPath"

if (@($results | Where-Object { -not $_.success }).Count -gt 0) {
    exit 1
}
