import logging
import re
import time
import traceback

import requests
from quart import Quart, redirect, jsonify, request
from quart_cors import cors, route_cors
from redis import Redis

app = Quart(__name__)
app = cors(app, allow_origin=['https://piracy.moe', 'http://localhost'])

# Sets up basic logging
logging.basicConfig(format='[%(asctime)s] %(message)s', datefmt='%m/%d/%Y %I:%M:%S %p')
app.logger.setLevel(logging.ERROR)

# Disables logging for every time an endpoint is visited
log = logging.getLogger('werkzeug')
log.setLevel(logging.ERROR)


def is_url(url):
    """ Checks if the URL data received matches our RegEx to verify it's a real URL. """
    return re.match(
        r"(?:(?:(?:https?|ftp):)?\/\/)(?:\S+(?::\S*)?@)?(?:(?!(?:10|127)(?:\.\d{1,3}){3})(?!(?:169\.254|192\.168)(?:\.\d{1,3}){2})(?!172\.(?:1[6-9]|2\d|3[0-1])(?:\.\d{1,3}){2})(?:[1-9]\d?|1\d\d|2[01]\d|22[0-3])(?:\.(?:1?\d{1,2}|2[0-4]\d|25[0-5])){2}(?:\.(?:[1-9]\d?|1\d\d|2[0-4]\d|25[0-4]))|(?:(?:[a-z\u00a1-\uffff0-9]-*)*[a-z\u00a1-\uffff0-9]+)(?:\.(?:[a-z\u00a1-\uffff0-9]-*)*[a-z\u00a1-\uffff0-9]+)*(?:\.(?:[a-z\u00a1-\uffff]{2,})))(?::\d{2,5})?(?:[/?#]\S*)?$",
        url)


@app.route("/", methods=["GET"])
async def index():
    """  Redirects user to index incase they some how stumbled upon this API. """
    return redirect("https://piracy.moe", code=302)


@app.route("/health", methods=["GET"])
async def health():
    """ Heartbeat endpoint for status.piracy.moe. """
    return "OK", 200


@app.route("/ping", methods=["POST"])
@route_cors()
async def ping():
    """
    Handles receiving the URL, checking the validity of it, and sends it to
    the backend for processing. Returns 'online', 'down', 'cloudflare', or 'error'
    """
    # Checks if multiple URLs should be fetched
    data = request.get_json(force=True)
    if "urls" not in data:
        # Checks if the URL key exists and has any data
        if "url" not in data:
            return "error - url was empty"
        url = data["url"]
        if not is_url(url):
            return "error - url not valid or didn't match regex"

        # Endpoint was sent a valid URL so we can make the request and return the status of the URL
        with Redis(decode_responses=True) as r:
            if r.exists("ping:" + url) and int(time.time()) - int(r.hget("ping:" + url, "time")) > 600:
                ping_url(url)
            elif not r.exists("ping:" + url):
                await ping_url(url)

            return jsonify(r.hgetall("ping:" + url))

    urls = data["urls"]
    with Redis(decode_responses=True) as r:
        tasks = []
        for url in urls:
            if url is None:
                return "error - url was empty"

            if not is_url(url):
                return "error - url not valid or didn't match regex"

            if r.exists("ping:" + url) and int(time.time()) - int(r.hget("ping:" + url, "time")) > 600:
                ping_url(url)
            elif not r.exists("ping:" + url):
                tasks.append(ping_url(url))
        for tr in tasks:
            await tr
        return jsonify([r.hgetall("ping:" + url) for url in urls])


def update_status(url, status):
    with Redis(decode_responses=True) as r:
        r.hmset("ping:" + url, {
            "url": url,
            "status": status,
            "time": int(time.time())
        })
    return status


async def ping_url(url):
    """ Sends request to URL and returns online or down, cached for 600 seconds. """
    # Attempts to send the HEAD request to get the status code
    try:
        # Use generic headers to evade some WAF
        headers = {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/88.0.4324.96 Safari/537.36"
        }
        r = requests.get(url, headers=headers, timeout=10, allow_redirects=True)
    except (requests.exceptions.ConnectionError, requests.exceptions.ReadTimeout):
        app.logger.error(f"Received connection error or timeout attempting to GET: {url}")
        return update_status(url, "down")
    except:
        app.logger.error(
            f"Unexpected exception occurred attempting to GET request: {url} {traceback.print_exc()}")
        print(f"Unexpected exception occurred attempting to GET request: {url} {traceback.print_exc()}")
        return update_status(url, "down")

    # Fixes some issues with requesting HTTPS on HTTP sites
    if r is None:
        return update_status(url, "down")

    app.logger.debug(f"{url} returned HTTP status code: {r.status_code}")

    # If the request returned a valid HTTP status code, return online
    if r.status_code in [200, 300, 301, 302, 307, 308]:
        return update_status(url, "online")

    # If the server is presenting us with a DDoS protection challenge
    if r.status_code in [401, 403, 503, 520] and r.headers["Server"] == "cloudflare" or \
            r.status_code == 403 and r.headers["Server"] == "ddos-guard":
        update_status(url, "cloudflare")

    # If we did not receive a valid HTTP status code, mark as down
    update_status(url, "down")


if __name__ == "__main__":
    app.run(host="0.0.0.0", debug=False, port=5000)
