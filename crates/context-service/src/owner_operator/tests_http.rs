#[test]
fn owner_http_parser_keeps_legacy_limit_separate_and_enforces_closed_framing() {
    use super::http::{MAX_BODY_BYTES, MAX_HEADER_BYTES};
    use crate::http::{ApiError, HttpRequest, MAX_HTTP_REQUEST_BYTES};

    let header = b"POST /v2/context-owner/invocations HTTP/1.1\r\nHost: console.example\r\nContent-Length: 3\r\n\r\n";
    let mut valid = header.to_vec();
    valid.extend_from_slice(b"abc");
    let parsed = HttpRequest::parse_with_limits(
        &valid,
        true,
        MAX_HEADER_BYTES + MAX_BODY_BYTES,
        MAX_BODY_BYTES,
        MAX_HEADER_BYTES,
    )
    .expect("bounded owner request");
    assert_eq!(parsed.body, b"abc");
    let mut legacy_sized =
        b"POST /v2/context-owner/invocations HTTP/1.1\r\nContent-Length: 20000\r\n\r\n".to_vec();
    legacy_sized.extend(std::iter::repeat_n(b'x', 20_000));
    assert!(
        HttpRequest::parse_with_limits(
            &legacy_sized,
            true,
            MAX_HEADER_BYTES + MAX_BODY_BYTES,
            MAX_BODY_BYTES,
            MAX_HEADER_BYTES,
        )
        .is_ok()
    );
    assert_eq!(HttpRequest::parse(&legacy_sized), Err(ApiError::BadRequest));
    assert_eq!(MAX_HTTP_REQUEST_BYTES, 16 * 1024);

    for length in [b"+3".as_slice(), b"3x".as_slice(), b"".as_slice()] {
        let mut request = b"POST / HTTP/1.1\r\nContent-Length: ".to_vec();
        request.extend_from_slice(length);
        request.extend_from_slice(b"\r\n\r\n");
        assert_eq!(
            HttpRequest::parse_with_limits(
                &request,
                false,
                MAX_HEADER_BYTES + MAX_BODY_BYTES,
                MAX_BODY_BYTES,
                MAX_HEADER_BYTES,
            ),
            Err(ApiError::BadRequest)
        );
    }
    let duplicate = b"POST / HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n";
    assert!(
        HttpRequest::parse_with_limits(
            duplicate,
            false,
            MAX_HEADER_BYTES + MAX_BODY_BYTES,
            MAX_BODY_BYTES,
            MAX_HEADER_BYTES,
        )
        .is_err()
    );
    let transfer = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
    assert!(
        HttpRequest::parse_with_limits(
            transfer,
            false,
            MAX_HEADER_BYTES + MAX_BODY_BYTES,
            MAX_BODY_BYTES,
            MAX_HEADER_BYTES,
        )
        .is_err()
    );

    let mut oversized_header = b"POST / HTTP/1.1\r\nX-Pad: ".to_vec();
    oversized_header.resize(MAX_HEADER_BYTES, b'a');
    oversized_header.extend_from_slice(b"\r\n\r\n");
    assert_eq!(
        HttpRequest::parse_with_limits(
            &oversized_header,
            false,
            MAX_HEADER_BYTES + MAX_BODY_BYTES,
            MAX_BODY_BYTES,
            MAX_HEADER_BYTES,
        ),
        Err(ApiError::TooLarge)
    );
}

#[test]
fn response_serializer_stops_at_limit_without_returning_a_partial_body() {
    let value = "x".repeat(super::http::MAX_BODY_BYTES + 1);
    assert!(super::http::serialize_bounded(&value).is_err());
    let exact = "y".repeat(super::http::MAX_BODY_BYTES - 2);
    let body = super::http::serialize_bounded(&exact).expect("bounded JSON string");
    assert_eq!(body.len(), super::http::MAX_BODY_BYTES);
}

#[test]
fn absolute_write_loop_recomputes_timeout_after_each_partial_write() {
    use std::cell::Cell;
    use std::io;
    use std::time::{Duration, Instant};

    let start = Instant::now();
    let deadline = start + Duration::from_millis(10);
    let clock = Cell::new(start);
    let timeouts = std::cell::RefCell::new(Vec::new());
    let writes = Cell::new(0);
    let result = super::http::write_all_with_deadline(
        b"abcd",
        deadline,
        || clock.get(),
        |timeout| {
            timeouts.borrow_mut().push(timeout);
            Ok(())
        },
        |remaining| {
            writes.set(writes.get() + 1);
            match writes.get() {
                1 => {
                    clock.set(start + Duration::from_millis(3));
                    Ok(2)
                }
                2 => {
                    assert_eq!(remaining, b"cd");
                    clock.set(start + Duration::from_millis(4));
                    Ok(1)
                }
                3 => {
                    assert_eq!(remaining, b"d");
                    clock.set(start + Duration::from_millis(5));
                    Ok(1)
                }
                _ => Err(io::Error::other("unexpected write")),
            }
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(
        timeouts.borrow().as_slice(),
        &[
            Duration::from_millis(10),
            Duration::from_millis(7),
            Duration::from_millis(6),
        ]
    );
    assert_eq!(writes.get(), 3);
}

#[test]
fn absolute_write_loop_stops_after_a_partial_write_crosses_deadline() {
    use std::cell::Cell;
    use std::time::{Duration, Instant};

    let start = Instant::now();
    let deadline = start + Duration::from_millis(5);
    let clock = Cell::new(start);
    let writes = Cell::new(0);
    let result = super::http::write_all_with_deadline(
        b"remaining",
        deadline,
        || clock.get(),
        |_| Ok(()),
        |_| {
            writes.set(writes.get() + 1);
            clock.set(start + Duration::from_millis(6));
            Ok(1)
        },
    );

    assert_eq!(result, Err(super::http::ReadError::Deadline));
    assert_eq!(writes.get(), 1);
}

#[test]
fn absolute_write_loop_retries_interrupted_with_remaining_time_and_rejects_zero() {
    use std::cell::Cell;
    use std::io;
    use std::time::{Duration, Instant};

    let start = Instant::now();
    let deadline = start + Duration::from_millis(5);
    let clock = Cell::new(start);
    let timeouts = std::cell::RefCell::new(Vec::new());
    let writes = Cell::new(0);
    let result = super::http::write_all_with_deadline(
        b"x",
        deadline,
        || clock.get(),
        |timeout| {
            timeouts.borrow_mut().push(timeout);
            Ok(())
        },
        |_| {
            writes.set(writes.get() + 1);
            if writes.get() == 1 {
                clock.set(start + Duration::from_millis(2));
                Err(io::Error::from(io::ErrorKind::Interrupted))
            } else {
                clock.set(start + Duration::from_millis(3));
                Ok(1)
            }
        },
    );
    assert_eq!(result, Ok(()));
    assert_eq!(
        timeouts.borrow().as_slice(),
        &[Duration::from_millis(5), Duration::from_millis(3)]
    );
    assert_eq!(writes.get(), 2);

    let zero_write =
        super::http::write_all_with_deadline(b"x", deadline, || start, |_| Ok(()), |_| Ok(0));
    assert_eq!(zero_write, Err(super::http::ReadError::Io));
}
