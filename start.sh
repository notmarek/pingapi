#!/usr/bin/env bash
# run ASGI-app of pingapi
echo "starting ASGI-workers"
hypercorn -k asyncio -w 5 -b unix:/tmp/pingapi.sock "app:app" &

# start nginx
echo "starting nginx server"
nginx -g 'pid /tmp/nginx.pid;'

# start redis-server in background
echo "starting redis server"
redis-server --daemonize yes

# timer for hypercorn
echo "waiting for hypercorn workers to have started"
sleep 5

# make the unix-socket writeable for nginx-worker
echo "modify rights of /tmp/pingapi.sock"
chmod 777 /tmp/pingapi.sock

# start background service
python background.py
