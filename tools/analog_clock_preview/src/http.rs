//! Localhost-only preview server, std-only HTTP. Every picture is
//! rendered by the shared core from the request's settings; the page at
//! `/` only shows sliders and the resulting PNG.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;

use crate::assets::DecodedHands;
use crate::params::{OutputMode, PreviewParams};

const INDEX: &str = include_str!("../index.html");

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    }
}

fn reply(stream: &mut std::net::TcpStream, code: u16, content_type: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {code} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reason(code),
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

/// Percent-decodes one query component (`+` becomes a space).
fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| match b {
                    b'0'..=b'9' => Some(b - b'0'),
                    b'a'..=b'f' => Some(b - b'a' + 10),
                    b'A'..=b'F' => Some(b - b'A' + 10),
                    _ => None,
                };
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(a), Some(b)) => {
                        out.push((a * 16 + b) as char);
                        i += 2;
                    }
                    _ => out.push('%'),
                }
            }
            b => out.push(b as char),
        }
        i += 1;
    }
    out
}

fn params_from_query(query: &str) -> Result<PreviewParams, String> {
    let mut params = PreviewParams::default();
    if query.is_empty() {
        return Ok(params);
    }
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| format!("setting {pair:?} needs a value"))?;
        params.apply(&unescape(key), &unescape(value))?;
    }
    Ok(params)
}

/// One encoded `/render` body plus the comparison headers for it (empty
/// unless diagnostics is `1` or the mode is `changes`).
#[derive(Debug, PartialEq)]
pub struct Rendered {
    pub png: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

fn encode_png(
    params: &PreviewParams,
    hands: &DecodedHands,
    cache: &mut crate::frame::FrameCache,
) -> Result<Rendered, String> {
    // The one HTTP attach point: clone the validated params, then set the
    // supplied dial, so legacy, gray, composed, and changes views all share
    // the same image with no extra argument or threading.
    let mut owned = params.clone();
    owned.dial = hands.dial.clone();
    let params = &owned;
    let borrowed = crate::params::hands(params, hands);
    if !borrowed.validate() {
        return Err("decoded assets failed validation".to_string());
    }
    let (w, h) = (params.width, params.height);
    // Every view renders through the shared core (unified `frame` entry,
    // one-entry static cache): gray directly, dots via the composition
    // pipeline. No JS or host-side lighting or dither clone anywhere on
    // this path. With diagnostics off and a non-changes mode the PNG bytes
    // are unchanged from the pre-composition path.
    let pair = crate::frame::render_pair(cache, params, &borrowed)?;
    let (raw, color): (Vec<u8>, image::ExtendedColorType) = match params.mode {
        OutputMode::Gray => (pair.current.gray.clone(), image::ExtendedColorType::L8),
        OutputMode::Dithered => (
            crate::params::bits_to_gray(&pair.current.bits, w as usize * h as usize),
            image::ExtendedColorType::L8,
        ),
        OutputMode::Changes => {
            let prev = pair
                .previous
                .as_ref()
                .ok_or_else(|| "changes view needs a reference frame".to_string())?;
            (
                crate::comparison::changed_overlay_rgb(w, h, &pair.current.bits, &prev.bits)?,
                image::ExtendedColorType::Rgb8,
            )
        }
    };
    let mut png = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png);
    image::ImageEncoder::write_image(encoder, &raw, w, h, color)
        .map_err(|e| format!("PNG encode failed: {e}"))?;
    let mut headers = Vec::new();
    if params.diagnostics || params.mode == OutputMode::Changes {
        let m = pair
            .metrics
            .as_ref()
            .ok_or_else(|| "diagnostics needs comparison metrics".to_string())?;
        headers.push((
            "X-Clock-Changed-Total".to_string(),
            m.changed_total.to_string(),
        ));
        headers.push((
            "X-Clock-Changed-Outside".to_string(),
            m.changed_outside.to_string(),
        ));
        headers.push((
            "X-Clock-Disocclusion-Mismatches".to_string(),
            m.disocclusion_mismatches.to_string(),
        ));
        headers.push(("X-Clock-Tone-MAE".to_string(), m.tone_mae.to_string()));
        headers.push(("X-Clock-Tone-Bias".to_string(), m.tone_bias.to_string()));
        headers.push((
            "X-Clock-Boundary-MAE".to_string(),
            m.boundary_mae.to_string(),
        ));
        headers.push((
            "X-Clock-Compare-Time".to_string(),
            format!("{:02}:{:02}", params.compare_hours, params.compare_minutes),
        ));
        headers.push((
            "X-Clock-Base-Reused".to_string(),
            pair.current.base_reused.to_string(),
        ));
    }
    Ok(Rendered { png, headers })
}

fn handle(
    mut stream: std::net::TcpStream,
    hands: &DecodedHands,
    cache: &mut crate::frame::FrameCache,
) {
    let mut head = [0u8; 8192];
    let Ok(n) = stream.read(&mut head) else {
        return;
    };
    let text = String::from_utf8_lossy(&head[..n]);
    let line = text.lines().next().unwrap_or("");
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if method != "GET" {
        reply(&mut stream, 405, "text/plain", b"GET only");
        return;
    }
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (target, ""),
    };
    match path {
        "/" | "/index.html" => reply(
            &mut stream,
            200,
            "text/html; charset=utf-8",
            INDEX.as_bytes(),
        ),
        "/render" => {
            let started = std::time::Instant::now();
            // Host-owned one-entry static cache lives in the serve loop
            // (never global, never per request): the current and
            // reference frames share one base, so the second render is
            // an exact byte hit, and identical later requests reuse the
            // entry across connections; any base input change replaces
            // it. Invalid requests and the index page return before
            // touching the cache, so they never discard it.
            match params_from_query(query)
                .and_then(|p| encode_png(&p, hands, cache).map(|r| (p, r)))
            {
                Ok((_, rendered)) => {
                    let ms = started.elapsed().as_secs_f64() * 1000.0;
                    let mut head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nX-Render-Ms: {ms:.1}\r\n",
                        rendered.png.len()
                    );
                    for (name, value) in &rendered.headers {
                        head.push_str(&format!("{name}: {value}\r\n"));
                    }
                    head.push_str("Connection: close\r\n\r\n");
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&rendered.png);
                }
                Err(message) => reply(&mut stream, 400, "text/plain", message.as_bytes()),
            }
        }
        _ => reply(&mut stream, 404, "text/plain", b"unknown path"),
    }
}

/// Serves the preview on 127.0.0.1 only; one connection at a time keeps
/// renders from piling up behind rapid slider movement.
pub fn serve(port: u16, hands: DecodedHands) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("cannot bind 127.0.0.1:{port}: {e}"))?;
    println!("analog preview at http://127.0.0.1:{port}/ (localhost only, Ctrl-C to stop)");
    let shared = Arc::new(hands);
    // One bounded host-owned cache across connections; the server
    // handles one connection at a time, so no locking is needed.
    let mut cache = crate::frame::FrameCache::new();
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle(stream, &shared, &mut cache),
            Err(e) => eprintln!("connection failed: {e}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_hands() -> DecodedHands {
        // 8x8 flat opaque stand-ins: exercises the endpoint path without
        // touching the real assets.
        let n = 8usize * 8;
        let hand = || crate::assets::DecodedHand {
            width: 8,
            height: 8,
            albedo: vec![128u8; n * 3],
            alpha: vec![255u8; n],
            normal: {
                let mut normal = vec![0u8; n * 3];
                for px in normal.chunks_exact_mut(3) {
                    px[0] = 127;
                    px[1] = 127;
                    px[2] = 255;
                }
                normal
            },
            spec: vec![128u8; n],
        };
        DecodedHands {
            hour: hand(),
            minute: hand(),
            dial: None,
        }
    }

    #[test]
    fn defaults_render_without_query() {
        let params = params_from_query("").expect("empty query");
        assert_eq!((params.width, params.height), (600, 600));
    }

    #[test]
    fn bad_values_are_errors_not_panics() {
        for query in [
            "w=99999",
            "w=abc",
            "time=25:00",
            "time=nope",
            "bogus=1",
            "samples=0",
            "hour_px=245",
            "hour_px=-1",
            "hour_py=0",
            "hour_py=810",
            "minute_px=156",
            "minute_py=1014",
            "hour_spec=9",
            "minute_spec=-1",
        ] {
            assert!(params_from_query(query).is_err(), "{query} should fail");
        }
    }

    #[test]
    fn tiny_hands_render_with_default_pivots() {
        // Small stand-ins borrow in-bounds center pivots instead of the
        // full-size canonical values, so defaults validate and encode.
        let hands = tiny_hands();
        assert!(hands.as_hands().validate());
        let params = params_from_query("w=64&h=64&time=10:09").expect("query");
        assert!(params.hour_pivot.is_none());
        encode_png(&params, &hands, &mut crate::frame::FrameCache::new()).expect("encode");
    }

    #[test]
    fn per_hand_spec_gains_change_the_picture() {
        let hands = tiny_hands();
        let base = params_from_query("w=64&h=64&time=10:09").expect("query");
        let png_base = encode_png(&base, &hands, &mut crate::frame::FrameCache::new())
            .expect("encode")
            .png;
        let muted =
            params_from_query("w=64&h=64&time=10:09&hour_spec=0&minute_spec=0").expect("query");
        let png_muted = encode_png(&muted, &hands, &mut crate::frame::FrameCache::new())
            .expect("encode")
            .png;
        assert_ne!(png_base, png_muted, "spec gains had no effect");
    }

    #[test]
    fn encoded_body_is_a_matching_png() {
        let hands = tiny_hands();
        let params = params_from_query(
            "w=64&h=64&time=03:00&mode=gray&hour_px=4&hour_py=4&minute_px=4&minute_py=4",
        )
        .expect("query");
        let png = encode_png(&params, &hands, &mut crate::frame::FrameCache::new())
            .expect("encode")
            .png;
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[16..24], [0, 0, 0, 64, 0, 0, 0, 64]);
        let dithered = params_from_query(
            "w=64&h=64&mode=dither&dither=bayer4&hour_px=4&hour_py=4&minute_px=4&minute_py=4",
        )
        .expect("query");
        let png = encode_png(&dithered, &hands, &mut crate::frame::FrameCache::new())
            .expect("encode")
            .png;
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // A moved rotation center must move the picture: pivot wiring.
        let shifted =
            params_from_query("w=64&h=64&time=03:00&hour_px=6&hour_py=4&minute_px=4&minute_py=4")
                .expect("query");
        let png_shifted = encode_png(&shifted, &hands, &mut crate::frame::FrameCache::new())
            .expect("encode")
            .png;
        let png_base = encode_png(&params, &hands, &mut crate::frame::FrameCache::new())
            .expect("encode")
            .png;
        assert_ne!(png_shifted, png_base, "pivot change had no effect");
    }

    #[test]
    fn unescape_handles_forms() {
        assert_eq!(unescape("10%3A09"), "10:09");
        assert_eq!(unescape("a+b"), "a b");
    }

    #[test]
    fn legacy_dither_query_equals_setting_all_four() {
        let hands = tiny_hands();
        let legacy =
            params_from_query("w=64&h=64&time=10:09&mode=dither&dither=none").expect("query");
        let split = params_from_query(
            "w=64&h=64&time=10:09&mode=dither&background_dither=none&clock_dither=none&hands_dither=none&shadows_dither=none",
        )
        .expect("query");
        assert_eq!(
            encode_png(&legacy, &hands, &mut crate::frame::FrameCache::new())
                .expect("encode")
                .png,
            encode_png(&split, &hands, &mut crate::frame::FrameCache::new())
                .expect("encode")
                .png,
            "legacy dither= must set all four profiles"
        );
    }

    #[test]
    fn bad_region_values_are_errors() {
        for query in [
            "background_dither=bayer",
            "clock_dither=ordered",
            "hands_dither=atkinson2",
            "shadows_dither=",
            "dither=diffusion",
        ] {
            assert!(params_from_query(query).is_err(), "{query} should fail");
        }
    }

    /// Each single-region change moves the dots picture inside that
    /// region's own geometric mask (from the shared `render_region_row`
    /// plane) while the composed grayscale stays bit-identical: patterns
    /// never change the lighting.
    #[test]
    fn individual_region_changes_move_dots_inside_their_mask() {
        use analog_clock::DitherRegion;
        let hands = tiny_hands();
        // Default light keeps the paper Background mask populated; grazing
        // light throws long shadows so the Shadows mask is populated even
        // on the 64x64 fixture. Each region is checked in the first scene
        // whose geometric mask actually contains it.
        let scenes = [
            "w=64&h=64&time=10:09&mode=dither",
            "w=64&h=64&time=10:09&mode=dither&light_h=0.2&light_size=0",
        ];
        for (key, region) in [
            ("background_dither", DitherRegion::Background),
            ("clock_dither", DitherRegion::Clock),
            ("hands_dither", DitherRegion::Hands),
            ("shadows_dither", DitherRegion::Shadows),
        ] {
            let mut covered = false;
            for scene_query in scenes {
                let base = params_from_query(scene_query).expect("query");
                let borrowed = crate::params::hands(&base, &hands);
                let (base_gray, mask) = crate::params::render_planes(&base, &borrowed);
                if !mask.contains(&region) {
                    continue;
                }
                covered = true;
                let base_dots = crate::params::bits_to_gray(
                    &crate::params::render_dithered(&base, &borrowed),
                    base.width as usize * base.height as usize,
                );
                let mut varied = base.clone();
                varied.apply(key, "none").expect("valid method");
                let borrowed = crate::params::hands(&varied, &hands);
                // Lighting is untouched by pattern selection.
                assert_eq!(
                    crate::params::render_gray(&varied, &borrowed),
                    crate::params::render_gray(&base, &crate::params::hands(&base, &hands)),
                    "{key}: gray changed"
                );
                assert_eq!(
                    crate::params::render_planes(&varied, &borrowed).0,
                    base_gray,
                    "{key}: composed plane changed"
                );
                let dots = crate::params::bits_to_gray(
                    &crate::params::render_dithered(&varied, &borrowed),
                    varied.width as usize * varied.height as usize,
                );
                assert_ne!(dots, base_dots, "{key}: dots picture did not change");
                let moved = dots
                    .iter()
                    .zip(&base_dots)
                    .zip(&mask)
                    .filter(|((a, b), r)| a != b && **r == region)
                    .count();
                assert!(
                    moved > 0,
                    "{key}: dots changed only outside the {region:?} mask"
                );
                break;
            }
            assert!(covered, "{key}: no {region:?} pixels in any scene mask");
        }
    }

    #[test]
    fn gray_png_ignores_pattern_selection() {
        let hands = tiny_hands();
        let base = params_from_query("w=64&h=64&time=10:09&mode=gray").expect("query");
        let varied = params_from_query(
            "w=64&h=64&time=10:09&mode=gray&background_dither=none&clock_dither=none&hands_dither=none&shadows_dither=none",
        )
        .expect("query");
        assert_eq!(
            encode_png(&base, &hands, &mut crate::frame::FrameCache::new())
                .expect("encode")
                .png,
            encode_png(&varied, &hands, &mut crate::frame::FrameCache::new())
                .expect("encode")
                .png,
            "gray view must not depend on dot patterns"
        );
    }

    #[test]
    fn gray_png_identical_across_compositions() {
        let hands = tiny_hands();
        let mut pngs = Vec::new();
        for composition in ["legacy", "reference", "replacement", "removal"] {
            let params = params_from_query(&format!(
                "w=64&h=64&time=10:09&mode=gray&composition={composition}"
            ))
            .expect("query");
            pngs.push(
                encode_png(&params, &hands, &mut crate::frame::FrameCache::new())
                    .expect("encode")
                    .png,
            );
        }
        for png in &pngs[1..] {
            assert_eq!(&pngs[0], png, "gray PNG must not depend on composition");
        }
    }

    #[test]
    fn changes_view_reports_rgb_and_headers() {
        let hands = tiny_hands();
        let params = params_from_query(
            "w=64&h=64&time=10:09&compare_time=10:10&mode=changes&composition=replacement",
        )
        .expect("query");
        let rendered =
            encode_png(&params, &hands, &mut crate::frame::FrameCache::new()).expect("encode");
        let img = image::load_from_memory(&rendered.png)
            .expect("decode changes PNG")
            .to_rgb8();
        assert_eq!((img.width(), img.height()), (64, 64));
        let value = |name: &str| {
            rendered
                .headers
                .iter()
                .find(|(k, _)| k == name)
                .unwrap_or_else(|| panic!("missing header {name}"))
                .1
                .clone()
        };
        value("X-Clock-Changed-Total")
            .parse::<usize>()
            .expect("int header");
        value("X-Clock-Changed-Outside")
            .parse::<usize>()
            .expect("int header");
        value("X-Clock-Disocclusion-Mismatches")
            .parse::<usize>()
            .expect("int header");
        value("X-Clock-Tone-MAE")
            .parse::<f64>()
            .expect("float header");
        value("X-Clock-Tone-Bias")
            .parse::<f64>()
            .expect("float header");
        value("X-Clock-Boundary-MAE")
            .parse::<f64>()
            .expect("float header");
        assert_eq!(value("X-Clock-Compare-Time"), "10:10");
        let reused = value("X-Clock-Base-Reused");
        assert!(reused == "true" || reused == "false", "base flag: {reused}");
    }

    #[test]
    fn serve_loop_cache_reuses_static_base_across_requests() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind test listener");
        let address = listener.local_addr().expect("listener address");
        let server = std::thread::spawn(move || {
            let hands = tiny_hands();
            let mut cache = crate::frame::FrameCache::new();
            for _ in 0..5 {
                let (stream, _) = listener.accept().expect("accept request");
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .expect("server read timeout");
                handle(stream, &hands, &mut cache);
            }
        });
        let request = |query: &str| {
            let mut stream = std::net::TcpStream::connect(address).expect("connect");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .expect("client read timeout");
            let request = format!("GET /render?{query} HTTP/1.1\r\nHost: localhost\r\n\r\n");
            stream.write_all(request.as_bytes()).expect("request");
            let mut response = Vec::new();
            stream.read_to_end(&mut response).expect("response");
            let header_end = response
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .expect("header terminator");
            String::from_utf8(response[..header_end].to_vec()).expect("headers")
        };
        let base = "w=64&h=64&compare_time=10:08&composition=replacement&diagnostics=1";
        let first = request(&format!("{base}&time=10:09"));
        assert!(first.starts_with("HTTP/1.1 200"));
        assert!(first.contains("X-Clock-Base-Reused: false"));
        assert!(request(&format!("{base}&time=10:10")).contains("X-Clock-Base-Reused: true"));
        let lit = format!("{base}&time=10:10&light_h=0.2");
        assert!(request(&lit).contains("X-Clock-Base-Reused: false"));
        assert!(request("w=abc").starts_with("HTTP/1.1 400"));
        assert!(request(&lit).contains("X-Clock-Base-Reused: true"));
        server.join().expect("server thread");
    }

    #[test]
    fn bad_compare_time_and_composition_fail() {
        for query in [
            "compare_time=25:00",
            "compare_time=nope",
            "composition=bogus",
            "mode=bogus",
            "diagnostics=2",
        ] {
            assert!(params_from_query(query).is_err(), "{query} should fail");
        }
    }
}
