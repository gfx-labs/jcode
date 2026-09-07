//! Exercise the production queues and HTTP client with synthetic data only.
//! Each scenario gets an isolated process/home. A second loopback listener traps
//! redirects and acts as the proxy for any accidental non-local destination.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const CHILD_MODE: &str = "JCODE_TEST_LOCAL_REPORTING_CHILD";

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn accept(listener: &TcpListener) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "local telemetry request timed out"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    }
}

fn request(stream: &mut TcpStream) -> (String, serde_json::Value) {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    assert!(first.starts_with("POST "), "{first}");
    let mut length = None;
    let mut is_json = false;
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            if key.eq_ignore_ascii_case("content-length") {
                length = Some(value.trim().parse::<usize>().unwrap());
            }
            if key.eq_ignore_ascii_case("content-type") {
                is_json = value.trim() == "application/json";
            }
        }
    }
    assert!(is_json);
    let length = length.expect("content length");
    assert!(length < 1_000_000);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    (
        first.split_whitespace().nth(1).unwrap().to_string(),
        serde_json::from_slice(&body).unwrap(),
    )
}

fn run_receiver_scenario(redirect: bool) {
    let home = tempfile::tempdir().unwrap();
    let receiver = TcpListener::bind("127.0.0.1:0").unwrap();
    receiver.set_nonblocking(true).unwrap();
    let trap = TcpListener::bind("127.0.0.1:0").unwrap();
    trap.set_nonblocking(true).unwrap();
    let proxy = format!("http://{}", trap.local_addr().unwrap());
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "local_reporting_child", "--nocapture"])
        .env(CHILD_MODE, "1")
        .env("JCODE_HOME", home.path())
        .env(
            "JCODE_TELEMETRY_BASE_URL",
            format!("http://{}/prefix/", receiver.local_addr().unwrap()),
        )
        .env_remove("JCODE_NO_TELEMETRY")
        .env_remove("DO_NOT_TRACK")
        .env("NO_PROXY", "127.0.0.1,localhost,::1")
        .env("no_proxy", "127.0.0.1,localhost,::1")
        .stdin(Stdio::piped());
    for key in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        command.env(key, &proxy);
    }
    let mut child = ChildGuard(command.spawn().unwrap());
    for event in ["transcript", "telemetry_opt_out"] {
        let mut stream = accept(&receiver);
        let (path, body) = request(&mut stream);
        assert_eq!(body["event"], event);
        assert_eq!(
            path,
            if event == "transcript" {
                "/prefix/v1/transcript"
            } else {
                "/prefix/v1/event"
            }
        );
        if event == "transcript" {
            assert_eq!(
                body["messages"][0]["content"],
                "synthetic local telemetry test"
            );
        } else {
            assert!(body.get("messages").is_none());
        }
        if redirect {
            write!(stream, "HTTP/1.1 307 Temporary Redirect\r\nLocation: {proxy}/must-not-receive\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        } else {
            write!(
                stream,
                "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        }
        stream.flush().unwrap();
        // The parent acknowledges receipt before the child opts out. This
        // avoids a sleep racing the background transcript's consent recheck.
        if event == "transcript" {
            child
                .0
                .stdin
                .as_mut()
                .unwrap()
                .write_all(b"received\n")
                .unwrap();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "telemetry child failed: {status}");
            break;
        }
        assert!(Instant::now() < deadline, "telemetry child did not exit");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        trap.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "a payload escaped to a redirect target or a non-local proxy destination"
    );
    assert_eq!(
        receiver.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert!(home.path().join("no_telemetry").exists());
}

#[test]
fn production_usage_and_transcript_delivery_reach_local_receiver() {
    run_receiver_scenario(false);
}

#[test]
fn production_usage_and_transcript_delivery_never_follow_redirects() {
    run_receiver_scenario(true);
}

#[test]
fn local_reporting_child() {
    if std::env::var_os(CHILD_MODE).is_none() {
        return;
    }
    use jcode_telemetry_core as telemetry;
    let messages =
        serde_json::json!([{"role": "user", "content": "synthetic local telemetry test"}]);
    assert!(!telemetry::record_transcript(
        "test",
        "test",
        telemetry::SessionEndReason::NormalExit,
        messages.clone()
    ));
    assert!(telemetry::set_content_sharing_enabled(true));
    assert!(telemetry::record_transcript(
        "test",
        "test",
        telemetry::SessionEndReason::NormalExit,
        messages.clone()
    ));
    let mut acknowledgement = String::new();
    std::io::stdin().read_line(&mut acknowledgement).unwrap();
    assert_eq!(acknowledgement.trim(), "received");
    assert!(telemetry::set_usage_telemetry_enabled(false));
    assert!(!telemetry::is_enabled());
    assert!(!telemetry::record_transcript(
        "test",
        "test",
        telemetry::SessionEndReason::NormalExit,
        messages
    ));
}
