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

. (Join-Path $PSScriptRoot "benchmark-common.ps1")

$environment = New-BenchmarkEnvironment `
    -ScriptRoot $PSScriptRoot `
    -Configuration $Configuration `
    -ArtifactsFolder "benchmarks"

function New-BenchmarkContext {
    param([pscustomobject]$Environment)

    $runStamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $runRoot = Join-Path $Environment.ArtifactsRoot $runStamp
    $fileRoot = Join-Path $runRoot "file"
    New-Item -ItemType Directory -Force -Path $runRoot, $fileRoot | Out-Null

    $seedFile = Join-Path $fileRoot "seed.txt"
    @(
        "Rover benchmark seed file",
        "Timestamp: $(Get-Date -Format o)",
        "This file exists to benchmark rover-probe file operations."
    ) | Set-Content -Path $seedFile -Encoding UTF8

    $browserFixture = Join-Path $Environment.RepoRoot "benchmarks\fixtures\browser\benchmark-form.html"
    $browserUri = [System.Uri]::new($browserFixture).AbsoluteUri

    [pscustomobject]@{
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
            Scenario  = "file"
            Name      = "stat-seed"
            Arguments = @("file", "stat", "--path", $Context.SeedFile)
        }
        $steps += [pscustomobject]@{
            Scenario  = "file"
            Name      = "copy-seed"
            Arguments = @("file", "copy", "--source", $Context.SeedFile, "--destination", $Context.CopyFile)
        }
        $steps += [pscustomobject]@{
            Scenario  = "file"
            Name      = "move-copy"
            Arguments = @("file", "move", "--source", $Context.CopyFile, "--destination", $Context.MovedFile)
        }
        $steps += [pscustomobject]@{
            Scenario  = "file"
            Name      = "delete-moved"
            Arguments = @("file", "delete", "--path", $Context.MovedFile)
        }
    }

    if (-not $SkipBrowserScenario) {
        $steps += [pscustomobject]@{
            Scenario  = "browser"
            Name      = "open-fixture"
            Arguments = @("browser", "open", "--url", $Context.BrowserFixtureUri)
        }
        $steps += [pscustomobject]@{
            Scenario  = "browser"
            Name      = "read-body"
            Arguments = @("browser", "read", "--target", "body")
        }
        $steps += [pscustomobject]@{
            Scenario  = "browser"
            Name      = "fill-name"
            Arguments = @("browser", "fill", "--target", "#name", "--value", "Rover benchmark")
        }
        $steps += [pscustomobject]@{
            Scenario  = "browser"
            Name      = "fill-notes"
            Arguments = @("browser", "fill", "--target", "#notes", "--value", "No LLM required")
        }
        $steps += [pscustomobject]@{
            Scenario  = "browser"
            Name      = "click-submit"
            Arguments = @("browser", "click", "--target", "#submit")
        }
        $steps += [pscustomobject]@{
            Scenario  = "browser"
            Name      = "read-result"
            Arguments = @("browser", "read", "--target", "#result")
        }
    }

    return $steps
}

Ensure-ProbeBinary -Environment $environment -SkipBuild:$SkipBuild

$context = New-BenchmarkContext -Environment $environment
$results = @()
$steps = Get-ScenarioSteps -Context $context

if ($steps.Count -eq 0) {
    throw "no benchmark steps selected"
}

Write-Step "running benchmarks into $($context.RunRoot)"

for ($iteration = 1; $iteration -le $Iterations; $iteration++) {
    foreach ($step in $steps) {
        Write-Step ("{0} / {1} (iteration {2})" -f $step.Scenario, $step.Name, $iteration)
        $runId = "{0}-{1}-{2}" -f $step.Scenario, $step.Name, $iteration
        $stdoutPath = Join-Path $context.RunRoot "$runId.stdout.log"
        $stderrPath = Join-Path $context.RunRoot "$runId.stderr.log"

        $results += Invoke-MeasuredProcess `
            -FilePath $environment.BinaryPath `
            -Arguments $step.Arguments `
            -WorkingDirectory $environment.RepoRoot `
            -StdoutPath $stdoutPath `
            -StderrPath $stderrPath `
            -SampleIntervalMs $SampleIntervalMs `
            -LogicalCpuCount $environment.LogicalCpuCount `
            -Metadata @{
                scenario  = $step.Scenario
                step      = $step.Name
                iteration = $iteration
                command   = ($step.Arguments -join " ")
            }
    }
}

if (-not $OutputPath) {
    $OutputPath = Join-Path $context.RunRoot "benchmark-results.json"
}

Save-BenchmarkResults -Results $results -OutputPath $OutputPath
Show-BenchmarkSummary -Results $results
Write-Step "saved benchmark results to $OutputPath"
