import logging
import re
import traceback

import requests
from flask import Flask, request, redirect
from flask_caching import Cache
from flask_cors import CORS, cross_origin

app = Flask(__name__)
cache = Cache(config={'CACHE_TYPE': 'simple'})
cache.init_app(app)
app.config['CORS_ORIGINS'] = ['https://piracy.moe', 'http://localhost:5000']
cors = CORS(app)

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
def index():
    """  Redirects user to index incase they some how stumbled upon this API. """
    return redirect("https://piracy.moe", code=302)


@app.route("/health", methods=["GET"])
def health():
    """ Heartbeat endpoint for status.piracy.moe. """
    return "OK", 200


@app.route("/ping", methods=["POST"])
@cross_origin()
def ping():
    """
    Handles receiving the URL, checking the validity of it, and sends it to
    the backend for processing. Returns 'online', 'down', 'cloudflare', or 'error'
    """
    # Checks if the URL key exists and has any data
    url = request.get_json('url')["url"]
    if url is None:
        return "error - url was empty"

    if not is_url(url):
        return "error - url not valid or didn't match regex"

    # Endpoint was sent a valid URL so we can make the request and return the status of the URL
    return send_network_request(url)


@cache.memoize(timeout=600)
def send_network_request(url):
    """ Sends request to URL and returns online or down, cached for 600 seconds. """
    session = requests.Session()

    # Attempts to send the HEAD request to get the status code
    try:
        # Use generic headers to evade some WAF
        headers = {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/88.0.4324.96 Safari/537.36"}
        r = session.get(url, headers=headers, timeout=7, allow_redirects=True)
    except (requests.exceptions.ConnectionError, requests.exceptions.ReadTimeout):
        app.logger.error(f"Received connection error or timeout attempting to GET: {url}")
        return "down"
    except Exception:
        app.logger.error(f"Unexpected exception occurred attempting to GET request: {url} {traceback.print_exc()}")
        return "down"

    # Fixes some issues with requesting HTTPS on HTTP sites
    if r is None:
        return "down"

    app.logger.debug(f"{url} returned HTTP status code: {r.status_code}")

    # If the request returned a valid HTTP status code, return online
    if r.status_code in [200, 300, 301, 302, 307, 308]:
        return "online"

    # If the server is presenting us with a DDoS protection challenge
    if r.status_code in [401, 403, 503, 520] and r.headers["Server"] == "cloudflare" or \
            r.status_code == 403 and r.headers["Server"] == "ddos-guard":
        return "cloudflare"

    # If we did not receive a valid HTTP status code, mark as down
    return "down"


if __name__ == "__main__":
    app.run(host="0.0.0.0", debug=False, port="5000")
