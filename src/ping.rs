use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    time::{Duration, SystemTime},
};

#[derive(Copy, Clone, Debug)]
pub enum Status {
    Up,
    Down,
    Unknown,
}

impl Status {
    fn to_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Unknown => "unknown",
        }
    }

    fn from_str(str: &str) -> Self {
        if str == "up" {
            Self::Up
        } else if str == "down" {
            Self::Down
        } else {
            Self::Unknown
        }
    }
}

impl ::serde::Serialize for Status {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ::serde::Serializer,
    {
        serializer.serialize_str(self.to_str())
    }
}
impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.to_str().fmt(f)
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct Website {
    pub time: u64,
    pub url: String,
    pub status: Status,
}

const REAL_CF_DOWN: &[u16] = &[500, 502, 504, 520, 521, 522, 523, 524, 525, 526, 527]; // also 530 but that also includes ip bans and all other 1xxx errors

async fn get_redis_connection(client: &redis::Client) -> redis::aio::Connection {
    client.get_async_connection().await.unwrap()
}

async fn get_epoch() -> Duration {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("SystemTime before UNIX EPOCH!")
}

async fn update_status(website_status: Website, client: &redis::Client) {
    let mut con = get_redis_connection(client).await;
    con.hset_multiple::<String, &str, &String, bool>(
        format!("ping:{}", website_status.url),
        &[
            ("url", &website_status.url),
            ("time", &website_status.time.to_string()),
            ("status", &website_status.status.to_string()),
        ],
    )
    .await
    .expect("Failed to update redis entry for url");
}

async fn get_last_status(
    url: &str,
    client: &redis::Client,
) -> Result<Website, Box<dyn std::error::Error>> {
    let mut con = get_redis_connection(client).await;
    let ex: bool = con.exists(format!("ping:{}", url)).await?;
    if !ex {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "url not found in redis",
        )));
    }
    let raw_data = con
        .hgetall::<String, HashMap<String, String>>(format!("ping:{}", url))
        .await?;
    Ok(Website {
        url: url.to_string(),
        time: raw_data.get("time").unwrap().parse::<u64>()?,
        status: Status::from_str(raw_data.get("status").unwrap()),
    })
}

async fn new_reqwest_client(timeout: u64) -> reqwest::Client {
    let proxy_ip = env::var("PROXY_IP").unwrap_or_default();
    let proxy_user = env::var("PROXY_USER").unwrap_or_default();
    let proxy_pass = env::var("PROXY_PASS").unwrap_or_default();

    let mut builder = reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .timeout(Duration::from_millis(timeout));

    if !proxy_ip.is_empty() {
        let mut proxy = reqwest::Proxy::all(proxy_ip).expect("proxy should exist");
        if !proxy_user.is_empty() && !proxy_user.is_empty() {
            proxy = proxy.basic_auth(&proxy_user, &proxy_pass);
        }
        builder = builder.proxy(proxy);
    }

    builder.build().unwrap()
}

pub async fn ping_multiple(
    urls: &[String],
    redis_client: &redis::Client,
    timeout: u64,
) -> Vec<Website> {
    let client = new_reqwest_client(timeout).await;
    let mut websites: Vec<Website> = vec![];
    for url in urls {
        websites.push(ping(url, redis_client, Some(client.clone()), timeout).await)
    }
    websites
}

pub async fn ping(
    url: &str,
    redis_client: &redis::Client,
    reqwest_client: Option<reqwest::Client>,
    timeout: u64,
) -> Website {
    let time = get_epoch().await.as_secs();
    if let Ok(last_status) = get_last_status(url, redis_client).await {
        if time - *crate::INTERVAL < last_status.time {
            return last_status;
        }
    }
    let client = reqwest_client.unwrap_or(new_reqwest_client(timeout).await);
    let mut w = Website {
        time,
        url: url.to_string(),
        status: Status::Unknown,
    };
    let res = client.get(url).send().await;

    let status = match res {
        Ok(r) => {
            let status = r.status();
            if status.is_success() || status.is_redirection() {
                Status::Up
            } else {
                let headers = r.headers();
                if headers.contains_key("server") {
                    let server = headers.get("server").unwrap().to_str().unwrap(); // we know that the header exists
                    if (server.eq("cloudflare") && !REAL_CF_DOWN.contains(&status.as_u16()))
                        || server.eq("ddos-guard")
                    {
                        flaresolverr_ping(url, client, timeout).await
                    } else {
                        Status::Down
                    }
                } else {
                    Status::Down
                }
            }
        }
        Err(_) => Status::Down,
    };
    w.status = status;
    update_status(w.clone(), redis_client).await;
    w
}

async fn flaresolverr_ping(url: &str, client: reqwest::Client, timeout: u64) -> Status {
    let flaresolverr_endpoint = env::var("FLARESOLVERR");
    #[derive(Serialize)]
    struct FSRequest {
        cmd: String,
        url: String,
        #[serde(rename = "maxTimeout")]
        timeout: u64,
    }

    #[derive(Deserialize)]
    struct FSResponse {
        status: String,
    }
    if let Ok(e) = flaresolverr_endpoint {
        let res: FSResponse = client
            .post(e)
            .json(&FSRequest {
                cmd: String::from("request.get"),
                url: url.to_string(),
                timeout,
            })
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if &res.status == "ok" {
            return Status::Up;
        }
    }

    Status::Unknown
}
