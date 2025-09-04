#!/bin/bash

echo "=== 测试修复后的端口显示功能 ==="

# 重新构建
echo "构建项目..."
cargo build --bin bore-gui

# 启动Web GUI
echo "启动Web GUI..."
./target/debug/bore-gui &
WEB_PID=$!
sleep 3

# 测试1: 使用未被占用的端口
echo -e "\n测试1: 使用端口 9999（未被占用）"
RESPONSE=$(curl -s -X POST http://localhost:3001/api/client/start \
  -H "Content-Type: application/json" \
  -d '{
    "local_host": "localhost",
    "local_port": 9999,
    "to": "bore.pub",
    "port": 0,
    "secret": null
  }')

echo "响应: $RESPONSE"
CLIENT_ID=$(echo $RESPONSE | grep -o '"id":"[^"]*' | grep -o '[^"]*$')
echo "客户端ID: $CLIENT_ID"

# 等待端口分配
echo -e "\n等待端口分配..."
sleep 3

# 清理
echo -e "\n清理进程..."
kill $WEB_PID 2>/dev/null
pkill -f "bore local" 2>/dev/null

echo -e "\n测试完成！"
echo ""
echo "使用说明："
echo "1. 运行 './target/debug/bore-gui'"
echo "2. 在浏览器中访问 http://localhost:3001"
echo "3. 确保本地端口未被其他程序占用"
echo "4. 启动客户端后，查看日志中的端口信息"