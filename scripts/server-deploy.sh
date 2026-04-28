#!/bin/bash
# 服务器端部署脚本
# 在服务器上直接执行: cd /opt/langchainrust/demo && ./server-deploy.sh

set -e

DEPLOY_DIR="/opt/langchainrust/demo"
SERVICE_NAME="langchainrust-agent"

echo "========================================"
echo "LangChainRust Agent 服务器端部署"
echo "========================================"

cd ${DEPLOY_DIR}

echo "[1/5] 停止服务..."
systemctl stop ${SERVICE_NAME} || true

echo "[2/5] 清理旧代码..."
rm -rf src crates Cargo.toml Cargo.lock config.toml.example 2>/dev/null || true

echo "[3/5] 解压文件..."
if [ -f "backend.tar" ]; then
    tar -xf backend.tar
    echo "  ✓ backend.tar 解压完成"
else
    echo "  ✗ backend.tar 不存在"
    exit 1
fi

if [ -f "frontend/frontend.tar" ]; then
    cd frontend && tar -xf frontend.tar && cd ..
    echo "  ✓ frontend.tar 解压完成"
else
    echo "  ✗ frontend/frontend.tar 不存在"
fi

echo "[4/5] 编译后端..."
source ~/.cargo/env || source $HOME/.cargo/env || true
cargo build --release

echo "[5/5] 启动服务..."
systemctl start ${SERVICE_NAME}
sleep 2
systemctl status ${SERVICE_NAME} --no-pager

echo ""
echo "========================================"
echo "部署完成！访问: http://localhost:8080"
echo "========================================"