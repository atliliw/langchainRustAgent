#!/bin/bash

echo "=========================================="
echo "LangChainRust Demo 部署脚本"
echo "=========================================="

DEPLOY_DIR="/opt/langchainrust-demo"
SERVICE_NAME="langchainrust-demo"

# 检查是否在服务器上编译
if [ ! -f "./langchainrust-demo" ]; then
    echo "需要先编译 Linux 版本..."
    echo ""
    echo "请执行以下步骤："
    echo "1. 安装 Rust（如果未安装）："
    echo "   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo "   source \$HOME/.cargo/env"
    echo ""
    echo "2. 上传源代码到服务器："
    echo "   scp -r /path/to/langchainrust/demo deploy@192.168.10.100:/tmp/demo-src"
    echo ""
    echo "3. 在服务器上编译："
    echo "   cd /tmp/demo-src && cargo build --release"
    echo "   cp target/release/langchainrust-demo /opt/langchainrust-demo/"
    echo ""
    exit 1
fi

# 创建部署目录
echo "创建部署目录..."
sudo mkdir -p $DEPLOY_DIR
sudo mkdir -p $DEPLOY_DIR/static
sudo mkdir -p $DEPLOY_DIR/uploads

# 复制文件
echo "复制应用文件..."
sudo cp langchainrust-demo $DEPLOY_DIR/
sudo cp config.toml $DEPLOY_DIR/
sudo cp static/index.html $DEPLOY_DIR/static/

# 设置权限
echo "设置权限..."
sudo chmod +x $DEPLOY_DIR/langchainrust-demo
sudo chown -R deploy:deploy $DEPLOY_DIR

# 创建 systemd 服务
echo "创建 systemd 服务..."
sudo tee /etc/systemd/system/$SERVICE_NAME.service > /dev/null <<EOF
[Unit]
Description=LangChainRust Demo Service
After=network.target

[Service]
Type=simple
User=deploy
WorkingDirectory=$DEPLOY_DIR
ExecStart=$DEPLOY_DIR/langchainrust-demo
Restart=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

# 启动服务
echo "启动服务..."
sudo systemctl daemon-reload
sudo systemctl enable $SERVICE_NAME
sudo systemctl start $SERVICE_NAME

# 检查状态
echo ""
echo "=========================================="
echo "服务状态："
sudo systemctl status $SERVICE_NAME --no-pager

echo ""
echo "=========================================="
echo "部署完成！"
echo ""
echo "访问地址: http://192.168.10.100:8080"
echo ""
echo "常用命令："
echo "  查看日志: sudo journalctl -u $SERVICE_NAME -f"
echo "  重启服务: sudo systemctl restart $SERVICE_NAME"
echo "  停止服务: sudo systemctl stop $SERVICE_NAME"
echo "=========================================="