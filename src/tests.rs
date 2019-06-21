use super::*;

#[test]
fn extract_host_good_header() {
    let header = "GET / HTTP/1.1\r
Host: www.rust-lang.org\r
User-Agent: Mozilla/5.0 (X11; Linux x86_64; rv:67.0) Gecko/20100101 Firefox/67.0\r
Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8\r
Accept-Language: en-US,en;q=0.5\r
Accept-Encoding: gzip, deflate, br\r
Connection: keep-alive\r
Upgrade-Insecure-Requests: 1\r
Cache-Control: max-age=0\r
\r
";

    let result = extract_host(header);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "www.rust-lang.org");
}

#[test]
fn extract_host_no_host() {
    let header = "GET / HTTP/1.1\r
User-Agent: Mozilla/5.0 (X11; Linux x86_64; rv:67.0) Gecko/20100101 Firefox/67.0\r
Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8\r
Accept-Language: en-US,en;q=0.5\r
Accept-Encoding: gzip, deflate, br\r
Connection: keep-alive\r
Upgrade-Insecure-Requests: 1\r
Cache-Control: max-age=0\r
\r
";

    let result = extract_host(header);
    assert!(result.is_err());
}

#[test]
fn extract_host_bad_header() {
    let header = "";

    let result = extract_host(header);
    assert!(result.is_err());
}

#[test]
fn extract_uri_good_header() {
    let header = "GET /foo/bar HTTP/1.1\r
Host: www.rust-lang.org\r
User-Agent: Mozilla/5.0 (X11; Linux x86_64; rv:67.0) Gecko/20100101 Firefox/67.0\r
Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8\r
Accept-Language: en-US,en;q=0.5\r
Accept-Encoding: gzip, deflate, br\r
Connection: keep-alive\r
Upgrade-Insecure-Requests: 1\r
Cache-Control: max-age=0\r
\r
";
    let result = extract_uri(header);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "/foo/bar");
}

#[test]
fn extract_uri_bad_header() {
    let header = "somegarbage./foo/bar.HTTP/1.1\r
Host: www.rust-lang.org\r
User-Agent: Mozilla/5.0 (X11; Linux x86_64; rv:67.0) Gecko/20100101 Firefox/67.0\r
Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8\r
Accept-Language: en-US,en;q=0.5\r
Accept-Encoding: gzip, deflate, br\r
Connection: keep-alive\r
Upgrade-Insecure-Requests: 1\r
Cache-Control: max-age=0\r
\r
";
    let result = extract_uri(header);
    assert!(result.is_err());
}

#[test]
fn extract_uri_bad_header2() {
    let header = "";
    let result = extract_uri(header);
    assert!(result.is_err());
}

//// Full server tests
use rouille::*;
use std::{net::TcpListener, ops::Range};

fn port_available(port: u16) -> bool {
    match TcpListener::bind(format!("127.0.0.1:{}", port)) {
        Ok(_) => true,
        Err(_) => false,
    }
}

fn get_available_port(mut range: Range<u16>) -> Option<u16> {
    range.find(|port| port_available(*port))
}

#[test]
fn test_server() {
    let listenport = get_available_port(6000..8000);
    assert!(listenport.is_some());
    let listenport = listenport.unwrap();

    // Create a registry that points host "test-website.com" to our port,
    // Start the proxy server and test we can proxy the connection.
    let registry = Arc::new(registry::tests::test_registry(
        "test-website.com",
        listenport,
    ));
    assert!(registry.update().is_ok());

    // These thread spawns leave threads running after the test ends. So far this
    // has not been a problem, but I should find a way to clean this up.
    eprintln!("http server listening on 127.0.0.1:{}", listenport);
    thread::spawn(move || {
        rouille::start_server(format!("127.0.0.1:{}", listenport), move |request| {
            router!(request,
                    (GET) (/) => {
                        rouille::Response::text("hello world")
                    },
                    _ => rouille::Response::empty_400())
        })
    });

    let proxyport = get_available_port(8000..10000);
    assert!(proxyport.is_some());
    let proxyport = proxyport.unwrap();

    eprintln!("proxy listening on 127.0.0.1:{}", proxyport);
    thread::spawn(move || {
        eprintln!(
            "PROXY SERVER RETURNED: {:?}",
            run_server(&format!("127.0.0.1:{}", proxyport), registry)
                .map_err(|e| format!("{:?}", e))
        );
    });

    let res = reqwest::Client::new()
        .get(&format!("http://127.0.0.1:{}", proxyport))
        .header(reqwest::header::HOST, "test-website.com")
        .send();

    assert!(res.is_ok());
    let mut response: reqwest::Response = res.unwrap();
    assert!(response.status().is_success());
    let text = response.text();
    assert!(text.is_ok());
    assert_eq!(text.unwrap(), "hello world");
}
