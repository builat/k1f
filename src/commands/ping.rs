use crate::commands::errors::BotPingError;
use lazy_regex::regex;
use pinger::PingResult;
use std::net::Ipv4Addr;
use url::Url;

static IP_RE: &lazy_regex::Lazy<lazy_regex::Regex> =
    regex!(r"^\d{1,3}[.]\d{1,3}[.]\d{1,3}[.]\d{1,3}$");

fn ping_formatter(ping_result: PingResult) -> String {
    match ping_result {
        PingResult::Pong(duration, line) => format!("[ {:?} ] => {}", duration, line),
        PingResult::Timeout(_) => format!("Timeout!"),
        PingResult::Unknown(line) => format!("Unknown line: {}", line),
        PingResult::PingExited(_code, _stderr) => format!("code: {} err: {}", _code, _stderr),
    }
}

fn extract_host_from_url(user_input: String) -> Result<String, BotPingError> {
    fn match_input_result_option(parsed: Url) -> String {
        match parsed.host_str() {
            Some(result) => result,
            None => "No target host found",
        }
        .to_string()
    }

    Url::parse(&user_input)
        .map(match_input_result_option)
        .map_err(|_| BotPingError::BadUrl)
}

fn extract_host_from_ip(user_input: &String) -> Result<String, BotPingError> {
    match String::from(user_input).parse::<Ipv4Addr>() {
        Ok(_) => Ok(String::from(user_input)),
        Err(_) => Err(BotPingError::NotAHost),
    }
}

fn handle_ping_input(user_input: &str) -> Result<String, BotPingError> {
    match String::from(user_input.trim()) {
        host if host.contains("://") => extract_host_from_url(host),
        host if IP_RE.is_match(&host) => extract_host_from_ip(&host),
        host => Ok(host),
    }
}

pub fn ping(addr: &str) -> String {
    // Take care of input
    handle_ping_input(addr)
        .map_err(BotPingError::from)
        // make ping happen
        .map(|input| pinger::ping(input, None).map_err(BotPingError::from))
        .map_err(BotPingError::from)
        .and_then(|flaten| flaten)
        // extract reciever
        .map(|response| response.recv().map_err(BotPingError::from))
        .and_then(|flaten| flaten)
        // formatting ping
        .map(|result| ping_formatter(result))
        // here we have Result<String, BotPingError> so `unwrap()` is safe.
        .unwrap()
}
