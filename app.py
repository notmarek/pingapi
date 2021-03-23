import os

from quart import Quart, redirect, jsonify, request
from quart_cors import cors, route_cors

from background import get_status

app = Quart(__name__)
app = cors(app, allow_origin=[os.environ.get("CORS"), 'http://localhost'])


@app.route("/", methods=["GET"])
async def index():
    """  Redirects user to index in case they some how stumbled upon this API. """
    return redirect(os.environ.get("CORS"), code=302)


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
    data = await request.get_json(force=True)
    if "urls" in data:
        return jsonify([get_status(url) for url in data["urls"]])

    # Checks if the URL key exists and has any data
    if "url" in data:
        return jsonify(get_status(data["url"]))

    return "Bad Request", 400
