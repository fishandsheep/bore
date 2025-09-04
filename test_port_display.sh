#!/bin/bash

# Test script for bore web GUI with port display
echo "=== Testing Port Display Feature ==="

# Build the project
echo "Building bore web GUI..."
cargo build --bin bore-gui

# Start web GUI in background
echo "Starting web GUI on http://localhost:3001"
./target/debug/bore-gui &
WEB_PID=$!
sleep 3

# Test 1: Start a server first
echo -e "\n1. Starting bore server..."
SERVER_RESPONSE=$(curl -s -X POST http://localhost:3001/api/server/start \
  -H "Content-Type: application/json" \
  -d '{
    "min_port": 10000,
    "max_port": 60000,
    "secret": null,
    "bind_addr": "127.0.0.1",
    "bind_tunnels": null
  }')
echo "Server response: $SERVER_RESPONSE"

# Extract server ID
SERVER_ID=$(echo $SERVER_RESPONSE | grep -o '"id":"[^"]*' | grep -o '[^"]*$')
echo "Server ID: $SERVER_ID"

# Wait a moment for server to start
sleep 2

# Test 2: Start client with port 0 (auto assign)
echo -e "\n2. Starting bore client with auto port..."
CLIENT_RESPONSE=$(curl -s -X POST http://localhost:3001/api/client/start \
  -H "Content-Type: application/json" \
  -d '{
    "local_host": "127.0.0.1",
    "local_port": 8080,
    "to": "127.0.0.1",
    "port": 0,
    "secret": null
  }')
echo "Client response: $CLIENT_RESPONSE"

# Extract client ID
CLIENT_ID=$(echo $CLIENT_RESPONSE | grep -o '"id":"[^"]*' | grep -o '[^"]*$')
echo "Client ID: $CLIENT_ID"

# Wait for port assignment
echo -e "\n3. Waiting for port assignment (5 seconds)..."
sleep 5

# Check if the processes are still running
echo -e "\n4. Checking process status..."
ps aux | grep bore | grep -v grep

# Clean up
echo -e "\n5. Cleaning up..."
kill $WEB_PID 2>/dev/null
pkill -f "bore local" 2>/dev/null
pkill -f "bore server" 2>/dev/null

echo -e "\nTest completed!"
echo "To see the port display in action:"
echo "1. Run './target/debug/bore-gui'"
echo "2. Open http://localhost:3001 in browser"
echo "3. Start a server"
echo "4. Start a client with port 0"
echo "5. Check the logs for the assigned port"