# VLM Visual Validation — checks demo screenshots against assertions
#
# Reads manifest.json from a demo's screenshot directory and sends
# each screenshot + assertion to a VLM for pass/fail judgment.
#
# Usage:
#   pwsh -NoProfile -File demos/validate-visual.ps1 -Demo hero
#   pwsh -NoProfile -File demos/validate-visual.ps1 -Demo hero -Provider claude
#   pwsh -NoProfile -File demos/validate-visual.ps1 -Demo hero -Provider openai
#
# Providers:
#   claude  — Claude claude-sonnet-4-20250514 via Anthropic API (ANTHROPIC_API_KEY)
#   openai  — GPT-4o via OpenAI API (OPENAI_API_KEY)
#   local   — Ollama local model (default: llava)
#
# Output: updates manifest.json with pass/fail results, prints summary

param(
    [Parameter(Mandatory)][string]$Demo,
    [string]$Provider = "claude",
    [string]$Model = ""
)

$ErrorActionPreference = 'Stop'

$screenshotDir = "$PSScriptRoot/screenshots/$Demo"
$manifestPath = "$screenshotDir/manifest.json"

if (-not (Test-Path $manifestPath)) {
    Write-Host "Manifest not found: $manifestPath" -ForegroundColor Red
    Write-Host "Run the demo first: pwsh -NoProfile -File demos/demo-01-$Demo.ps1" -ForegroundColor Yellow
    exit 1
}

$manifest = Get-Content $manifestPath -Raw | ConvertFrom-Json

Write-Host "`n=== Visual Validation: $Demo ===" -ForegroundColor Cyan
Write-Host "  Provider: $Provider" -ForegroundColor DarkGray
Write-Host "  Assertions: $($manifest.assertions.Count)" -ForegroundColor DarkGray
Write-Host ""

$passed = 0
$failed = 0
$errors = 0

foreach ($a in $manifest.assertions) {
    $imgPath = Join-Path $screenshotDir $a.file
    if (-not (Test-Path $imgPath)) {
        Write-Host "  [$($a.index)] SKIP: $($a.file) not found" -ForegroundColor Yellow
        $errors++
        continue
    }

    $imgBase64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($imgPath))
    $prompt = @"
You are a visual QA tester for a terminal multiplexer application (psmux).
Look at this screenshot and determine if this assertion is TRUE or FALSE.

ASSERTION: $($a.assertion)

Respond with exactly one line:
PASS: <brief reason>
or
FAIL: <what you see instead>
"@

    try {
        $result = switch ($Provider) {
            "claude" {
                $m = if ($Model) { $Model } else { "claude-sonnet-4-20250514" }
                $body = @{
                    model = $m
                    max_tokens = 100
                    messages = @(@{
                        role = "user"
                        content = @(
                            @{ type = "image"; source = @{ type = "base64"; media_type = "image/png"; data = $imgBase64 } }
                            @{ type = "text"; text = $prompt }
                        )
                    })
                } | ConvertTo-Json -Depth 6
                $resp = Invoke-RestMethod -Uri "https://api.anthropic.com/v1/messages" `
                    -Method POST -ContentType "application/json" `
                    -Headers @{ "x-api-key" = $env:ANTHROPIC_API_KEY; "anthropic-version" = "2023-06-01" } `
                    -Body $body
                $resp.content[0].text
            }
            "openai" {
                $m = if ($Model) { $Model } else { "gpt-4o" }
                $body = @{
                    model = $m
                    max_tokens = 100
                    messages = @(@{
                        role = "user"
                        content = @(
                            @{ type = "image_url"; image_url = @{ url = "data:image/png;base64,$imgBase64" } }
                            @{ type = "text"; text = $prompt }
                        )
                    })
                } | ConvertTo-Json -Depth 6
                $resp = Invoke-RestMethod -Uri "https://api.openai.com/v1/chat/completions" `
                    -Method POST -ContentType "application/json" `
                    -Headers @{ "Authorization" = "Bearer $env:OPENAI_API_KEY" } `
                    -Body $body
                $resp.choices[0].message.content
            }
            "local" {
                $m = if ($Model) { $Model } else { "llava" }
                $body = @{
                    model = $m
                    prompt = $prompt
                    images = @($imgBase64)
                    stream = $false
                } | ConvertTo-Json -Depth 4
                $resp = Invoke-RestMethod -Uri "http://localhost:11434/api/generate" `
                    -Method POST -ContentType "application/json" `
                    -Body $body
                $resp.response
            }
        }

        $result = $result.Trim()
        if ($result -match "^PASS") {
            Write-Host "  [$($a.index)] PASS  $($a.file)" -ForegroundColor Green
            Write-Host "         $result" -ForegroundColor DarkGray
            $a.result = "pass"
            $passed++
        } elseif ($result -match "^FAIL") {
            Write-Host "  [$($a.index)] FAIL  $($a.file)" -ForegroundColor Red
            Write-Host "         $result" -ForegroundColor Yellow
            Write-Host "         Expected: $($a.assertion)" -ForegroundColor DarkGray
            $a.result = "fail"
            $failed++
        } else {
            Write-Host "  [$($a.index)] ???   $($a.file)" -ForegroundColor Yellow
            Write-Host "         VLM response: $result" -ForegroundColor DarkGray
            $a.result = "unclear: $result"
            $errors++
        }
    } catch {
        Write-Host "  [$($a.index)] ERROR $($a.file): $_" -ForegroundColor Red
        $a.result = "error: $_"
        $errors++
    }
}

# Update manifest with results
$manifest | ConvertTo-Json -Depth 3 | Set-Content $manifestPath -Encoding UTF8

# Summary
Write-Host ""
Write-Host "  Results: $passed passed, $failed failed, $errors errors / $($manifest.assertions.Count) total" -ForegroundColor $(if ($failed -eq 0 -and $errors -eq 0) { "Green" } else { "Yellow" })
Write-Host "  Manifest updated: $manifestPath" -ForegroundColor DarkGray

if ($failed -gt 0) { exit 1 }
