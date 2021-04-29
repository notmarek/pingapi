extern crate redis;

use std::{env, thread};
use std::collections::HashMap;
use std::str::from_utf8;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use actix_cors::Cors;
use actix_web::{App, Error, get, http, HttpResponse, HttpServer, post, Responder, web};
use actix_web::client::{Client, Connector};
use log::{debug, info, trace};
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use redis::Commands;
use serde::Deserialize;

#[derive(Deserialize)]
struct Url {
    url: String
}

#[derive(Deserialize)]
struct Urls {
    urls: Vec<String>
}


// ------------------------------------------------------------------------------
// background tasks
// ------------------------------------------------------------------------------

// returns a new connection to redis server
fn get_redis_con() -> redis::Connection {
    let client = redis::Client::open("redis://127.0.0.1")
        .expect("Cannot connect to local redis server");
    client.get_connection()
        .expect("Connection to redis server failed")
}

// fetches the current status of given url from the redis server
fn get_status(url: &String) -> HashMap<String, String> {
    let mut con = get_redis_con();
    let ex: bool = con.exists(format!("ping:{}", url))
        .expect("Failed to determine existence of url");
    if !ex {
        con.sadd::<&str, &String, bool>("urls", url)
            .expect("Failed to add url to redis urls list");
        con.hset_multiple::<String, &str, &String, bool>(format!("ping:{}", url), &[
            ("url", url),
            ("time", &"0".to_string()),
            ("status", &"unknown".to_string())
        ])
            .expect("Failed to create new redis entry for url");
    }

    con.hgetall::<String, HashMap<String, String>>(format!("ping:{}", url))
        .expect("Failed to get data from redis server")
}

// updates the status of given url in the redis server
fn update_status(url: &String, status: &str) {
    let mut con = get_redis_con();
    let time = get_epoch();
    con.hset_multiple::<String, &str, &String, bool>(format!("ping:{}", url), &[
        ("url", url),
        ("time", &time.as_secs().to_string()),
        ("status", &status.to_string())
    ])
        .expect("Failed to update redis entry for url");
}

// returns the current UNIX_EPOCH as Duration
fn get_epoch() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("SystemTime before UNIX EPOCH!")
}

// pings given url and upon finish calls update_status with the result
async fn ping_url(url: &String, timeout: u64) {
    trace!("Pinging {} with timeout {}", url, timeout.to_string());

    // required for handling https requests
    let mut builder = SslConnector::builder(SslMethod::tls())
        .unwrap();

    // disable cert verification, due to lack of local cert
    builder.set_verify(SslVerifyMode::NONE);

    let client = Client::builder()
        .timeout(Duration::new(timeout, 0))
        .connector(Connector::new().ssl(builder.build()).finish())
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:88.0) Gecko/20100101 Firefox/88.0")
        .finish();

    let response = client.get(url).send().await;

    match response {
        Ok(res) => {
            debug!("Ping request of {} succeeded with {:?}", url, res);
            let status = &res.status().as_u16();
            let safe: &[u16] = &[200, 300, 301, 302, 307, 308];

            if safe.contains(status) {
                return update_status(url, "up");
            }

            let headers = res.headers();
            debug!("{} has headers: {:?}", url, headers);
            if headers.contains_key("Server") {
                let server = headers.get("Server")
                    .expect("Failed to parse Server response header")
                    .to_str().unwrap();
                debug!("Server of {} is {}", url, server);
                let unknown: &[u16] = &[401, 403, 503, 520];
                let forbidden: &u16 = &403;
                if unknown.contains(status) && server.eq("cloudflare") ||
                    status.eq(forbidden) && server.eq("ddos-guard") {
                    info!("Unknown HTTP status of {}: {}", url, status);
                    return update_status(url, "unknown");
                }
            }

            info!("{} is down: HTTP status {}", url, status);
            update_status(url, "down");
        }
        Err(e) => {
            // TODO: implement error catching, see:
            // https://docs.rs/actix-web/3.3.2/actix_web/client/enum.SendRequestError.html

            info!("Unexpected error occurred during ping of {}: {:?}", url, e);
            update_status(url, "down");
        }
    }
}

// background task, which checks when to update known entries of the redis server
async fn background_scan(interval: u64, timeout: u64) {
    debug!("Running background task");
    let mut con = get_redis_con();
    let mut urls = con.smembers::<&str, Vec<String>>("urls")
        .expect("Failed to retrieve urls from redis urls list");
    urls.retain(|url| {
        let t = con.hget::<String, &str, String>(format!("ping:{}", url), "time")
            .expect("Failed to retrieve urls from redis urls list")
            .parse::<u64>()
            .expect("Failed to convert string to u64");
        get_epoch().as_secs() - t > interval
    });
    info!("Found {} urls to be pinged", urls.len());
    if urls.len() > 0 {
        let p = urls.iter().map(|url| ping_url(url, timeout));
        for f in p {
            f.await;
        }
    }
}


// ------------------------------------------------------------------------------
// web service
// ------------------------------------------------------------------------------

#[get("/")]
async fn index() -> impl Responder {
    let url = env::var("CORS").expect("env CORS not found");
    debug!("Redirect to {}", url);
    HttpResponse::Found()
        .header("Location", url)
        .finish()
}

#[get("/health")]
async fn health() -> impl Responder {
    debug!("alive");
    HttpResponse::Ok().header("Content-Type", "text/html; charset=UTF-8").body("OK")
}

#[post("/ping")]
async fn ping(url: web::Json<Url>) -> Result<HttpResponse, Error> {
    debug!("Request ping of {}", url.url);
    Ok(HttpResponse::Ok().json(get_status(&url.url)))
}

#[post("/pings")]
async fn pings(urls: web::Json<Urls>) -> Result<HttpResponse, Error> {
    debug!("Request pings");
    let mut status: Vec<HashMap<String, String>> = Vec::new();
    for url in urls.urls.iter() {
        status.push(get_status(url));
    }

    Ok(HttpResponse::Ok().json(status))
}

#[actix_web::main]
async fn main() {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    info!("Starting webservice");
    HttpServer::new(|| {
        let cors = Cors::default()
            .allowed_origin("http://localhost:8080")
            .allowed_origin_fn(|origin, _| {
                let u = env::var("CORS")
                    .expect("env CORS not found");
                let cors_regex = regex::Regex::new(&*u).unwrap();
                match from_utf8(origin.as_bytes()) {
                    Ok(origin_utf8) => cors_regex.is_match(origin_utf8),
                    Err(_) => {
                        debug!("Could not decode origin string {:?}", origin);
                        false
                    }
                }
            })
            .allowed_methods(vec!["GET", "POST"])
            .allowed_header(http::header::CONTENT_TYPE)
            .max_age(3600);
        App::new()
            .wrap(cors)
            .service(index)
            .service(health)
            .service(ping)
            .service(pings)
    })
        .bind("0.0.0.0:5000")
        .expect("Failed to launch web service")
        .run();

    info!("Starting pingapi");
    let interval = env::var("INTERVAL")
        .expect("env INTERVAL not found")
        .parse::<u64>()
        .expect("Failed to convert INTERVAL to u64");
    let timeout = env::var("TIMEOUT")
        .expect("env TIMEOUT not found")
        .parse::<u64>()
        .expect("Failed to convert TIMEOUT to u64");
    loop {
        background_scan(interval, timeout).await;
        thread::sleep(Duration::from_secs(timeout * 2));
    }
}
