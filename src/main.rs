mod ping;
use actix_web::{
    get, post,
    web::{Data, Json},
    App, HttpResponse, HttpServer, Responder,
};
use serde::Deserialize;

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
    return HttpResponse::Ok().json(
        ping::ping(
            &url.url,
            &redis_client,
            None,
        )
        .await,
    );
}

#[post("/pings")]
async fn ping_urls(urls: Json<Urls>, redis_client: Data<redis::Client>) -> impl Responder {
    return HttpResponse::Ok().json(
        ping::ping_multiple(
            &urls.urls,
            &redis_client,
        )
        .await,
    );
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
