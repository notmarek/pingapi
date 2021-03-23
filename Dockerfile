FROM python:3.9-alpine

# install nginx
RUN apk update && \
    apk add --no-cache nginx redis

# install needed python packages
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# replace default nginx conf
COPY nginx.conf /etc/nginx/conf.d/default.conf

WORKDIR /app
COPY . /app

EXPOSE 5000
HEALTHCHECK CMD curl --fail http://localhost:5000/health || exit 1

CMD redis-server --daemonize yes && nginx -g 'pid /tmp/nginx.pid;' && hypercorn -k asyncio -w 5 -b unix:/tmp/pingapi.sock "app:app"