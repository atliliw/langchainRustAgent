#!/bin/bash
# LangChainRust Agent 部署脚本 v2
# 支持本地打包 + 远程部署
# 使用方法: ./scripts/deploy.sh

set -e

DEPLOY_DIR="/opt/langchainrust/demo"
SERVER_USER="root"
SERVER_IP="192.168.10.100"

echo "========================================"
echo "LangChainRust Agent 部署脚本"
echo "========================================"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

check_local_files() {
    echo -e "${YELLOW}[步骤1] 检查本地打包文件...${NC}"
    
    if [ ! -f "backend.tar" ]; then
        echo -e "${RED}错误: backend.tar 不存在，正在打包...${NC}"
        tar --exclude='target' --exclude='uploads' --exclude='frontend' --exclude='internal' --exclude='*.md' --exclude='docs' --exclude='.git' --exclude='*.db' --exclude='*.tar' --exclude='scripts' -cvf backend.tar Cargo.toml Cargo.lock config.toml.example crates src
    fi
    
    if [ ! -f "frontend.tar" ]; then
        echo -e "${RED}错误: frontend.tar 不存在，正在打包...${NC}"
        cd frontend && tar -cvf ../frontend.tar index.html css js && cd ..
    fi
    
    echo -e "${GREEN}✓ 本地文件准备完成${NC}"
    echo "  backend.tar: $(ls -lh backend.tar | awk '{print $5}')"
    echo "  frontend.tar: $(ls -lh frontend.tar | awk '{print $5}')"
}

upload_files() {
    echo -e "${YELLOW}[步骤2] 上传文件到服务器...${NC}"
    
    echo "上传 backend.tar..."
    scp backend.tar ${SERVER_USER}@${SERVER_IP}:${DEPLOY_DIR}/
    
    echo "上传 frontend.tar..."
    scp frontend.tar ${SERVER_USER}@${SERVER_IP}:${DEPLOY_DIR}/frontend/
    
    echo -e "${GREEN}✓ 文件上传完成${NC}"
}

remote_deploy() {
    echo -e "${YELLOW}[步骤3] 远程部署...${NC}"
    
    ssh ${SERVER_USER}@${SERVER_IP} << 'REMOTE_SCRIPT'
set -e

DEPLOY_DIR="/opt/langchainrust/demo"
SERVICE_NAME="langchainrust-agent"

cd ${DEPLOY_DIR}

echo "停止服务..."
systemctl stop ${SERVICE_NAME} || true

echo "清理旧代码..."
rm -rf src crates Cargo.toml Cargo.lock config.toml.example 2>/dev/null || true

echo "解压后端代码..."
tar -xf backend.tar

echo "解压前端文件..."
cd frontend && tar -xf frontend.tar && cd ..

echo "检查 Rust 环境..."
source ~/.cargo/env || source $HOME/.cargo/env || true
if ! command -v cargo &> /dev/null; then
    echo "安装 Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source $HOME/.cargo/env
fi

echo "编译后端 (release模式)..."
echo "预计需要 2-5 分钟，请耐心等待..."
cargo build --release

echo "启动服务..."
systemctl start ${SERVICE_NAME}

echo "等待服务启动..."
sleep 3

echo "验证服务状态..."
systemctl status ${SERVICE_NAME} --no-pager || true

echo "测试 API..."
curl -s http://localhost:8080/api/stats | python3 -m json.tool || curl -s http://localhost:8080/api/stats

REMOTE_SCRIPT
    
    echo -e "${GREEN}✓ 远程部署完成${NC}"
}

verify_deployment() {
    echo -e "${YELLOW}[步骤4] 验证部署结果...${NC}"
    
    echo "测试远程 API..."
    sleep 2
    API_RESULT=$(curl -s --connect-timeout 5 http://${SERVER_IP}:8080/api/stats 2>/dev/null || echo "")
    
    if [ -n "$API_RESULT" ] && [ "$API_RESULT" != "" ]; then
        echo -e "${GREEN}✓ API 响应正常${NC}"
        echo "  ${API_RESULT}"
    else
        echo -e "${YELLOW}⚠ API 响应超时，请检查服务状态${NC}"
        echo "  查看日志: ssh ${SERVER_USER}@${SERVER_IP} 'journalctl -u langchainrust-agent -n 50'"
    fi
}

print_summary() {
    echo ""
    echo "========================================"
    echo -e "${GREEN}部署完成！${NC}"
    echo "========================================"
    echo ""
    echo -e "${BLUE}访问地址: http://${SERVER_IP}:8080${NC}"
    echo ""
    echo "新增功能:"
    echo "  • 深色模式 (右上角切换按钮)"
    echo "  • 系统监控面板 (导航栏 '📊 系统监控')"
    echo "  • 消息编辑/删除/复制 (hover 消息显示操作按钮)"
    echo "  • 搜索历史记录 (搜索框下方)"
    echo "  • 批量文件上传 (拖拽多文件)"
    echo "  • 文档预览 (文档管理点击预览)"
    echo ""
    echo "常用命令:"
    echo "  查看日志: ssh ${SERVER_USER}@${SERVER_IP} 'journalctl -u langchainrust-agent -f'"
    echo "  重启服务: ssh ${SERVER_USER}@${SERVER_IP} 'systemctl restart langchainrust-agent'"
    echo "  服务状态: ssh ${SERVER_USER}@${SERVER_IP} 'systemctl status langchainrust-agent'"
    echo ""
}

main() {
    check_local_files
    upload_files
    remote_deploy
    verify_deployment
    print_summary
}

main