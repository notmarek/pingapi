use serde::Serialize;
use std::env;

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
    pub time: i64,
    pub url: String,
    pub status: Status,
}

const REAL_CF_DOWN: &[u16] = &[500, 502, 504, 520, 521, 522, 523, 524, 525, 526, 527]; // also 530 but that also includes ip bans and all other 1xxx errors

async fn get_redis_connection(client: &redis::Client) -> redis::aio::Connection {
    client.get_async_connection().await.unwrap()
}

async fn new_reqwest_client() -> reqwest::Client {
    let proxy_ip = env::var("PROXY_IP").unwrap_or(String::new());
    let proxy_user = env::var("PROXY_USER").unwrap_or(String::new());
    let proxy_pass = env::var("PROXY_PASS").unwrap_or(String::new());

    let mut builder = reqwest::Client::builder().user_agent(crate::USER_AGENT);

    if !proxy_ip.is_empty() {
        let mut proxy = reqwest::Proxy::all(proxy_ip).expect("proxy should exist");
        if !proxy_user.is_empty() && !proxy_user.is_empty() {
            proxy = proxy.basic_auth(&proxy_user, &proxy_pass);
        }
        builder = builder.proxy(proxy);
    }

    builder.build().unwrap()
}

pub async fn ping_multiple(urls: &Vec<String>, redis_client: &redis::Client) -> Vec<Website> {
    let client = new_reqwest_client().await;
    let mut websites: Vec<Website> = vec![];
    for url in urls {
        websites.push(ping(url, redis_client, Some(client.clone())).await)
    }
    return websites
}

pub async fn ping(
    url: &String,
    redis_client: &redis::Client,
    reqwest_client: Option<reqwest::Client>,
) -> Website {
    // let redis_con = get_redis_connection(redis_client).await; 
    // TODO: implement redis
    
    let client = reqwest_client.unwrap_or(new_reqwest_client().await);
    let mut w = Website {
        time: 0,
        url: url.clone(),
        status: Status::Unknown,
    };
    let res = client.get(url).send().await;

    match res {
        Ok(r) => {
            let status = r.status();
            println!("URL: {}", url);

            println!("Status: {}", status);
            if status.is_success() || status.is_redirection() {
                w.status = Status::Up;
                return w
            } else {
                let headers = r.headers();
                println!("Headers: {:#?}", headers);

                if headers.contains_key("server") {
                    let server = headers.get("server").unwrap().to_str().unwrap(); // we know that the header exists
                    println!("Server: {}, {}", server, server.eq("cloudflare"));
                    if (server.eq("cloudflare") && !REAL_CF_DOWN.contains(&status.as_u16())) || server.eq("ddos-guard") {
                        // TODO: Use flaresolverr
                        w.status = Status::Unknown;
                        return w
                    }
                }
                w.status = Status::Down;
                return w
            }
        }
        Err(_) => w.status = Status::Down,
    }

    return w;
}
