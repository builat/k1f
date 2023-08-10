use pinger::PingResult;
use url::Url;

fn ping_formatter(ping_result: PingResult) -> String {
    match ping_result {
        PingResult::Pong(duration, line) => format!("[ {:?} ] => {}", duration, line),
        PingResult::Timeout(_) => format!("Timeout!"),
        PingResult::Unknown(line) => format!("Unknown line: {}", line),
        PingResult::PingExited(_code, _stderr) => format!("code: {} err: {}", _code, _stderr),
    }
}

fn handle_ping_input(user_input: &str) -> Result<String, String> {
    let host = match user_input {
        "0.0.0.0" => "Go ping yourself",
        "localhost" => "Nice try!",
        "192.168.1.1" => "Nope",
        ok => ok,
    }.to_string();

    if !host.contains("://") {
        return Ok(host);
    }
    Url::parse(&host)
        .map(|parsed| parsed.host_str().unwrap_or("No host found").to_string())
        .map_err(|_| String::from("Not a host"))
}

pub fn ping(addr: &str) -> String {
    let validated_input = handle_ping_input(addr);
    if validated_input.is_err() {
        return format!("{}", validated_input.unwrap_err());
    }
    let reciever_result = pinger::ping(format!("{}", validated_input.unwrap()), None);

    if reciever_result.is_err() {
        return format!("Bad response from host");
    }

    let reciever = reciever_result.expect("to be ok").recv();

    if reciever.is_err() {
        return format!("Bad pinger response");
    }

    let ping_result = reciever.expect("ping result should exists");

    return ping_formatter(ping_result);
}
