# 构建 release 并安装到 deploy\bin（更新只需运行本脚本）。
$ErrorActionPreference = 'Stop'

$deploy = $PSScriptRoot
$repo = Split-Path -Parent $deploy
Set-Location -LiteralPath $repo

Write-Host "[build] cargo build --release ..."
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed (exit $LASTEXITCODE)" }

$src = Join-Path $repo 'target\release\paigu-bot-core.exe'
if (-not (Test-Path -LiteralPath $src)) { throw "未找到构建产物：$src" }

$bin = Join-Path $deploy 'bin'
New-Item -ItemType Directory -Path $bin -Force | Out-Null
$dst = Join-Path $bin 'paigu-bot-core.exe'
Copy-Item -LiteralPath $src -Destination $dst -Force

Write-Host "[build] installed -> $dst"
