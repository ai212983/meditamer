use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use super::{make_client, require_health_check};

fn spawn_health_server(statuses: Vec<u16>) -> (u16, Arc<AtomicUsize>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    listener.set_nonblocking(false).expect("set blocking mode");
    let port = listener.local_addr().expect("local_addr").port();
    let hit_count = Arc::new(AtomicUsize::new(0));
    let hit_count_clone = Arc::clone(&hit_count);
    let handle = thread::spawn(move || {
        for code in statuses {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            hit_count_clone.fetch_add(1, Ordering::Relaxed);
            let body = if code == 200 { "ok" } else { "err" };
            let response = format!(
                "HTTP/1.1 {code} TEST\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    (port, hit_count, handle)
}

#[test]
fn health_check_retries_and_fails_with_attempt_count() {
    let (port, hit_count, handle) = spawn_health_server(vec![500, 500, 500]);
    let client = make_client(2.0).expect("client");
    let err = require_health_check(&client, "127.0.0.1", port, 2.0, 3, 1).expect_err("must fail");
    assert!(err.to_string().contains("attempts=3"));
    assert_eq!(hit_count.load(Ordering::Relaxed), 3);
    handle.join().expect("join");
}

#[test]
fn health_check_succeeds_after_transient_failures() {
    let (port, hit_count, handle) = spawn_health_server(vec![500, 500, 200]);
    let client = make_client(2.0).expect("client");
    require_health_check(&client, "127.0.0.1", port, 2.0, 5, 1).expect("must succeed");
    assert_eq!(hit_count.load(Ordering::Relaxed), 3);
    handle.join().expect("join");
}
