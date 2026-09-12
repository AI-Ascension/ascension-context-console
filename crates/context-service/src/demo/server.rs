// SPDX-License-Identifier: MIT

//! Loopback listener and request loop for the integrated demonstration.
//!
//! The listener binds `127.0.0.1` only, reads one bounded request per connection, writes one
//! response, and closes. There is no outbound connection and no process execution path.

use super::state::DemoState;
use crate::http::{ApiError, HttpRequest, error_response, read_request_bytes};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

/// Run the bounded integrated demonstration until the operator terminates the process.
pub fn run(port: u16) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let mut state = DemoState::build(address.port()).map_err(|error| error.to_string())?;
    println!("integrated_demo_ready=http://{address}/web/");
    println!("integrated_demo_provider_calls=0");
    println!("integrated_demo_game_launches=0");
    std::io::stdout()
        .flush()
        .map_err(|error| error.to_string())?;

    for incoming in listener.incoming() {
        let mut stream = incoming.map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|error| error.to_string())?;
        let response = match read_request(&mut stream) {
            Ok(request) => state.dispatch(&request),
            Err(error) => error_response(error),
        };
        response
            .write_to(&mut stream)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, ApiError> {
    let bytes = read_request_bytes(stream)?;
    HttpRequest::parse(&bytes)
}
