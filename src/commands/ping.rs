use crate::commands::bot_init::ChatRequest;
use crate::commands::errors::BotPingError;
use lazy_regex::regex;
use pinger::PingResult;
use std::net::Ipv4Addr;
use teloxide::prelude::*;
use url::Url;

static IP_RE: &lazy_regex::Lazy<lazy_regex::Regex> =
    regex!(r"^\d{1,3}[.]\d{1,3}[.]\d{1,3}[.]\d{1,3}$");

pub struct PingCmd<'cr, 'host> {
    pub chat_request: &'cr ChatRequest,
    pub host: &'host str,
}

impl PingCmd<'_, '_> {
    pub fn new<'a>(chat_request: &'a ChatRequest, host: &'a str) -> PingCmd<'a, 'a> {
        PingCmd { chat_request, host }
    }

    pub async fn respond(&self) -> Result<Message, teloxide::RequestError> {
        self.chat_request
            .bot
            .send_message(self.chat_request.msg.chat.id, self.ping())
            .await
    }

    fn ping_formatter(&self, ping_result: PingResult) -> String {
        match ping_result {
            PingResult::Pong(duration, line) => format!("[ {:?} ] => {}", duration, line),
            PingResult::Timeout(_) => format!("Timeout!"),
            PingResult::Unknown(line) => format!("Unknown line: {}", line),
            PingResult::PingExited(_code, _stderr) => format!("code: {} err: {}", _code, _stderr),
        }
    }

    fn extract_host_from_url(user_input: &str) -> Result<String, BotPingError> {
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

    fn extract_host_from_ip(user_input: &str) -> Result<String, BotPingError> {
        match String::from(user_input).parse::<Ipv4Addr>() {
            Ok(_) => Ok(String::from(user_input)),
            Err(_) => Err(BotPingError::NotAHost),
        }
    }

    fn handle_ping_input(&self, user_input: &str) -> Result<String, BotPingError> {
        match String::from(user_input.trim()) {
            host if host.contains("://") => Self::extract_host_from_url(self.host),
            host if IP_RE.is_match(&host) => Self::extract_host_from_ip(&self.host),
            host => Ok(host),
        }
    }

    fn ping(&self) -> String {
        // Take care of input
        self.handle_ping_input(&self.host)
            .map_err(BotPingError::from)
            // make ping happen
            .map(|input| pinger::ping(input, None).map_err(BotPingError::from))
            .map_err(BotPingError::from)
            .and_then(|flaten| flaten)
            // extract reciever
            .map(|response| response.recv().map_err(BotPingError::from))
            .and_then(|flaten| flaten)
            // formatting ping
            .map(|result| self.ping_formatter(result))
            // here we have Result<String, BotPingError> so `unwrap()` is safe.
            .unwrap()
    }
}
