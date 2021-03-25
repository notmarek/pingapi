extern crate redis;

use std::collections::HashMap;
use std::{env, thread};
use redis::{Commands};
use log::{info, trace, error, debug};
use futures::future::join_all;

use actix_cors::Cors;
use actix_web::{App, Error, get, HttpResponse, HttpServer, post, Responder, web, http};
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use reqwest::Response;

#[derive(Deserialize)]
struct Url {
    url: String
}

#[derive(Deserialize)]
struct Urls {
    urls: Vec<String>
}

fn get_redis_con() -> redis::Connection {
    let client = redis::Client::open("redis://127.0.0.1")
        .expect("Cannot connect to local redis server");
    client.get_connection()
        .expect("Connection to redis server failed")
}

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

fn get_epoch() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("SystemTime before UNIX EPOCH!")
}

async fn ping_url(url: &String, timeout: u64) {
    trace!("Pinging {} with timeout {}", url, timeout);

    let client = reqwest::Client::new();
    let req = client.head(url)
        .timeout(Duration::new(timeout, 0))
        .send()
        .await;

    match req {
        Ok(res) => {
            eval_response(url, res);
        }
        Err(e) => {
            let mut unknown_error = false;
            if e.is_timeout() {
                info!("Timeout of ping {}", url);
            } else if e.is_builder() {
                error!("Builder of ping {} failed", url);
            } else if e.is_request() {
                info!("Request failure of ping {}", url);
            } else if e.is_connect() {
                info!("Connection failure of ping {}", url);
            } else if e.is_decode() {
                info!("Decode failure of ping {}", url);
            } else if e.is_redirect() {
                if let Some(final_stop) = e.url() {
                    info!("Ping {} has redirect loop at {}", url, final_stop);
                } else {
                    unknown_error = true;
                }
            } else {
                unknown_error = true;
            }

            if unknown_error {
                info!("Unexpected error occurred during ping of {}", url);
            }
            update_status(url, "down");
        }
    }
}

fn eval_response(url: &String, res: Response) {
    let status = &res.status().as_u16();
    let safe: &[u16] = &[200, 300, 301, 302, 307, 308];

    if safe.contains(status) {
        return update_status(url, "up");
    }

    let headers = res.headers();
    if headers.contains_key("Server") {
        let server = headers["Server"].to_str()
            .expect("Failed to parse Server response header");
        let unknown: &[u16] = &[401, 403, 503, 520];
        let forbidden: &u16 = &403;
        if unknown.contains(status) && server.eq("cloudflare") ||
            status.eq(forbidden) && server.eq("ddos-guard") {
            return update_status(url, "unknown");
        }
    }

    update_status(url, "down");
}

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
    if urls.len() > 0 {
        let p = urls.iter().map(|url| ping_url(url, timeout));
        join_all(p).await;
    }
}

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
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("debug"));

    info!("Starting webservice");
    HttpServer::new(|| {
        let cors = Cors::default()
            .allowed_origin("http://localhost")
            .allowed_origin("https://piracy.moe")
            .allowed_origin_fn(|origin, _req_head| {
                let u = env::var("CORS")
                    .expect("env CORS not found");
                origin.as_bytes().starts_with(u.as_bytes())
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