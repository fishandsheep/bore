#!/bin/bash

# Test script for bore web GUI
echo "=== Bore Web GUI Test ==="

# Build the project
echo "Building bore web GUI..."
cargo build --bin bore-gui
if [ $? -ne 0 ]; then
    echo "Build failed!"
    exit 1
fi
echo "Build successful!"

# Start web GUI in background
echo "Starting web GUI on http://localhost:3001"
./target/debug/bore-gui &
WEB_PID=$!
sleep 2

# Test server start
echo -e "\n1. Testing server start..."
SERVER_RESPONSE=$(curl -s -X POST http://localhost:3001/api/server/start \
  -H "Content-Type: application/json" \
  -d '{
    "min_port": 10000,
    "max_port": 60000,
    "secret": null,
    "bind_addr": "0.0.0.0",
    "bind_tunnels": null
  }')
echo "Server response: $SERVER_RESPONSE"

# Test client start
echo -e "\n2. Testing client start..."
CLIENT_RESPONSE=$(curl -s -X POST http://localhost:3001/api/client/start \
  -H "Content-Type: application/json" \
  -d '{
    "local_host": "localhost",
    "local_port": 8000,
    "to": "localhost",
    "port": 9000,
    "secret": null
  }')
echo "Client response: $CLIENT_RESPONSE"

# Clean up
echo -e "\n3. Cleaning up..."
kill $WEB_PID 2>/dev/null
echo "Test completed!"