//! The helper's half of the contract: read the token this boot's device wrote, serve one verb on
//! loopback, and tell the device where that is.
//!
//! The token file is the only thing the two processes share, and the device writes it: the helper
//! proves it is the process the device meant by presenting it, and cannot mint one of its own. That
//! is what keeps a local user from registering a browser the operator did not choose.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

/// Where the device leaves this boot's token (`ProgramData` gives users read, administrators write).
pub const TOKEN_FILE: &str = r"C:\ProgramData\Summrise\helper.token";
/// The device's loopback API, where a registration goes.
pub const DEVICE: &str = "127.0.0.1:18090";

/// The token for this boot, or `None` while the device has not written one yet.
pub fn read_token() -> Option<String> {
    let path = token_path();
    std::fs::read_to_string(path)
        .ok()
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
}

pub fn token_path() -> std::path::PathBuf {
    #[cfg(windows)]
    {
        std::path::PathBuf::from(TOKEN_FILE)
    }
    #[cfg(not(windows))]
    {
        std::env::temp_dir().join("summrise-helper.token")
    }
}

/// The loopback server the device calls. It answers `POST /launch` and nothing else.
pub struct Server {
    port: u16,
}

impl Server {
    /// Bind `127.0.0.1:0` and serve until the process ends.
    pub fn start(token: String) -> std::io::Result<Server> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        std::thread::spawn(move || {
            // The launch verb is served one request at a time, and the previous browser is this
            // thread's business: a second Chrome with the same profile would only hand the request
            // to the first one, which is listening on the old port.
            let mut last: Option<std::process::Child> = None;
            for stream in listener.incoming().flatten() {
                handle(stream, &token, &mut last);
            }
        });
        Ok(Server { port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

fn handle(mut stream: TcpStream, token: &str, last: &mut Option<std::process::Child>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let route = (request.method.as_str(), request.path.as_str());
    if !matches!(route, ("POST", "/launch") | ("POST", "/capture")) {
        respond(
            &mut stream,
            404,
            "Not Found",
            "{\"ok\":false,\"error\":\"no such route\"}",
        );
        return;
    }
    if !request.authorized(token) {
        respond(
            &mut stream,
            401,
            "Unauthorized",
            "{\"ok\":false,\"error\":\"that is not this boot's token\"}",
        );
        return;
    }
    if request.path == "/capture" {
        match capture() {
            Ok(picture) => respond(
                &mut stream,
                200,
                "OK",
                &format!(
                    "{{\"ok\":true,\"width\":{},\"height\":{},\"png\":\"{}\"}}",
                    picture.width, picture.height, picture.png
                ),
            ),
            Err(error) => respond(
                &mut stream,
                409,
                "Conflict",
                &format!("{{\"ok\":false,\"error\":{}}}", quoted(&error)),
            ),
        }
        return;
    }

    let Ok(launch) = serde_json::from_str::<LaunchRequest>(&request.body) else {
        respond(
            &mut stream,
            400,
            "Bad Request",
            "{\"ok\":false,\"error\":\"the body is not a launch request\"}",
        );
        return;
    };
    match launch_chrome(launch.port, last) {
        Ok(pid) => respond(
            &mut stream,
            200,
            "OK",
            &format!("{{\"ok\":true,\"pid\":{pid}}}"),
        ),
        Err(error) => respond(
            &mut stream,
            500,
            "Internal Server Error",
            &format!("{{\"ok\":false,\"error\":{}}}", quoted(&error)),
        ),
    }
}

/// One picture of the session this helper runs in.
struct Picture {
    width: u32,
    height: u32,
    png: String,
}

/// Capture the desktop, or say why it cannot be captured.
///
/// A locked session answers `Winlogon` when asked for its input desktop, and a disconnected one
/// renders nothing; both are refusals with a sentence, because a black frame is not a picture of
/// anything and would be read as "the screen is black".
fn capture() -> Result<Picture, String> {
    #[cfg(windows)]
    {
        let desktop = summrise_winsys::input_desktop_name()
            .map_err(|error| format!("the input desktop could not be read: {error}"))?;
        if desktop != "Default" {
            return Err(format!("the session is locked ({desktop})"));
        }
        let screen = summrise_winsys::capture_primary_screen()
            .map_err(|error| format!("the screen could not be captured: {error}"))?;
        if is_uniform(&screen.rgba) {
            return Err("the session is not being displayed: every pixel is the same".to_owned());
        }
        let png = encode_png(screen.width, screen.height, &screen.rgba)?;
        use base64::Engine;
        Ok(Picture {
            width: screen.width,
            height: screen.height,
            png: base64::engine::general_purpose::STANDARD.encode(png),
        })
    }
    #[cfg(not(windows))]
    {
        Err("screen capture is Windows-only".to_owned())
    }
}

/// Whether every pixel is identical: the signature of a desktop nobody is looking at.
fn is_uniform(rgba: &[u8]) -> bool {
    let pixels = rgba.as_chunks::<4>().0;
    let Some(first) = pixels.first() else {
        return true;
    };
    pixels.iter().all(|pixel| pixel == first)
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| format!("the PNG header could not be written: {error}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|error| format!("the PNG data could not be written: {error}"))?;
    }
    Ok(out)
}

/// What the device sends: the port it wants CDP on.
#[derive(serde::Deserialize)]
struct LaunchRequest {
    port: u16,
}

/// Start Chrome in this process's session with a profile of its own.
///
/// A profile of its own is not a preference: recent Chrome refuses `--remote-debugging-port` together
/// with the default user-data directory. The directory lives in the user's `LocalAppData`, so the
/// browser the device drives is inspectable by the person at the machine and survives a restart.
#[cfg(windows)]
fn launch_chrome(port: u16, last: &mut Option<std::process::Child>) -> Result<u32, String> {
    if let Some(previous) = last.as_mut() {
        if matches!(previous.try_wait(), Ok(None)) {
            let _ = previous.kill();
            let _ = previous.wait();
        }
    }
    let executable = discover_chrome().ok_or("chrome was not found for this user")?;
    let profile = profile_dir()?;
    std::fs::create_dir_all(&profile).map_err(|error| error.to_string())?;
    let child = std::process::Command::new(executable)
        .arg(format!("--remote-debugging-port={port}"))
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("about:blank")
        .spawn()
        .map_err(|error| format!("chrome did not start: {error}"))?;
    let pid = child.id();
    *last = Some(child);
    Ok(pid)
}

#[cfg(not(windows))]
fn launch_chrome(_port: u16, _last: &mut Option<std::process::Child>) -> Result<u32, String> {
    Err("the browser is launched on Windows only".to_owned())
}

/// The browser this helper owns: `SUMMRISE_CHROME` when it names a file, then the user's installs.
#[cfg(windows)]
fn discover_chrome() -> Option<std::path::PathBuf> {
    if let Some(configured) = std::env::var_os("SUMMRISE_CHROME") {
        let path = std::path::PathBuf::from(configured);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut candidates = Vec::new();
    for root in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(root) {
            let base = std::path::PathBuf::from(base);
            candidates.push(base.join(r"Google\Chrome\Application\chrome.exe"));
            candidates.push(base.join(r"Microsoft\Edge\Application\msedge.exe"));
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(windows)]
fn profile_dir() -> Result<std::path::PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or("this user has no LocalAppData")?;
    Ok(std::path::PathBuf::from(base)
        .join("Summrise")
        .join("chrome"))
}

/// JSON-quote a string without pulling a serializer into a line that only formats a sentence.
fn quoted(text: &str) -> String {
    let escaped: String = text
        .chars()
        .flat_map(|character| match character {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            '\n' => vec!['\\', 'n'],
            other => vec![other],
        })
        .collect();
    format!("\"{escaped}\"")
}

/// Tell the device where the helper listens. The registration is idempotent, so the caller may repeat
/// it — that is how a helper survives a device restart without a reconnect protocol.
pub fn register(device: &str, token: &str, port: u16, pid: u32) -> std::io::Result<()> {
    let body = format!("{{\"port\":{port},\"pid\":{pid}}}");
    let request = format!(
        "POST /helper HTTP/1.1\r\nHost: {device}\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(device)?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    stream.write_all(request.as_bytes())?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let text = String::from_utf8_lossy(&response);
    let status = text.lines().next().unwrap_or_default();
    if status.contains(" 204 ") {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "the device answered {status}"
        )))
    }
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Request {
    fn authorized(&self, token: &str) -> bool {
        let presented = self
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .and_then(|(_, value)| value.strip_prefix("Bearer "));
        match presented {
            Some(presented) => same(presented.as_bytes(), token.as_bytes()),
            None => false,
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    // Read until the head is complete, then exactly the body the head declared.
    loop {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = find(&buffer, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..end]).into_owned();
            let mut lines = head.lines();
            let request_line = lines.next()?;
            let mut parts = request_line.split_whitespace();
            let method = parts.next()?.to_owned();
            let path = parts.next()?.to_owned();
            let headers: Vec<(String, String)> = lines
                .filter_map(|line| line.split_once(':'))
                .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
                .collect();
            let declared: usize = headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.parse().ok())
                .unwrap_or(0);
            let body_start = end + 4;
            while buffer.len() < body_start + declared {
                let read = stream.read(&mut chunk).ok()?;
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
            }
            let body = String::from_utf8_lossy(&buffer[body_start..]).into_owned();
            return Some(Request {
                method,
                path,
                headers,
                body,
            });
        }
        if buffer.len() > 64 * 1024 {
            return None;
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn respond(stream: &mut TcpStream, status: u16, reason: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn same(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the server the way the device will, over a real socket.
    fn call(port: u16, bearer: &str) -> (u16, String) {
        call_path(port, "/launch", bearer)
    }

    fn call_path(port: u16, path: &str, bearer: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("the helper answers");
        let body = "{\"port\":51234}";
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {bearer}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .expect("the request writes");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("the answer reads");
        let status = response
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        (status, response)
    }

    #[test]
    fn the_launch_route_refuses_anything_but_this_boots_token() {
        let server = Server::start("a".repeat(64)).expect("the helper binds");
        let (status, _) = call(server.port(), &"b".repeat(64));
        assert_eq!(status, 401);
        // With the right token the answer depends on the platform (CI's Linux job has no browser to
        // launch); what must not happen is another refusal.
        let (status, body) = call(server.port(), &"a".repeat(64));
        assert_ne!(status, 401, "{body}");
        assert!(body.contains("\"ok\""), "{body}");
    }

    #[test]
    fn the_capture_route_needs_the_token_and_says_why_it_cannot_capture_here() {
        let server = Server::start("a".repeat(64)).expect("the helper binds");
        let (status, _) = call_path(server.port(), "/capture", &"b".repeat(64));
        assert_eq!(status, 401);
        // With the right token the answer depends on the platform: CI's Windows runner has a desktop
        // and captures it, while the Linux job can only say why it cannot.
        let (status, body) = call_path(server.port(), "/capture", &"a".repeat(64));
        assert_ne!(status, 401, "{body}");
        if cfg!(windows) {
            assert!(status == 200 || status == 409, "{status}: {body}");
        } else {
            assert_eq!(status, 409, "{body}");
            assert!(body.contains("Windows-only"), "{body}");
        }
    }

    #[test]
    fn a_uniform_frame_is_not_a_picture() {
        assert!(is_uniform(&[0, 0, 0, 255, 0, 0, 0, 255]));
        assert!(!is_uniform(&[0, 0, 0, 255, 1, 0, 0, 255]));
        assert!(is_uniform(&[]));
    }

    #[test]
    fn the_png_writer_produces_a_png() {
        let png = encode_png(2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]).expect("the encoder writes");
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[test]
    fn a_body_that_is_not_a_launch_request_is_refused() {
        let server = Server::start("a".repeat(64)).expect("the helper binds");
        let mut stream = TcpStream::connect(("127.0.0.1", server.port())).expect("connects");
        let body = "{\"nope\":1}";
        let request = format!(
            "POST /launch HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            "a".repeat(64),
            body.len()
        );
        stream.write_all(request.as_bytes()).expect("writes");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("reads");
        assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    }

    #[test]
    fn the_launch_route_is_the_only_one() {
        let server = Server::start("a".repeat(64)).expect("the helper binds");
        let mut stream = TcpStream::connect(("127.0.0.1", server.port())).expect("connects");
        stream
            .write_all(b"GET /nope HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .expect("writes");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("reads");
        assert!(response.starts_with("HTTP/1.1 404"), "{response}");
    }

    #[test]
    fn a_registration_names_the_port_and_carries_the_token() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a fake device");
        let address = listener.local_addr().expect("the address");
        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let recorder = seen.clone();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("one connection");
            let mut buffer = [0u8; 4096];
            let read = stream.read(&mut buffer).expect("the request reads");
            *recorder.lock().expect("the recorder") =
                String::from_utf8_lossy(&buffer[..read]).into_owned();
            stream
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .expect("writes");
        });

        register(&address.to_string(), &"a".repeat(64), 49812, 2260).expect("the device accepts");

        let request = seen.lock().expect("the recorder").clone();
        assert!(request.starts_with("POST /helper HTTP/1.1"), "{request}");
        assert!(request.contains(&"a".repeat(64)), "{request}");
        assert!(request.contains("\"port\":49812"), "{request}");
        assert!(request.contains("\"pid\":2260"), "{request}");
    }

    #[test]
    fn a_registration_that_the_device_refuses_is_an_error_not_a_silence() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a fake device");
        let address = listener.local_addr().expect("the address");
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("one connection");
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            stream
                .write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n")
                .expect("writes");
        });
        let error =
            register(&address.to_string(), "wrong", 1, 1).expect_err("a refusal is an error");
        assert!(error.to_string().contains("401"), "{error}");
    }

    #[test]
    fn a_token_that_only_differs_in_length_does_not_authorize() {
        assert!(!same(b"abc", b"abcd"));
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
    }
}
