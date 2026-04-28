# LangChainRust Agent 部署脚本 (Windows PowerShell)
# 使用方法: .\scripts\deploy.ps1

$DeployDir = "/opt/langchainrust/demo"
$ServerUser = "root"
$ServerIP = "192.168.10.100"

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "LangChainRust Agent 部署脚本" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

# 步骤1: 检查打包文件
Write-Host "[步骤1] 检查打包文件..." -ForegroundColor Yellow

if (-not (Test-Path "backend.tar")) {
    Write-Host "backend.tar 不存在" -ForegroundColor Red
    Write-Host "请先手动打包后端文件" -ForegroundColor Red
    exit 1
}

if (-not (Test-Path "frontend.tar")) {
    Write-Host "frontend.tar 不存在" -ForegroundColor Red
    Write-Host "请先手动打包前端文件" -ForegroundColor Red
    exit 1
}

Write-Host "✓ 文件检查完成" -ForegroundColor Green

# 步骤2: 上传文件
Write-Host "[步骤2] 上传文件到服务器..." -ForegroundColor Yellow

Write-Host "上传 backend.tar..."
scp backend.tar ${ServerUser}@${ServerIP}:${DeployDir}/

Write-Host "上传 frontend.tar..."
scp frontend.tar ${ServerUser}@${ServerIP}:${DeployDir}/frontend/

Write-Host "✓ 文件上传完成" -ForegroundColor Green

# 步骤3: 远程部署
Write-Host "[步骤3] 远程部署..." -ForegroundColor Yellow

$remoteScript = @"
cd ${DeployDir}
systemctl stop langchainrust-agent || true
rm -rf src crates Cargo.toml Cargo.lock config.toml.example
tar -xf backend.tar
cd frontend && tar -xf frontend.tar && cd ..
source ~/.cargo/env
cargo build --release
systemctl start langchainrust-agent
sleep 3
systemctl status langchainrust-agent --no-pager
curl http://localhost:8080/api/stats
"@

ssh ${ServerUser}@${ServerIP} $remoteScript

Write-Host "✓ 远程部署完成" -ForegroundColor Green

# 步骤4: 验证
Write-Host "[步骤4] 验证部署..." -ForegroundColor Yellow
Start-Sleep -Seconds 2

try {
    $result = Invoke-RestMethod -Uri "http://${ServerIP}:8080/api/stats" -TimeoutSec 5
    Write-Host "✓ API 响应正常" -ForegroundColor Green
    Write-Host ("  文档数: {0}" -f $result.total_documents)
} catch {
    Write-Host "⚠ API 响应超时" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "========================================" -ForegroundColor Green
Write-Host "部署完成！" -ForegroundColor Green
Write-Host "========================================" -ForegroundColor Green
Write-Host ""
Write-Host "访问地址: http://${ServerIP}:8080" -ForegroundColor Cyan
Write-Host ""
Write-Host "查看日志: ssh ${ServerUser}@${ServerIP} 'journalctl -u langchainrust-agent -f'"