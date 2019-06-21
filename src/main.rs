#[macro_use]
extern crate lazy_static;
mod read_http_header;
mod registry;

use std::{error::Error, str::from_utf8, sync::Arc, thread, time::Duration};

use regex::{Regex, RegexBuilder};
use tokio::{
    io::{copy, write_all},
    net::{TcpListener, TcpStream},
    prelude::*,
    runtime::Builder,
};

use read_http_header::read_http_header;
use registry::{GetHostError, ServiceRegistry};

#[cfg(test)]
mod tests;

fn main() {
    launch().map_err(|e| eprintln!("Error: {}", e)).ok();
}

fn launch() -> Result<(), Box<dyn Error>> {
    let conf = get_config();

    // Weird syntax for default type parameters. See:
    // https://github.com/rust-lang/rust/issues/36980#issuecomment-251726254
    // https://github.com/rust-lang-nursery/reference/issues/24
    let registry = Arc::new(<ServiceRegistry>::new());
    registry
        .update()
        .map_err(|e| format!("{} is consul running on 127.0.0.1:8500?", e))?;
    let refresh_copy = registry.clone();
    thread::spawn(move || refresh_service_map(refresh_copy));
    let bind_address = format!(
        "{}:{}",
        conf.get_str("bind_host").unwrap(),
        conf.get_str("bind_port").unwrap()
    );
    run_server(&bind_address, registry).map_err(|e| e.into())
}

fn refresh_service_map(registry: Arc<ServiceRegistry>) {
    loop {
        thread::sleep(Duration::from_secs(10));
        let result = registry.update();
        if let Err(e) = result {
            eprintln!("Failed to update service map: {:?}", e);
        }
    }
}

fn get_config() -> config::Config {
    let mut conf = config::Config::default();
    conf.set_default("bind_host", "0.0.0.0")
        .unwrap()
        .set_default("bind_port", 60000)
        .unwrap();

    conf.merge(config::File::with_name("/etc/robby"))
        .map_err(|e| eprintln!("Using default config: {}", e))
        .ok();

    conf
}

/// Proxy connection copies bytes back and forth between two TcpStreams.
/// This returns a future which will resolve when either stream closes the
/// connection.
fn proxy_connection(
    server_stream: TcpStream,
    client_stream: TcpStream,
) -> impl Future<Item = (), Error = ()> {
    let (sreader, swriter) = server_stream.split();
    let (creader, cwriter) = client_stream.split();

    let from_server_bytes_copied = copy(sreader, cwriter);
    let from_client_bytes_copied = copy(creader, swriter);

    let handle_from_server = from_server_bytes_copied
        .map(|count| {
            println!("wrote {} bytes from server to client.", count.0);
            println!("Server closed the connection. Disconnecting from client.");
        })
        .map_err(|err| eprintln!("IO error {:?}", err));

    let handle_from_client = from_client_bytes_copied
        .map(|count| {
            println!("wrote {} bytes from client to server.", count.0);
            println!("Client closed the connection. Disconnecting from server.");
        })
        .map_err(|err| eprintln!("IO error {:?}", err));

    // Wait for one to complete then drop the other.
    handle_from_server
        .select(handle_from_client)
        .then(|_res| future::ok(()))
}

fn extract_host(header: &str) -> Result<&str, ()> {
    lazy_static! {
        static ref RE: Regex = RegexBuilder::new(r"Host: ([^\r]*)[\r]?")
            .case_insensitive(true)
            .build()
            .unwrap();
    }
    let caps = RE.captures(header);
    if let None = caps {
        eprintln!("Failed to find 'Host' header.");
        eprintln!("Header: {}", header);
        return Err(());
    }
    let caps = caps.unwrap();
    let host = caps.get(1).unwrap().as_str();
    Ok(host)
}

fn extract_uri(header: &str) -> Result<&str, ()> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"[A-Z]* ([^ ]*) .*\r").unwrap();
    }
    let mut lines = header.split("\n");
    let reqline = lines.nth(0).unwrap();
    let caps = RE.captures(reqline);
    if let None = caps {
        eprintln!("Failed to extract request URI.");
        eprintln!("Header: {}", header);
        return Err(());
    }
    let caps = caps.unwrap();
    let host = caps.get(1).unwrap().as_str();
    Ok(host)
}

fn run_server<T>(server_address: &str, registry: Arc<ServiceRegistry<T>>) -> Result<(), String>
where
    T: 'static + registry::ServiceProvider + Send + Sync,
{
    let addr = server_address
        .parse()
        .map_err(|e| format!("Can't parse address {}. {}", server_address, e))?;
    let listener =
        TcpListener::bind(&addr).map_err(|e| format!("Failed to bind address {}. {}", addr, e))?;

    println!("Robby listening on {}", &server_address);
    let server = listener
        .incoming()
        .map_err(|e| eprintln!("accept failed = {:?}", e))
        .for_each(move |client_sock| {
            println!("Connection from {}", client_sock.peer_addr().unwrap());

            // Each proxy connection needs a reference to the registry.
            let registry = registry.clone();

            // Read the http header and determine the target host by the Host header.
            // Don't exceed 16384 (16k) bytes.
            let headerbuf = vec![0; 16384];
            let con = read_http_header(client_sock, headerbuf)
                .map_err(|e| eprintln!("Read error: {:?}", e))
                .and_then(move |(client_sock, mut buffer, totalbytes, split)| {
                    buffer.truncate(totalbytes);
                    let header = &buffer[..split];
                    let parsed_header = from_utf8(&header);
                    if let Err(e) = parsed_header {
                        eprintln!("Failed to decode utf-8: {:?}", e);
                        return Err(());
                    }
                    let parsed_header = parsed_header.ok().unwrap();

                    let host = extract_host(&parsed_header)?;
                    // In the future, match on Host header *and* URI path.
                    //let _uri = extract_uri(&parsed_header)?;

                    // Lookup this host in the service registry.
                    let addr = registry.lookup(host).map_err(|e| {
                        match e {
                            GetHostError::StrErr(estr) => {
                                eprintln!("Error: {:?}", estr);
                            }
                            GetHostError::PoisonErr(estr) => {
                                // This should happen if the lock is poisoned,
                                // meaning we can't continue.
                                eprintln!("Failed to acquire lock: {:?}", estr);
                                panic!();
                            }
                        }
                    })?;
                    println!("Have mapping {} -> {}", host, addr);

                    // Connect to the address from the service map, copy the bytes
                    // we already read, and then proxy the remainder of the connection.
                    let server_sock = TcpStream::connect(&addr);
                    let server_con = server_sock
                        .map_err(|e| eprintln!("Failed to connect: {:?}", e))
                        .and_then(|server_stream| {
                            write_all(server_stream, buffer)
                                .map_err(|e| eprintln!("Error: {:?}", e))
                        })
                        .and_then(|(server_stream, _buf)| {
                            proxy_connection(server_stream, client_sock)
                        })
                        .map_err(|e| eprintln!("Error: {:?}", e));
                    tokio::spawn(server_con);
                    Ok(())
                })
                .map_err(|e| eprintln!("Read Error: {:?}", e));

            // We spawn con and return an empty future, even though we could return con.
            // The reason is that futures returned in for_each blocks are each resolved
            // before the next iteration begins. The future held in con resolves after the
            // header has been read and the downstream connection has been made.
            // If we return con, then each connection from a client will read headers and
            // connect to the target serially rather than concurrently like we want.
            tokio::spawn(con);
            future::ok(())
        })
        .map_err(|e| eprintln!("Error talking to client: {:?}", e));

    // We need to add a panic_handler that kills the process when a worker panics.
    // There's no valid excuse to continue after a panic, since we don't know what
    // state the program is in. Panics are not exceptional conditions or errors,
    // they are panics. I'm really not sure why tokio catches and ignores them by
    // default.
    let mut runtime = Builder::new()
        .panic_handler(|e| {
            eprintln!("FATAL ERROR: {:?}", e);
            std::process::exit(1);
        })
        .build()
        .expect("failed to start new Runtime");
    runtime.spawn(server);
    runtime.shutdown_on_idle().wait().unwrap();
    Ok(())
}
