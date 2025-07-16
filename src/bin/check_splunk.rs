use std::collections::HashMap;
use std::num::NonZeroU16;
use std::time::Duration;

use clap::*;
use reqwest::ClientBuilder;
use serde::Deserialize;

#[cfg(not(tarpaulin_include))]
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
    #[clap(long, short, env = "SPLUNK_AUTHTOKEN")]
    /// The Splunk auth token to use
    authtoken: Option<String>,

    #[clap(long, short)]
    /// Show more logs
    debug: bool,
}

#[cfg(not(tarpaulin_include))]
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
/// Used to parse a field from a Splunk search result
struct SplunkField {
    pub name: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
/// Search messages
struct SearchMessage {
    #[serde(alias = "type")]
    type_: String,
    text: String,
}

#[cfg(not(tarpaulin_include))]
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
/// Used to parse a search result from Splunk
struct SearchResult {
    #[serde(default)]
    pub preview: Option<bool>,
    #[serde(default)]
    pub post_process_count: Option<u64>,
    #[serde(default)]
    pub init_offset: Option<u64>,
    #[serde(default)]
    pub messages: Vec<SearchMessage>,
    #[serde(default)]
    pub fields: Vec<SplunkField>,
    #[serde(default)]
    pub results: Vec<serde_json::Value>,
}

#[cfg(not(tarpaulin_include))]
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
            format!("index IN ( {indexes} )")
        }
        true => "index IN (_*, *)".to_string(),
    };

    let sourcetype_stmt = match args.sourcetypes.is_empty() {
        false => {
            let sourcetypes = &args
                .sourcetypes
                .iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<String>>()
                .join(",");
            format!("sourcetype IN ( {sourcetypes} ) ")
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
        time_message.push_str(&format!("over {earliest}"));
    } else if let Some(lookback) = &args.lookback {
        payload.insert(
            "earliest_time".to_string(),
            format!("-{}h", lookback.parse::<u64>().unwrap_or(24)),
        );
        time_message.push_str(&format!("over -{lookback} hours"));
    }

    if let Some(latest) = &args.latest {
        payload.insert("latest_time".to_string(), latest.to_string());
    }

    if args.debug {
        eprintln!("{search}");
    }

    let client = client.build().expect("Failed to build client");
    if args.debug {
        eprintln!("payload: {payload:#?}");
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
        eprintln!("Sending request to splunk at {url}");
    }
    let res = match res {
        Err(err) => {
            eprint!(
                "CRITICAL: Failed to send request to splunk at {url}: {err:?}"
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
                "CRITICAL: Failed to get response body from Splunk: {err:?}"
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
                        eprintln!("Failed to parse this to a search result: {l:?}");
                        None
                    }
                }
            }
        })
        .collect();

    if args.debug {
        eprintln!("{results:#?}");
    }
    let result = match results.into_iter().next() {
        Some(r) => r,
        None => {
            eprintln!("CRITICAL: Expected 1 result, got 0");
            std::process::exit(1)
        }
    };
    let sourcetype_log = match &args.sourcetypes.is_empty() {
        false => format!(" sourcetype IN ({})", args.sourcetypes.join(",")),
        true => String::new(),
    };

    let result_entry = match result.results.into_iter().next() {
        Some(val) => val,
        None => {
            print!(
                "CRITICAL: No results found for host={}{}",
                args.host, sourcetype_log
            );
            std::process::exit(1)
        }
    };
    #[derive(Deserialize)]
    struct GetCount {
        count: String,
        #[serde(flatten)]
        _rest: serde_json::Value,
    }

    let count = match serde_json::from_value::<GetCount>(result_entry) {
        Ok(val) => val.count,
        Err(err) => {
            print!("CRITICAL: Failed to parse count from result: {err:?}");
            std::process::exit(1)
        }
    };

    print!(
        "OK: Found host={} count={} {}{}",
        args.host, count, sourcetype_log, time_message
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_searchmessage() {
        let input = r#"{"messages":[{"type":"WARN","text":"Search not executed: The maximum number of concurrent historical searches on this instance has been reached., concurrency_category=\"historical\", concurrency_context=\"instance-wide\", current_concurrency=10, concurrency_limit=10","help":""}]}"#;

        let result: SearchResult =
            serde_json::from_str(input).expect("Failed to deserialize messages");
        assert_eq!(result.messages.len(), 1);
    }
}
