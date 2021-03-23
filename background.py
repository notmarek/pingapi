import concurrent.futures
import logging
import os
import sys
import time

import requests
from redis import Redis


def update_status(url, status, t=time.time()):
    with Redis() as r:
        r.hmset("ping:" + url, {
            "url": url,
            "status": status,
            "time": int(t)
        })
    return status


def get_status(url):
    if url is None:
        return "error - url was empty"

    with Redis(decode_responses=True) as r:
        if not r.exists("ping:" + url):
            r.sadd("urls", url)
            update_status(url, "unknown", 0)

        return r.hgetall("ping:" + url)


def ping_url(url):
    """ Sends request to URL and returns online or down, cached for 600 seconds. """
    # Attempts to send the HEAD request to get the status code
    try:
        # Use generic headers to evade some WAF
        headers = {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/88.0.4324.96 Safari/537.36"
        }
        req = requests.head(url, headers=headers, timeout=int(os.environ.get("TIMEOUT")), allow_redirects=True)
    except (requests.exceptions.ConnectionError, requests.exceptions.ReadTimeout):
        log.error(f"Received connection error or timeout attempting to GET: {url}")
        return update_status(url, "down")
    except Exception as exc:
        log.error(f"Unexpected exception occurred attempting to GET request: {url} {exc}")
        return update_status(url, "down")

    # Fixes some issues with requesting HTTPS on HTTP sites
    if req is None:
        log.info(f"{url} request returned none")
        return update_status(url, "down")

    return eval_status(url, req.status_code, req.headers)


def eval_status(url, status_code, headers):
    log.debug(f"{url} returned HTTP status code: {status_code}")

    # If the request returned a valid HTTP status code, return online
    if status_code in [200, 300, 301, 302, 307, 308]:
        return update_status(url, "up")

    # If the server is presenting us with a DDoS protection challenge
    if status_code in [401, 403, 503, 520] and headers["Server"] == "cloudflare" or \
            status_code == 403 and r.headers["Server"] == "ddos-guard":
        log.info(f"{url} status could not be determined")
        return update_status(url, "unknown")

    # If we did not receive a valid HTTP status code, mark as down
    log.info(f"{url} considered down with HTTP status code: {status_code}")
    return update_status(url, "down")


# run background task to ping urls as needed
if __name__ == "__main__":
    log = logging.getLogger('werkzeug')
    log.setLevel(logging.INFO)
    handler = logging.StreamHandler(sys.stdout)
    handler.setLevel(logging.INFO)
    formatter = logging.Formatter('[%(asctime)s] %(message)s', datefmt='%Y-%m-%d %I:%M:%S %p')
    handler.setFormatter(formatter)
    log.addHandler(handler)

    log.info("background service started")
    while True:
        with Redis(decode_responses=True) as r:
            urls = [url for url in r.smembers("urls") if
                    time.time() - int(r.hget("ping:" + url, "time")) > int(os.environ.get("INTERVAL"))]

        if len(urls) > 0:
            with concurrent.futures.ThreadPoolExecutor(max_workers=int(os.environ.get("CONNECTIONS"))) as executor:
                future_to_url = (executor.submit(ping_url, url) for url in urls)
                for future in concurrent.futures.as_completed(future_to_url):
                    try:
                        data = future.result()
                    except Exception as exc:
                        log.error(f"Unexpected exception occurred while waiting for pings to finish: {urls} {exc}")
                        data = str(type(exc))
        time.sleep(5)
