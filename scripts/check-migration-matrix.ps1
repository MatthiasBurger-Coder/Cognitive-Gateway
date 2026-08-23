[CmdletBinding()]
param(
    [string] $RepositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
)

$ErrorActionPreference = 'Stop'
$inventoryPath = Join-Path $RepositoryRoot 'docs/tiny-swarm-world-agent-skill-inventory.md'
$matrixPath = Join-Path $RepositoryRoot 'docs/tiny-swarm-world-agent-skill-migration.md'

$inventory = Get-Content -LiteralPath $inventoryPath
$matrix = Get-Content -LiteralPath $matrixPath
$allowed = @(
    'generic-core-catalog', 'generic-development', 'generic-quality',
    'generic-architecture', 'generic-documentation', 'generic-governance',
    'project-specific:tiny-swarm-world', 'duplicate/merge-candidate', 'deprecated'
)

function Get-Candidates([string[]] $Lines, [string] $PathPrefix) {
    $candidatePattern = '^\| `([^`]+)` \| `\.agents/' + [regex]::Escape($PathPrefix)
    $Lines | Where-Object { $_ -match $candidatePattern } |
        ForEach-Object { [regex]::Match($_, '^\| `([^`]+)`').Groups[1].Value }
}

function Get-MatrixRows([string[]] $Lines, [string] $Section) {
    $start = [Array]::IndexOf($Lines, $Section)
    if ($start -lt 0) { throw "Missing matrix section: $Section" }
    $end = $Lines.Count
    for ($i = $start + 1; $i -lt $Lines.Count; $i++) {
        if ($Lines[$i] -match '^## ') { $end = $i; break }
    }
    $Lines[($start + 1)..($end - 1)] | Where-Object {
        $_ -match '^\| `[^`]+` \| `[^`]+` \| `[^`]+` \|'
    }
}

function Assert-Matrix([string[]] $Candidates, [string[]] $Rows, [string] $Name) {
    $seen = @{}
    foreach ($row in $Rows) {
        $match = [regex]::Match($row, '^\| `([^`]+)` \| `([^`]+)` \| `([^`]+)` \|')
        if (-not $match.Success) { throw "Malformed $Name matrix row: $row" }
        $source = $match.Groups[1].Value
        $canonical = $match.Groups[2].Value
        $classification = $match.Groups[3].Value
        if ($seen.ContainsKey($source)) { throw "Duplicate $Name candidate: $source" }
        $seen[$source] = $true
        if ($Candidates -notcontains $source) { throw "Unknown $Name candidate: $source" }
        if ($classification -notin $allowed) { throw "Invalid classification for ${source}: $classification" }
        if ($canonical -notmatch '^[a-z0-9](?:[a-z0-9-]{0,126}[a-z0-9])?$') {
            throw "Invalid canonical ID for ${source}: $canonical"
        }
    }
    $missing = @($Candidates | Where-Object { -not $seen.ContainsKey($_) })
    if ($missing.Count -gt 0) { throw "Missing $Name candidates: $($missing -join ', ')" }
    if ($seen.Count -ne $Candidates.Count) { throw "$Name count mismatch" }
}

$roles = @(Get-Candidates $inventory 'roles/')
$skills = @(Get-Candidates $inventory 'skills/')
$roleRows = @(Get-MatrixRows $matrix '## Role decision matrix')
$skillRows = @(Get-MatrixRows $matrix '## Project-skill decision matrix')
Assert-Matrix $roles $roleRows 'role'
Assert-Matrix $skills $skillRows 'skill'
Write-Output "Migration matrix valid: $($roles.Count) roles, $($skills.Count) project skills, $($roleRows.Count + $skillRows.Count) decisions."
