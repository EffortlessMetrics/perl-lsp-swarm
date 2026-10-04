# Appended to the public binary's emitted script and executed with -NoProfile.
# Commands are passed through stdin; no profile or execution policy is changed.
$ErrorActionPreference = 'Stop'
$failures = @()
foreach ($case in @(
    @{ line = 'perllsp --doc'; expected = @('--doctor'); kind = 'ParameterName' },
    @{ line = 'perllsp --completion p'; expected = @('powershell', 'pwsh'); kind = 'ParameterValue' },
    @{ line = 'perllsp --feature-profile p'; expected = @('prod', 'production'); kind = 'ParameterValue' },
    @{ line = 'perllsp --not-a-real-option'; expected = @(); kind = 'ParameterName' }
)) {
    $Error.Clear()
    $result = TabExpansion2 $case.line $case.line.Length
    $actual = @($result.CompletionMatches | ForEach-Object { $_.CompletionText })
    if ($actual.Count -ne $case.expected.Count) {
        $failures += "$($case.line): expected $($case.expected -join ', '); got $($actual -join ', ')"
    }
    foreach ($match in $result.CompletionMatches) {
        if ($case.expected -notcontains $match.CompletionText -or
            $match.ResultType.ToString() -ne $case.kind -or
            [string]::IsNullOrWhiteSpace($match.ToolTip)) {
            $failures += "$($case.line): incorrect result $($match.CompletionText) / $($match.ResultType)"
        }
    }
    if ($Error.Count -ne 0) {
        $failures += "$($case.line): $($Error -join '; ')"
    }
}
if ($failures.Count -ne 0) {
    $failures | ForEach-Object { [Console]::Error.WriteLine($_) }
    exit 1
}
[Console]::WriteLine('native completion probe passed')
exit 0
