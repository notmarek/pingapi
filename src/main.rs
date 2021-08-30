extern crate redis;

use actix_cors::Cors;
use actix_web::{get, http, post, web, App, Error, HttpResponse, HttpServer, Responder};
use log::{debug, info, trace};
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use redis::Commands;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::str::from_utf8;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{env, thread};

#[derive(Deserialize)]
struct Url {
    url: String,
}

#[derive(Deserialize)]
struct Urls {
    urls: Vec<String>,
}

// ------------------------------------------------------------------------------
// background tasks
// ------------------------------------------------------------------------------

// returns a new connection to redis server
fn get_redis_con() -> redis::Connection {
    let client =
        redis::Client::open("redis://127.0.0.1").expect("Cannot connect to local redis server");
    client
        .get_connection()
        .expect("Connection to redis server failed")
}

// fetches the current status of given url from the redis server
fn get_status(url: &String) -> HashMap<String, String> {
    let mut con = get_redis_con();
    let ex: bool = con
        .exists(format!("ping:{}", url))
        .expect("Failed to determine existence of url");
    if !ex {
        con.sadd::<&str, &String, bool>("urls", url)
            .expect("Failed to add url to redis urls list");
        con.hset_multiple::<String, &str, &String, bool>(
            format!("ping:{}", url),
            &[
                ("url", url),
                ("time", &"0".to_string()),
                ("status", &"unknown".to_string()),
            ],
        )
        .expect("Failed to create new redis entry for url");
    }

    con.hgetall::<String, HashMap<String, String>>(format!("ping:{}", url))
        .expect("Failed to get data from redis server")
}

// updates the status of given url in the redis server
fn update_status(url: &String, status: &str) {
    let mut con = get_redis_con();
    let time = get_epoch();
    con.hset_multiple::<String, &str, &String, bool>(
        format!("ping:{}", url),
        &[
            ("url", url),
            ("time", &time.as_secs().to_string()),
            ("status", &status.to_string()),
        ],
    )
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
    let socks_ip = env::var("SOCKS_IP").unwrap_or(String::new());
    let socks_user = env::var("SOCKS_USER").unwrap_or(String::new());
    let socks_pass = env::var("SOCKS_PASS").unwrap_or(String::new());
    let mut builder = Client::builder();

    if !socks_user.is_empty() && !socks_pass.is_empty() && !socks_ip.is_empty() {
        let proxy = reqwest::Proxy::all(&format!("{}", socks_ip)).expect("proxy should exist");
        builder = builder.proxy(proxy.basic_auth(&socks_user, &socks_pass));
    }

    let client = builder.user_agent("Mozilla/5.0 (piracy.moe; Windows NT 10.0; Win64; x64; rv:88.0) Gecko/20100101 Firefox/88.0").build().unwrap();

    let response = client.get(url).send().await;

    match response {
        Ok(res) => {
            debug!("Ping request of {} succeeded with {:?}", url, res);
            let status = res.status();
            let headers = res.headers();

            if status.is_success() || status.is_redirection() {
                return update_status(url, "up");
            }
            debug!("{} has headers {:?}", url, headers);
            if headers.contains_key("Server") {
                let server = headers
                    .get("Server")
                    .expect("Failed to parse Server response header")
                    .to_str()
                    .unwrap();
                debug!("Server of {} is {}", url, server);
                if (status.is_client_error() || status.is_server_error())
                    && server.eq("cloudflare")
                    && server.eq("ddos-guard")
                {
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
    let mut urls = con
        .smembers::<&str, Vec<String>>("urls")
        .expect("Failed to retrieve urls from redis urls list");
    urls.retain(|url| {
        let t = con
            .hget::<String, &str, String>(format!("ping:{}", url), "time")
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
    HttpResponse::Found().header("Location", url).finish()
}

#[get("/health")]
async fn health() -> impl Responder {
    debug!("Alive");
    HttpResponse::Ok().body("OK")
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
                let u = env::var("CORS").expect("env CORS not found");
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
