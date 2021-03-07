import logging
import re
import traceback

import requests
from flask import Flask, request
from flask_caching import Cache
from flask_cors import CORS, cross_origin

app = Flask(__name__)
cache = Cache(config={'CACHE_TYPE': 'simple'})
cache.init_app(app)
cors = CORS(app)   # TODO: Configure this to only allow piracy.moe requests.

logging.basicConfig(format='[%(asctime)s] %(message)s', datefmt='%m/%d/%Y %I:%M:%S %p')
app.logger.setLevel(logging.DEBUG)

@app.route("/", methods=["GET"])
def index():
    return "Hello, world!", 200


# HTTP testing route for ease of use during development.
@app.route("/test/insecure/<domain>", methods=["GET"])
def test_inscure(domain):
    domain = "http://" + domain
    return str(send_network_request(domain))


# HTTP testing route for ease of use during development.
@app.route("/test/secure/<domain>", methods=["GET"])
def test_secure(domain):
    domain = "https://" + domain
    return str(send_network_request(domain))


@app.route("/ping", methods=["POST"])
@cross_origin()
def ping():
    """
    Handles receiving the URL, checking the validity of it, and sending it to
    the backend for processing. Returns either 'online' or 'down' for the frontend.
    """
    # Checks if the URL key exists and has any data.
    url = request.get_json('url')["url"]
    if url is None:
        return "error - url was empty"

    # Checks if the URL data received matches our RegEx to verify it's a real URL.
    match = re.match(r"((http|https)\:\/\/)?[a-zA-Z0-9\.\/\?\:@\-_=#]+\.([a-zA-Z]){2,6}([a-zA-Z0-9\.\&\/\?\:@\-_=#])*", url)
    if not match:
        return "error - url not valid or didn't match regex"

    # Endpoint was sent a valid URL so we can make the request and return the status of the URL.
    return send_network_request(url)


# Sends request to URL and returns online or down, cached for 600 seconds.
@cache.memoize(timeout=600)
def send_network_request(url):
    session = requests.Session()

    # Attempts to send the HEAD request to get the status code.
    try:
        # Use generic headers to evade some WAF.
        headers = {"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/88.0.4324.96 Safari/537.36"}
        r = session.get(url, headers=headers, timeout=7, allow_redirects=True)
    except (requests.exceptions.ConnectionError, requests.exceptions.ReadTimeout):
        app.logger.error(f"Received connection error or timeout attempting to GET: {url}")
        return "down"
    except Exception:
        app.logger.error(f"Unexpected exception occurred attempting to HEAD request: {url} {traceback.print_exc()}")
        return "down"

    # Fixes some issues with requesting HTTPS on HTTP sites.
    if r is None:
        return "down"

    app.logger.debug(f"{url} returned HTTP status code: {r.status_code}")

    # If the request returned a valid HTTP status code, return online.
    if r.status_code in [200, 300, 301, 302, 307, 308]:
        return "online"

    # If the server is presenting us with a (presumable) Cloudflare challenge.
    if r.status_code == 503 and r.headers["Server"] == "cloudflare":
        return "cloudflare"

    # If we did not receive a valid HTTP status code, mark as down.
    return "down"


if __name__ == "__main__":
    app.run(host="0.0.0.0", debug=False, port="5000")
