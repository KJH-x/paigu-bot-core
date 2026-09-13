# Serve the replay viewer locally.
# Usage:  pwsh -File viewer\serve.ps1 [port]      (default port 8095)
param([int]$Port = 8095)

Set-Location -LiteralPath $PSScriptRoot

$inUse = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
if ($inUse) {
    Write-Host "端口 $Port 已被占用 (pid=$($inUse.OwningProcess))，可能已在运行。" -ForegroundColor Yellow
    Write-Host "打开: http://127.0.0.1:$Port/"
    exit 0
}

Write-Host "排谷重放查看器: http://127.0.0.1:$Port/" -ForegroundColor Green
Write-Host "按 Ctrl+C 停止。"
python -m http.server $Port --bind 127.0.0.1
