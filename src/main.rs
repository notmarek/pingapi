#![feature(proc_macro_hygiene, decl_macro)]

extern crate redis;
#[macro_use]
extern crate rocket;

use std::env;

use redis::Commands;
use rocket::config::Array;
use rocket::response::Redirect;
use rocket_cors::{AllowedOrigins, Cors, CorsOptions};
use serde_redis::RedisDeserialize;

use rocket_contrib::json::{Json, JsonValue};
use serde::Deserialize;

#[derive(Deserialize)]
struct Site {
    url: String,
    time: String,
    status: String,
}

fn get_status(&url: String) -> Site {
    let client = redis::Client::open("redis://127.0.0.1/");
    let mut con = client.get_connection();

    if !con.exists("ping:" + url).unwrap() {
        con.sadd("urls", url);
        con.hset_multiple("ping:" + url, &[("url", url), ("status", "unknown"), ("time", "0")]);
    }

    let result: Site = con.hgetall("ping:" + url).deserialize();
    return result
}

#[get("/health")]
fn health() -> &'static str {
    "OK"
}

#[post("/ping", format = "json", data = "<urls>")]
fn ping(urls: Json<Vec<Site>>) -> JsonValue {
    let urls_vec = urls.lock().await;
    let mut status: Vec<Site> = Vec::new();
    for url in urls_vec.iter() {
        status.push(get_status(url));
    }
    json!(status)
}

#[get("/")]
fn index() -> Redirect {
    Redirect::to(env::var("CORS"))
}

fn main() {
    let cors = CorsOptions::default()
        .allowed_origins(AllowedOrigins::some_exact(&[env::var("CORS"), "http://localhost"]));
    rocket::ignite()
        .mount("/", routes![health, ping, index])
        .attach(cors)
        .launch();
}