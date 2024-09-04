use std::collections::HashMap;
use std::num::NonZeroU16;
use std::time::Duration;

use clap::*;
use reqwest::ClientBuilder;
use serde::Deserialize;

#[derive(Parser)]
struct Cli {
    /// The Splunk instance to connect to
    splunk_host: String,
    /// The API port on the Splunk instance to connect to, defaults to 8089
    #[clap(long, short)]
    port: Option<NonZeroU16>,
    /// The host entry to search for
    host: String,

    /// Look back this many hours from now, defaults to 24
    #[clap(long)]
    lookback: Option<String>,

    /// Earliest time, use Unix seconds
    #[clap(long)]
    earliest: Option<u64>,

    /// The earliest time to search for, defaults to "now"
    #[clap(short, long)]
    latest: Option<String>,
    /// The indexes to search for, defaults to *
    #[clap(short, long)]
    indexes: Vec<String>,
    /// The sourcetypes to search for, defaults to *
    #[clap(short, long)]
    sourcetypes: Vec<String>,
    #[clap(long, short = 'D')]
    /// In case your Splunk instance uses a self-signed certificate
    disable_tls_verify: bool,

    #[clap(long, short)]
    /// Timeout in seconds for the search, defaults to 30
    timeout: Option<u64>,

    #[clap(long, env = "SPLUNK_USERNAME")]
    /// The Splunk username to use
    username: Option<String>,

    #[clap(long, env = "SPLUNK_PASSWORD")]
    /// The Splunk password to use
    password: Option<String>,
    #[clap(long, short, env = "SPLUNK_TOKEN")]
    /// The Splunk auth token to use
    authtoken: Option<String>,

    #[clap(long, short)]
    /// Show more logs
    debug: bool,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SplunkField {
    pub name: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SearchResult {
    pub preview: Option<bool>,
    pub init_offset: Option<u64>,
    pub messages: Vec<String>,
    pub fields: Vec<SplunkField>,
    pub results: Vec<serde_json::Value>,
}

#[tokio::main]
/// Run a Splunk search looking for a host
async fn main() -> Result<(), String> {
    let args = Cli::parse();

    let mut client =
        ClientBuilder::new().user_agent(format!("Maremma/{}", env!("CARGO_PKG_VERSION")));
    if args.disable_tls_verify {
        client = client
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true);
    }

    let port = args.port.unwrap_or(NonZeroU16::new(8089).ok_or(
        "Failed to parse 8089 into a non-zero u16, this is an internal error".to_string(),
    )?);

    let url = format!(
        "https://{}:{}/services/search/jobs/",
        args.splunk_host, port
    );

    let index_stmt = match args.indexes.is_empty() {
        false => {
            let indexes = &args
                .indexes
                .into_iter()
                .map(|i| format!("\"{i}\""))
                .collect::<Vec<String>>()
                .join(",");
            format!("index IN ( {} )", indexes)
        }
        true => "index IN (_*, *)".to_string(),
    };

    let sourcetype_stmt = match args.sourcetypes.is_empty() {
        false => {
            let sourcetypes = &args
                .sourcetypes
                .into_iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<String>>()
                .join(",");
            format!("sourcetype IN ( {} ) ", sourcetypes)
        }
        true => "sourcetype=*".to_string(),
    };

    let search = format!(
        "| tstats count where host=\"{}\" {} {} by host | table count, host | search host=\"{}\"",
        args.host, index_stmt, sourcetype_stmt, args.host
    );

    let mut payload: HashMap<String, String> = HashMap::from_iter([
        ("search".to_string(), search.clone()),
        ("output_mode".to_string(), "json".to_string()),
        ("exec_mode".to_string(), "oneshot".to_string()),
    ]);

    let mut time_message = String::new();

    if let Some(earliest) = &args.earliest {
        payload.insert("earliest_time".to_string(), earliest.to_string());
        time_message.push_str(&format!("over {}", earliest));
    } else if let Some(lookback) = &args.lookback {
        payload.insert(
            "earliest_time".to_string(),
            format!("-{}h", lookback.parse::<u64>().unwrap_or(24)),
        );
        time_message.push_str(&format!("over -{} hours", lookback));
    }

    if let Some(latest) = &args.latest {
        payload.insert("latest_time".to_string(), latest.to_string());
    }

    if args.debug {
        eprintln!("{search}");
    }

    let client = client.build().expect("Failed to build client");
    if args.debug {
        eprintln!("payload: {:#?}", payload);
    }
    let mut request = client
        .post(&url)
        .form(&payload)
        .timeout(Duration::from_secs(args.timeout.unwrap_or(30)));

    if let (Some(username), Some(password)) = (args.username, args.password) {
        request = request.basic_auth(username, Some(password));
    } else if let Some(token) = args.authtoken {
        request = request.bearer_auth(token);
    }

    let res = request.send().await;

    if args.debug {
        eprintln!("Sending request to splunk at {}", url);
    }
    let res = match res {
        Err(err) => {
            eprint!(
                "CRITICAL: Failed to send request to splunk at {}: {:?}",
                url, err
            );
            std::process::exit(1)
        }
        Ok(res) => res,
    };

    if res.status().is_client_error() {
        if res.status() == 401 {
            print!("CRITICAL: Received 401 from Splunk, check your credentials");
            std::process::exit(1)
        }
        println!(
            "CRITICAL: Received client error from Splunk: {:?} - body {}",
            res.status(),
            res.text().await.expect("Failed to get response body")
        );
        std::process::exit(1)
    }

    let body = match res.text().await {
        Ok(val) => val,
        Err(err) => {
            print!(
                "CRITICAL: Failed to get response body from Splunk: {:?}",
                err
            );
            std::process::exit(1)
        }
    };

    let results: Vec<SearchResult> = body
        .lines()
        .filter_map(|l| {
            if l.trim().is_empty() {
                None
            } else {
                match serde_json::from_str(l.trim()) {
                    Ok(r) => Some(r),
                    Err(_) => {
                        eprintln!("Failed to parse this to a search result: {:?}", l);
                        None
                    }
                }
            }
        })
        .collect();

    if args.debug {
        println!("{:#?}", results);
    }
    if results.len() != 1 {
        print!("CRITICAL: Expected 1 result, got {}", results.len());
        std::process::exit(1)
    }
    print!("OK: Found host {} {}", args.host, time_message);
    Ok(())
}