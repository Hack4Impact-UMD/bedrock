//! Shared setup for tests that talk to the local AWS emulator. Ensures that tests don't
//! accidentally hit real AWS resources.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

pub async fn ses_client() -> Option<aws_sdk_sesv2::Client> {
    let Some(endpoint) = std::env::var("AWS_ENDPOINT_URL").ok() else {
        return skip("AWS_ENDPOINT_URL is not set");
    };
    let addr = loopback_addr(&endpoint).unwrap_or_else(|| {
        panic!("AWS_ENDPOINT_URL={endpoint} is not a loopback address; refusing to run SES tests")
    });
    if TcpStream::connect_timeout(&addr, Duration::from_secs(1)).is_err() {
        return skip(&format!("nothing is listening at {endpoint}"));
    }

    let shared = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .load()
        .await;
    let config = aws_sdk_sesv2::config::Builder::from(&shared)
        .endpoint_url(&endpoint)
        .build();
    Some(aws_sdk_sesv2::Client::from_conf(config))
}

fn skip<T>(reason: &str) -> Option<T> {
    assert!(
        std::env::var_os("CI").is_none(),
        "the AWS emulator must be running in CI: {reason}"
    );
    eprintln!("skipping: {reason}");
    None
}

fn loopback_addr(endpoint: &str) -> Option<SocketAddr> {
    let (default_port, rest) = if let Some(rest) = endpoint.strip_prefix("https://") {
        (443, rest)
    } else {
        let rest = endpoint.strip_prefix("http://")?;
        (80, rest)
    };
    let authority = rest.split('/').next()?;
    let addrs: Vec<SocketAddr> = match authority.to_socket_addrs() {
        Ok(addrs) => addrs.collect(),
        Err(_) => (authority, default_port).to_socket_addrs().ok()?.collect(),
    };
    if addrs.is_empty() || !addrs.iter().all(|a| a.ip().is_loopback()) {
        return None;
    }
    addrs.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_loopback_endpoints() {
        assert!(loopback_addr("https://127.0.0.1:9297").is_some());
        assert!(loopback_addr("http://localhost:4566/").is_some());
        assert!(loopback_addr("https://[::1]:9297").is_some());

        assert!(loopback_addr("https://email.us-east-1.amazonaws.com").is_none());
        assert!(loopback_addr("https://10.0.0.5:9297").is_none());
        assert!(loopback_addr("127.0.0.1:9297").is_none());
        assert!(loopback_addr("").is_none());
    }
}
