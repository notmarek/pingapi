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

pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:88.0) Gecko/20100101 Firefox/88.0";

async fn get_redis_connection(client: &redis::Client) -> redis::aio::Connection {
    client.get_async_connection().await.unwrap()
}

#[get("/health")]
async fn health() -> impl Responder {
    HttpResponse::Ok().body("OK")
}

#[post("/ping")]
async fn ping_url(url: Json<Url>, redis_client: Data<redis::Client>) -> impl Responder {
    return HttpResponse::Ok().json(
        ping::ping(
            &url.url,
            get_redis_connection(&redis_client).await,
            None,
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
            .app_data(Data::new(redis_client))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
