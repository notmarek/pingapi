#!/usr/bin/env sh
# start redis-server in background
echo "starting redis server"
redis-server --daemonize yes

# start rust api service
pingapi

# start background service
python background.py
