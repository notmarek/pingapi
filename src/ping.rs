use serde::{Deserialize, Serialize};
use std::{
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

#[derive(Serialize, Debug)]
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
    // let redis_con = get_redis_connection(redis_client).await;
    // TODO: implement redis

    let client = reqwest_client.unwrap_or(new_reqwest_client(timeout).await);
    let mut w = Website {
        time: get_epoch().await.as_secs(),
        url: url.to_string(),
        status: Status::Unknown,
    };
    let res = client.get(url).send().await;

    let status = match res {
        Ok(r) => {
            let status = r.status();
            println!("URL: {}", url);

            println!("Status: {}", status);
            if status.is_success() || status.is_redirection() {
                Status::Up
            } else {
                let headers = r.headers();
                println!("Headers: {:#?}", headers);

                if headers.contains_key("server") {
                    let server = headers.get("server").unwrap().to_str().unwrap(); // we know that the header exists
                    println!("Server: {}, {}", server, server.eq("cloudflare"));
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
