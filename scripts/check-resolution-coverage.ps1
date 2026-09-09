param(
    [string] $ReportPath = 'target/cg08-coverage.json',
    [switch] $SelfTest
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Test-ResolutionCoverage($Files, [string[]] $Expected) {
    if ($Expected.Count -eq 0) { throw 'No resolver production files found.' }
    foreach ($path in $Expected) {
        $matches = @($Files | Where-Object {
            $normalized = $_.filename.Replace('\', '/')
            $normalized -eq $path -or $normalized.EndsWith('/' + $path)
        })
        if ($matches.Count -ne 1) { throw "Expected exactly one coverage entry: $path" }
        $lines = $matches[0].summary.lines
        if ($lines.count -le 0 -or $lines.covered -lt 0 -or $lines.covered -gt $lines.count) {
            throw "Invalid line counts: $path"
        }
        # Compare counts, not rounded percentages. The threshold is not configurable.
        if (100.0 * $lines.covered -lt 95.0 * $lines.count) {
            throw "Below 95% line coverage: $path ($($lines.covered)/$($lines.count))"
        }
    }
}

if ($SelfTest) {
    $path = 'crates/gateway-application/src/resolution.rs'
    function New-Entry($Covered, $Count = 100) {
        [pscustomobject]@{
            filename = "D:\repo\$($path.Replace('/', '\'))"
            summary = [pscustomobject]@{ lines = [pscustomobject]@{ covered = $Covered; count = $Count } }
        }
    }
    Test-ResolutionCoverage @((New-Entry 95)) @($path)
    $cases = @(
        @{ files = @((New-Entry 94)); expected = @($path) },
        @{ files = @((New-Entry 9499 10000)); expected = @($path) },
        @{ files = @(); expected = @($path) },
        @{ files = @((New-Entry 100), (New-Entry 100)); expected = @($path) },
        @{ files = @((New-Entry 0 0)); expected = @($path) },
        @{ files = @((New-Entry 101)); expected = @($path) },
        @{ files = @((New-Entry 100)); expected = @('other/resolution.rs') },
        @{ files = @((New-Entry 100)); expected = @() }
    )
    foreach ($case in $cases) {
        $rejected = $false
        try { Test-ResolutionCoverage $case.files $case.expected } catch { $rejected = $true }
        if (-not $rejected) { throw 'Coverage gate self-test accepted invalid evidence.' }
    }
    Write-Output 'Resolver coverage gate self-tests passed (threshold, rounding, missing, duplicate, zero, invalid, path).'
    exit 0
}

$source = Join-Path $PSScriptRoot '../crates/gateway-application/src'
$expected = @(Get-ChildItem -LiteralPath $source -Filter 'resolution*.rs' -File |
    ForEach-Object { 'crates/gateway-application/src/' + $_.Name } | Sort-Object)
$report = Get-Content -LiteralPath $ReportPath -Raw | ConvertFrom-Json
$files = @($report.data | ForEach-Object { $_.files })
Test-ResolutionCoverage $files $expected
foreach ($path in $expected) {
    $entry = $files | Where-Object { $_.filename.Replace('\', '/').EndsWith('/' + $path) -or $_.filename.Replace('\', '/') -eq $path }
    Write-Output "$path $($entry.summary.lines.covered)/$($entry.summary.lines.count) lines (>=95%)"
}
