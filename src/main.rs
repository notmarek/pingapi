mod ping;
use actix_web::{
    get, post,
    web::{Data, Json},
    App, HttpResponse, HttpServer, Responder,
};
use serde::Deserialize;
use std::env;

lazy_static::lazy_static! {
    static ref TIMEOUT: u64 = match env::var("TIMEOUT") {
        Ok(t) => {
            t.parse().unwrap_or(10000)
        }
        Err(_) => {
            10000
        }
    };
}

lazy_static::lazy_static! {
    pub static ref INTERVAL: u64 = match env::var("INTERVAL") {
        Ok(t) => {
            t.parse().unwrap_or(300)
        }
        Err(_) => {
            300 // 5 minutes
        }
    };
}

#[derive(Deserialize)]
struct Url {
    url: String,
}

#[derive(Deserialize)]
struct Urls {
    urls: Vec<String>,
}

pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:88.0) Gecko/20100101 Firefox/88.0";

#[get("/health")]
async fn health() -> impl Responder {
    HttpResponse::Ok().body("OK")
}

#[post("/ping")]
async fn ping_url(url: Json<Url>, redis_client: Data<redis::Client>) -> impl Responder {
    return HttpResponse::Ok().json(ping::ping(&url.url, &redis_client, None, *TIMEOUT).await);
}

#[post("/pings")]
async fn ping_urls(urls: Json<Urls>, redis_client: Data<redis::Client>) -> impl Responder {
    return HttpResponse::Ok().json(ping::ping_multiple(&urls.urls, &redis_client, *TIMEOUT).await);
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        let redis_client = redis::Client::open("redis://127.0.0.1/").unwrap();
        App::new()
            .service(health)
            .service(ping_url)
            .service(ping_urls)
            .app_data(Data::new(redis_client))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
