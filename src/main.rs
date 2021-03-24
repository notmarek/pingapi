extern crate redis;

use std::collections::HashMap;
use std::env;

use actix_cors::Cors;
use actix_web::{App, Error, get, HttpResponse, HttpServer, post, Responder, web, http};
use serde::Deserialize;

#[derive(Deserialize)]
struct Url {
    url: String
}

#[derive(Deserialize)]
struct Urls {
    urls: Vec<String>
}

fn get_status(url: &String) -> HashMap<String, String> {
    use redis::Commands;

    let client = redis::Client::open("redis://127.0.0.1")
        .expect("Cannot connect to local redis server");
    let mut con = client.get_connection()
        .expect("Connection to redis server failed");

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

    let data: HashMap<String, String> = con.hgetall(format!("ping:{}", url))
        .expect("Failed to get data from redis server");
    data
}

#[get("/")]
async fn index() -> impl Responder {
    HttpResponse::Found()
        .header("Location", env::var("CORS").expect("env CORS not found"))
        .finish()
}

#[get("/health")]
async fn health() -> impl Responder {
    HttpResponse::Ok().body("OK")
}

#[post("/ping")]
async fn ping(url: web::Json<Url>) -> Result<HttpResponse, Error> {
    Ok(HttpResponse::Ok().json(get_status(&url.url)))
}

#[post("/pings")]
async fn pings(urls: web::Json<Urls>) -> Result<HttpResponse, Error> {
    let mut status: Vec<HashMap<String, String>> = Vec::new();
    for url in urls.urls.iter() {
        status.push(get_status(url));
    }

    Ok(HttpResponse::Ok().json(status))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        let cors = Cors::default()
            .allowed_origin("http://localhost")
            .allowed_origin("https://piracy.moe")
            .allowed_origin_fn(|origin, _req_head| {
                let u = env::var("CORS").expect("env CORS not found");
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
        .bind("0.0.0.0:5000")?
        .run()
        .await
}