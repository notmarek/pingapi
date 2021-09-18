mod ping;
use actix_cors::Cors;
use actix_web::{
    get, http, post,
    web::{Data, Json},
    App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use log::{debug, info};
use ping::get_redis_connection;
use redis::AsyncCommands;
use serde::Deserialize;
use std::{env, thread, time::Duration};

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

lazy_static::lazy_static! {
    static ref CORS: String = env::var("CORS").unwrap_or(String::from("https://piracy.moe"));
}

lazy_static::lazy_static! {
    static ref SECRET: String = env::var("SECRET").unwrap_or(String::from("i am not very secure ),:"));
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

async fn check_secret(req: HttpRequest) -> Option<HttpResponse> {
    match req.headers().get("authorization") {
        Some(header) => {
            if header.to_str().unwrap().ne(&*SECRET) {
                return Some(HttpResponse::Forbidden().finish());
            }
        }
        _ => return Some(HttpResponse::Forbidden().finish()),
    }
    None
}

#[post("/add")]
async fn add(
    data: Json<Urls>,
    redis_client: Data<redis::Client>,
    req: HttpRequest,
) -> impl Responder {
    match check_secret(req).await {
        Some(e) => return e,
        _ => {}
    }

    let mut con = get_redis_connection(&redis_client).await;
    for url in data.0.urls {
        con.sadd::<&str, &String, bool>("urls", &url)
            .await
            .expect("Failed to add url to redis urls list");
    }

    HttpResponse::Ok().finish()
}

#[post("/remove")]
async fn remove(
    data: Json<Urls>,
    redis_client: Data<redis::Client>,
    req: HttpRequest,
) -> impl Responder {
    match check_secret(req).await {
        Some(e) => return e,
        _ => {}
    }

    let mut con = get_redis_connection(&redis_client).await;
    for url in data.0.urls {
        con.srem::<&str, &String, bool>("urls", &url)
            .await
            .expect("Failed to add url to redis urls list");
    }

    HttpResponse::Ok().finish()
}

#[get("/health")]
async fn health() -> impl Responder {
    HttpResponse::Ok().body("OK")
}

#[get("/")]
async fn index() -> impl Responder {
    HttpResponse::PermanentRedirect().with_header(("Location", (*CORS).clone()))
}

#[post("/ping")]
async fn ping_url(url: Json<Url>, redis_client: Data<redis::Client>) -> impl Responder {
    if let Some(data) = ping::get_status(&url.url, &redis_client).await {
        HttpResponse::Ok().json(data)
    } else {
        HttpResponse::NotFound().finish()
    }
}

#[post("/pings")]
async fn ping_urls(urls: Json<Urls>, redis_client: Data<redis::Client>) -> impl Responder {
    HttpResponse::Ok().json(ping::get_status_multiple(&urls.urls, &redis_client).await)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    actix_rt::spawn(async {
        info!("Spawning background task");
        let redis_client = redis::Client::open("redis://127.0.0.1/").unwrap();
        loop {
            debug!("Running ping loop.");
            ping::background(&redis_client, *TIMEOUT).await;
            thread::sleep(Duration::from_secs(*INTERVAL));
        }
    });
    info!("Starting webservice");
    HttpServer::new(|| {
        let redis_client = redis::Client::open("redis://127.0.0.1/").unwrap();
        let cors = Cors::default()
            .allowed_origin("http://localhost:8080")
            .allowed_origin_fn(|origin, _| {
                let cors_regex = regex::Regex::new(&*CORS).unwrap();
                match String::from_utf8(origin.as_bytes().to_vec()) {
                    Ok(origin_utf8) => cors_regex.is_match(&origin_utf8),
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
            .service(health)
            .service(ping_url)
            .service(ping_urls)
            .service(index)
            .service(add)
            .service(remove)
            .app_data(Data::new(redis_client))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
