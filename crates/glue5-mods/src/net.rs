//! `net`: a mod's requests to the network, HTTP and HTTPS, in the shape of
//! Lune's `@lune/net` (`net.request`), the Luau runtime's: code written for
//! it works here. ureq does the requests; rustls with ring and webpki's
//! roots the TLS, the same on every system (no system TLS; Apple's ATS is
//! for its own URL loading, so plain HTTP works on iOS too). Each request
//! runs on a host thread of its own; the mod's coroutine waits for the
//! answer as `wait()` does, a frame at a time, so the game never waits for
//! the network. The guards are the app's: [`IN_FLIGHT`] requests at once a
//! mod (more wait for a free one), answers of [`BODY`] at most, a timeout
//! on each. A mod that stops leaves its requests to end on their own
//! (their timeout at most); their answers go nowhere.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use mlua::{AnyUserData, Function, Lua, Table, UserData, Value};
use ureq::http::StatusCode;

/// Requests a mod may have going at once.
const IN_FLIGHT: usize = 4;
/// The largest answer a mod gets (its body, unpacked).
const BODY: u64 = 16 << 20;
/// A request's time, from its start to its answer's last byte, by default
/// and at most (seconds).
const TIMEOUT: f64 = 30.0;
const TIMEOUT_MAX: f64 = 300.0;

struct Request {
    method: String,
    url: String,
    headers: Vec<(String, Vec<u8>)>,
    body: Option<Vec<u8>>,
    timeout: Duration,
}

struct Reply {
    status: StatusCode,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

type Slot = Arc<Mutex<Option<Result<Reply, String>>>>;

/// A request on its way, until `poll` has its answer.
struct Pending {
    what: String,
    slot: Slot,
}

impl UserData for Pending {}

/// The agent every request goes through (its connections are kept a while
/// and reused).
fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .allow_non_standard_methods(true)
                .user_agent(concat!("GLUE5/", env!("CARGO_PKG_VERSION")))
                .build(),
        )
    })
}

fn run(r: Request) -> Result<Reply, String> {
    let timeout = r.timeout;
    let head = r.method == "HEAD";
    let mut builder = ureq::http::Request::builder().method(r.method.as_str()).uri(r.url.as_str());
    for (name, value) in &r.headers {
        builder = builder.header(name.as_str(), value.as_slice());
    }
    let reply = match r.body {
        Some(body) => {
            let request = builder.body(body).map_err(|e| e.to_string())?;
            agent().run(agent().configure_request(request).timeout_global(Some(timeout)).build())
        }
        None => {
            let request = builder.body(()).map_err(|e| e.to_string())?;
            agent().run(agent().configure_request(request).timeout_global(Some(timeout)).build())
        }
    };
    let mut reply = reply.map_err(|e| describe(e, timeout))?;
    let mut headers: Vec<(String, String)> = Vec::new();
    for (name, value) in reply.headers() {
        let value = String::from_utf8_lossy(value.as_bytes());
        // Repeated ones as one, as HTTP allows.
        match headers.iter_mut().find(|(n, _)| n == name.as_str()) {
            Some((_, v)) => {
                v.push_str(", ");
                v.push_str(&value);
            }
            None => headers.push((name.as_str().to_string(), value.into_owned())),
        }
    }
    let status = reply.status();
    // None to read (ureq would try to unpack an empty gzip one).
    if head || status == StatusCode::NO_CONTENT || status == StatusCode::NOT_MODIFIED {
        return Ok(Reply { status, headers, body: Vec::new() });
    }
    let body = reply.body_mut().with_config().limit(BODY).read_to_vec().map_err(|e| match e {
        ureq::Error::BodyExceedsLimit(_) => format!("the answer is larger than {} MB", BODY >> 20),
        e => describe(e, timeout),
    })?;
    Ok(Reply { status, headers, body })
}

fn describe(e: ureq::Error, timeout: Duration) -> String {
    match e {
        ureq::Error::Timeout(_) => format!("no answer within {} s", timeout.as_secs_f64()),
        e => e.to_string(),
    }
}

/// The request `net.request` was given: a URL, or a table
/// (`{url, method, headers, body, query, timeout}`).
fn request(v: Value) -> mlua::Result<Request> {
    let bad = |what: &str| mlua::Error::runtime(format!("net.request: {what}"));
    let t = match v {
        Value::String(url) => return request_of(url.to_str()?.to_string(), None),
        Value::Table(t) => t,
        _ => return Err(bad("a URL, or a table with one")),
    };
    let url: String = t.get::<Option<String>>("url")?.ok_or_else(|| bad("a url, \"https://...\""))?;
    request_of(url, Some(&t))
}

fn request_of(mut url: String, t: Option<&Table>) -> mlua::Result<Request> {
    let bad = |what: &str| mlua::Error::runtime(format!("net.request: {what}"));
    let Some(t) = t else {
        return Ok(Request { method: "GET".into(), url, headers: Vec::new(), body: None, timeout: secs(TIMEOUT) });
    };
    let method = t.get::<Option<String>>("method")?.unwrap_or_else(|| "GET".into()).to_ascii_uppercase();
    if let Some(query) = t.get::<Option<Table>>("query")? {
        let mut pairs = Vec::new();
        for pair in query.pairs::<mlua::LuaString, mlua::LuaString>() {
            let (k, v) = pair.map_err(|_| bad("a query is names and their text"))?;
            pairs.push(format!("{}={}", encode(&k.as_bytes()), encode(&v.as_bytes())));
        }
        // In one order, whatever the table's.
        pairs.sort();
        if !pairs.is_empty() {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&pairs.join("&"));
        }
    }
    let mut headers = Vec::new();
    if let Some(h) = t.get::<Option<Table>>("headers")? {
        for pair in h.pairs::<String, mlua::LuaString>() {
            let (name, value) = pair.map_err(|_| bad("headers are names and their text"))?;
            headers.push((name, value.as_bytes().to_vec()));
        }
    }
    let body = match t.get::<Value>("body")? {
        Value::Nil => None,
        Value::String(s) => Some(s.as_bytes().to_vec()),
        Value::Buffer(b) => Some(b.to_vec()),
        _ => return Err(bad("a body is a string or a buffer")),
    };
    let timeout = t.get::<Option<f64>>("timeout")?.unwrap_or(TIMEOUT);
    if timeout.is_nan() || timeout <= 0.0 {
        return Err(bad("a timeout is seconds, more than 0"));
    }
    Ok(Request { method, url, headers, body, timeout: secs(timeout) })
}

fn secs(s: f64) -> Duration {
    Duration::from_secs_f64(s.min(TIMEOUT_MAX))
}

/// Percent-encoding of a URL's query part (all but the unreserved).
fn encode(text: &[u8]) -> String {
    let mut out = String::with_capacity(text.len());
    for &b in text {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Adds `net` to a mod's VM.
pub fn install(lua: &Lua) -> mlua::Result<()> {
    let flight = Arc::new(AtomicUsize::new(0));
    // A request, on a thread of its own; nil while the mod has its
    // requests' worth going.
    let start = lua.create_function(move |_, v: Value| {
        let r = request(v)?;
        if flight.load(Ordering::Relaxed) >= IN_FLIGHT {
            return Ok(None);
        }
        let what = format!("net.request({} {})", r.method, r.url);
        let slot = Slot::default();
        let (s, f) = (slot.clone(), flight.clone());
        flight.fetch_add(1, Ordering::Relaxed);
        glue5_runtime::qos::spawn("mod net", glue5_runtime::qos::Qos::Utility, move || {
            let reply = std::panic::catch_unwind(|| run(r)).unwrap_or_else(|_| Err("the request failed".into()));
            *crate::lock(&s) = Some(reply);
            f.fetch_sub(1, Ordering::Relaxed);
        })
        .map_err(|e| {
            flight.fetch_sub(1, Ordering::Relaxed);
            mlua::Error::runtime(format!("{what}: no thread for it: {e}"))
        })?;
        Ok(Some(Pending { what, slot }))
    })?;
    // Its answer, once there is one.
    let poll = lua.create_function(|lua, pending: AnyUserData| {
        let pending = pending.borrow::<Pending>()?;
        let Some(reply) = crate::lock(&pending.slot).take() else { return Ok(Value::Nil) };
        let reply = reply.map_err(|e| mlua::Error::runtime(format!("{}: {e}", pending.what)))?;
        let t = lua.create_table()?;
        t.set("ok", reply.status.is_success())?;
        t.set("statusCode", reply.status.as_u16())?;
        t.set("statusMessage", reply.status.canonical_reason().unwrap_or(""))?;
        let headers = lua.create_table()?;
        for (name, value) in reply.headers {
            headers.set(name, value)?;
        }
        t.set("headers", headers)?;
        t.set("body", lua.create_string(reply.body)?)?;
        Ok(Value::Table(t))
    })?;
    let request: Function = lua
        .load(
            "local start, poll, yield = ...\n\
             return function(config)\n\
             local pending = start(config)\n\
             while not pending do yield(0) pending = start(config) end\n\
             while true do\n\
             local reply = poll(pending)\n\
             if reply then return reply end\n\
             yield(0)\n\
             end end",
        )
        .set_name("=net.request")
        .call((start, poll, lua.globals().get::<Table>("coroutine")?.get::<Function>("yield")?))?;
    let net = lua.create_table()?;
    net.set("request", request)?;
    crate::socket::install(lua, &net)?;
    lua.globals().set("net", net)
}

/// The definitions of `net`, for the type checker (Lune's types).
pub const DEFINITIONS: &str = "
export type HttpMethod = \"GET\" | \"POST\" | \"PUT\" | \"DELETE\" | \"HEAD\" | \"OPTIONS\" | \"PATCH\"
export type FetchParams = {
\turl: string,
\t-- \"GET\" by default.
\tmethod: HttpMethod?,
\tbody: (string | buffer)?,
\t-- Added to the URL, encoded.
\tquery: { [string]: string }?,
\theaders: { [string]: string }?,
\t-- Seconds until it gives up (30 by default, 300 at most); GLUE 5's, not Lune's.
\ttimeout: number?,
}
export type WebSocket = {
\t-- How it was closed (1000: normally), once it is; nil while open.
\tcloseCode: number?,
\t-- Closes it (1000 by default); next() returns nil once the other end agrees.
\tclose: (self: WebSocket, code: number?) -> (),
\t-- A text message (UTF-8), or binary with asBinaryMessage.
\tsend: (self: WebSocket, message: (string | buffer)?, asBinaryMessage: boolean?) -> (),
\t-- The next message; waits for it as wait() does; nil once the socket is closed.
\tnext: (self: WebSocket) -> string?,
}
export type FetchResponse = {
\t-- A 2xx status.
\tok: boolean,
\tstatusCode: number,
\tstatusMessage: string,
\t-- By lowercase name; repeated ones joined with \", \".
\theaders: { [string]: string },
\tbody: string,
}

-- The network, as Lune's @lune/net. Requests wait, as wait() does, while they go on away from the game:
-- the game goes on meanwhile. An answer with any status is a response (`ok` for 2xx); no answer (no
-- network, a bad address, the timeout) is an error. A mod has 4 requests going at once at most (more
-- wait for one to end); an answer's body may be 16 MB (gzip is unpacked).
declare net: {
\t-- An HTTP or HTTPS request: a URL (GET), or what FetchParams says.
\trequest: (config: string | FetchParams) -> FetchResponse,
\t-- A WebSocket, ws:// or wss://; waits while it connects. A mod has 4 open at once, messages of 16 MB.
\tsocket: (url: string) -> WebSocket,
}
";

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use super::*;

    /// Runs `code` as the engine would: a coroutine resumed until it ends.
    fn run_mod(lua: &Lua, code: &str) -> mlua::Result<Value> {
        let thread = lua.create_thread(lua.load(code).into_function()?)?;
        let start = std::time::Instant::now();
        loop {
            let v: Value = thread.resume(())?;
            if !thread.is_resumable() {
                return Ok(v);
            }
            assert!(start.elapsed() < Duration::from_secs(20), "no answer");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A server on this computer that answers `answers` requests with what
    /// it was sent, in its body.
    fn echo(answers: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(answers) {
                let mut stream = stream.unwrap();
                let mut got = Vec::new();
                let mut buf = [0; 4096];
                // Headers, then the body its length says.
                loop {
                    let n = stream.read(&mut buf).unwrap();
                    got.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&got);
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if got.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                let reply = format!(
                    "HTTP/1.1 201 Created\r\nX-A: 1\r\nX-A: 2\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    got.len()
                );
                stream.write_all(reply.as_bytes()).unwrap();
                stream.write_all(&got).unwrap();
            }
        });
        format!("http://{address}")
    }

    #[test]
    fn requests_wait_for_their_answer() {
        let lua = Lua::new();
        install(&lua).unwrap();
        lua.globals().set("base", echo(2)).unwrap();
        let v = run_mod(
            &lua,
            r#"
            local r = net.request({ url = base .. "/x", method = "POST", body = "hello", headers = { ["X-Mod"] = "garage" }, query = { b = "x y", a = "é" } })
            assert(r.statusCode == 201 and r.statusMessage == "Created" and r.ok and r.headers["x-a"] == "1, 2", r.statusCode)
            assert(r.body:find("POST /x?a=%C3%A9&b=x%20y HTTP/1.1", 1, true), r.body)
            assert(r.body:find("x-mod: garage", 1, true) and r.body:find("hello$"))
            local g = net.request({ url = base .. "/y", method = "put", body = buffer.fromstring("b") })
            return g.body:sub(1, 6)
            "#,
        )
        .unwrap();
        assert_eq!(v.as_string().unwrap().to_str().unwrap(), "PUT /y");
    }

    #[test]
    fn failures_are_errors() {
        let lua = Lua::new();
        install(&lua).unwrap();
        // Nothing listens there.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let e = run_mod(&lua, &format!("return net.request('http://127.0.0.1:{port}/')")).unwrap_err().to_string();
        assert!(e.contains(&format!("net.request(GET http://127.0.0.1:{port}/)")), "{e}");
        let e = run_mod(&lua, "return net.request('ftp://example.com/')").unwrap_err().to_string();
        assert!(e.contains("net.request"), "{e}");
        let e = run_mod(&lua, "return net.request({})").unwrap_err().to_string();
        assert!(e.contains("a url"), "{e}");
        // An answer that never comes.
        let silent = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", silent.local_addr().unwrap());
        let e =
            run_mod(&lua, &format!("return net.request({{ url = '{url}', timeout = 0.3 }})")).unwrap_err().to_string();
        assert!(e.contains("no answer within 0.3 s"), "{e}");
    }

    /// HTTPS to the internet (`--ignored`: it needs the network).
    #[test]
    #[ignore]
    fn https() {
        let lua = Lua::new();
        install(&lua).unwrap();
        let v = run_mod(&lua, "local r = net.request('https://example.com/') return r.statusCode == 200 and r.body:find('Example Domain') ~= nil").unwrap();
        assert_eq!(v, Value::Boolean(true));
    }
}
