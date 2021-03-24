#!/usr/bin/env sh
# start redis-server in background
echo "starting redis server"
redis-server --daemonize yes

# start rust api service
echo "starting webapi"
pingapi &

# start background service
echo "starting background ping"
python background.py
