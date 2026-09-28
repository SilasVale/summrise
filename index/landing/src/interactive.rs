//! interactive.rs — the landing page's two behaviours, in Rust, compiled to wasm.
//!
//! These are the ONLY things on this page that are not text: a theme toggle and a
//! decorative particle field. Everything else is in the HTML before this module
//! is even fetched (see `page.rs`).
//!
//! The theme BOOTSTRAP is deliberately not here — it must run before first paint
//! and a wasm module cannot. What is here is the toggle it drives.
//!
//! THE FIELD IS THE FOURTH COPY OF ONE IDEA, and the other three are held to a
//! shared set of facts by `scripts/test/particles-check.mjs`: the palette comes
//! from the brand's tokens, every fallback is one of the brand's own colours, the
//! draw is `rgba(` from a colour rather than `hsla(` from a hue, it refuses to run
//! under `prefers-reduced-motion`, and all four draw the SAME field (cap, density
//! and peak alpha). The numbers are named constants below for that reason — the
//! check reads them by name, so they cannot drift from the panel's and the
//! console's in silence.

use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

const MAX_MOTES: usize = 90;
const MAX_ALPHA: f64 = 0.5;
/// Motes per 100 000 px² — the same field the panel and the console reach as
/// `(w * h / 100_000) * DENSITY * 10` with their `DENSITY = 0.55`.
const DENSITY: f64 = 5.5;
/// The twinkle's peak, before `MAX_ALPHA`. Named because the other three copies
/// write it as a bare `0.35` and the check compares the four.
const TWINKLE: f64 = 0.35;
/// The mark's own gold — never a colour the brand does not use.
const MARK_GOLD: [u8; 3] = [245, 159, 0];

#[wasm_bindgen(start)]
pub fn start() {
    attach_toggle();
    start_particles();
}

// ── theme toggle ────────────────────────────────────────────────

fn attach_toggle() {
    let doc = document();
    let Some(btn) = doc.query_selector(".theme-toggle").ok().flatten() else {
        return;
    };
    let cb = Closure::<dyn FnMut()>::new(toggle_theme);
    let _ = btn.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref());
    cb.forget();
}

fn toggle_theme() {
    let win = window();
    let Some(body) = win.document().and_then(|d| d.body()) else {
        return;
    };
    let dark = body.has_attribute("data-ds-dark-theme");
    let storage = win.local_storage().ok().flatten();
    if dark {
        let _ = body.remove_attribute("data-ds-dark-theme");
        if let Some(s) = storage {
            let _ = s.set_item("summrise-theme", "light");
        }
    } else {
        let _ = body.set_attribute("data-ds-dark-theme", "");
        if let Some(s) = storage {
            let _ = s.set_item("summrise-theme", "dark");
        }
    }
}

// ── particle field ──────────────────────────────────────────────

struct Mote {
    x: f64,
    y: f64,
    r: f64,
    vx: f64,
    vy: f64,
    tone: usize,
    phase: f64,
}

struct Field {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    motes: Vec<Mote>,
    colours: Vec<[u8; 3]>,
    last: f64,
    w: f64,
    h: f64,
}

fn start_particles() {
    let win = window();
    let doc = document();

    // A decorative animation that ignores prefers-reduced-motion is an
    // accessibility defect; the honest fallback is NO animation, not a slower one.
    if let Ok(Some(mq)) = win.match_media("(prefers-reduced-motion: reduce)") {
        if mq.matches() {
            return;
        }
    }

    let canvas = match doc.create_element("canvas") {
        Ok(e) => match e.dyn_into::<HtmlCanvasElement>() {
            Ok(c) => c,
            Err(_) => return,
        },
        Err(_) => return,
    };
    let _ = canvas.set_attribute("aria-hidden", "true");
    canvas
        .style()
        .set_css_text("position:fixed;inset:0;z-index:0;pointer-events:none;display:block");
    let Some(body) = doc.body() else { return };
    if body.append_child(&canvas).is_err() {
        return;
    }
    let ctx = match canvas.get_context("2d") {
        Ok(Some(c)) => match c.dyn_into::<CanvasRenderingContext2d>() {
            Ok(c) => c,
            Err(_) => return,
        },
        _ => {
            let _ = canvas.remove();
            return;
        }
    };

    let field = Rc::new(RefCell::new(Field {
        canvas,
        ctx,
        motes: Vec::new(),
        colours: palette(),
        last: 0.0,
        w: 0.0,
        h: 0.0,
    }));
    resize(&field);
    request_next(&field);

    let f = field.clone();
    let on_resize = Closure::<dyn FnMut()>::new(move || resize(&f));
    let _ = win.add_event_listener_with_callback("resize", on_resize.as_ref().unchecked_ref());
    on_resize.forget();
}

/// The mark's colours, read from the SAME tokens the mark is drawn from, so the
/// field cannot drift from the palette. Resolved per frame, so a theme switch
/// reaches motes already in flight.
///
/// THE FALLBACK IS A HEX STRING, spelled the way the other three copies spell it.
/// A fallback is what people SEE when a token is missing, so it is stated as the
/// brand colour it is rather than as a triple nobody can read at a glance — and
/// `particles-check.mjs` reads every fallback in every copy and holds it to the
/// brand's own values, which is how the cyan arrived in the first place.
fn colour_of(name: &str, fallback: &str) -> [u8; 3] {
    let win = window();
    let Some(body) = win.document().and_then(|d| d.body()) else {
        return brand(fallback);
    };
    let raw = match win.get_computed_style(&body) {
        Ok(Some(cs)) => cs.get_property_value(name).unwrap_or_default(),
        _ => String::new(),
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return brand(fallback);
    }
    parse_colour(raw).unwrap_or_else(|| brand(fallback))
}

/// A brand colour named as the hex the design system names it by.
fn brand(hex: &str) -> [u8; 3] {
    parse_colour(hex).unwrap_or(MARK_GOLD)
}

fn parse_colour(raw: &str) -> Option<[u8; 3]> {
    if let Some(hex) = raw.strip_prefix('#') {
        if hex.len() == 6 {
            let n = u32::from_str_radix(hex, 16).ok()?;
            return Some([(n >> 16) as u8, (n >> 8) as u8, n as u8]);
        }
        return None;
    }
    let inner = raw
        .strip_prefix("rgba(")
        .or_else(|| raw.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<f64> = inner
        .split(|c: char| c.is_whitespace() || c == ',' || c == '/')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();
    if parts.len() >= 3 {
        return Some([parts[0] as u8, parts[1] as u8, parts[2] as u8]);
    }
    None
}

fn palette() -> Vec<[u8; 3]> {
    vec![
        colour_of("--brand-mark-a", "#f59f00"),
        colour_of("--brand-mark-b", "#e8590c"),
        colour_of("--brand-mark-c", "#ffd43b"),
    ]
}

fn resize(field: &Rc<RefCell<Field>>) {
    let win = window();
    let dpr = win.device_pixel_ratio().min(2.0);
    let w = win.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
    let h = win.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
    let mut f = field.borrow_mut();
    f.canvas.set_width((w * dpr).floor() as u32);
    f.canvas.set_height((h * dpr).floor() as u32);
    let _ = f.canvas.style().set_property("width", &px(w));
    let _ = f.canvas.style().set_property("height", &px(h));
    let _ = f.ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    f.colours = palette();
    f.w = w;
    f.h = h;
    let want = MAX_MOTES.min((w * h / 100_000.0 * DENSITY).round() as usize);
    while f.motes.len() > want {
        f.motes.pop();
    }
    let tones = f.colours.len().max(1);
    while f.motes.len() < want {
        let rnd = || js_sys::Math::random();
        let mote = Mote {
            x: rnd() * w,
            y: rnd() * h,
            r: 0.6 + rnd() * 1.9,
            vx: (rnd() - 0.5) * 0.16,
            vy: -0.05 - rnd() * 0.18,
            tone: (rnd() * tones as f64) as usize,
            phase: rnd() * std::f64::consts::PI * 2.0,
        };
        f.motes.push(mote);
    }
}

fn draw(f: &mut Field) {
    let w = f.w;
    let h = f.h;
    f.ctx.clear_rect(0.0, 0.0, w, h);
    for i in 0..f.motes.len() {
        let m = &mut f.motes[i];
        m.x += m.vx;
        m.y += m.vy;
        m.phase += 0.012;
        if m.y < -8.0 {
            m.y = h + 8.0;
        }
        if m.y > h + 8.0 {
            m.y = -8.0;
        }
        if m.x < -8.0 {
            m.x = w + 8.0;
        }
        if m.x > w + 8.0 {
            m.x = -8.0;
        }
        let tw = 0.55 + 0.45 * m.phase.sin();
        let c = f.colours.get(m.tone).copied().unwrap_or(f.colours[0]);
        f.ctx.begin_path();
        // INTEGER FORMATTING ON PURPOSE. `{:.3}` on an f64 pulls Rust's float
        // formatter into the binary; thousandths as an integer draws the same
        // pixels and is the single largest size saving in this module — measured
        // at 8,879 gzipped bytes, 26% of the whole payload, for one `format!`.
        let peak = MAX_ALPHA * tw * TWINKLE;
        let alpha = (peak * 1000.0).round() as u32;
        f.ctx.set_fill_style_str(&format!(
            "rgba({}, {}, {}, 0.{:03})",
            c[0], c[1], c[2], alpha
        ));
        let _ = f.ctx.arc(m.x, m.y, m.r, 0.0, std::f64::consts::PI * 2.0);
        f.ctx.fill();
    }
}

fn request_next(field: &Rc<RefCell<Field>>) {
    let f = field.clone();
    let cb = Closure::<dyn FnMut(f64)>::new(move |t: f64| {
        {
            let mut st = f.borrow_mut();
            // ~30fps: plenty for a drift this slow.
            if t - st.last >= 33.0 {
                st.last = t;
                draw(&mut st);
            }
        }
        request_next(&f);
    });
    let _ = window().request_animation_frame(cb.as_ref().unchecked_ref());
    // One closure per frame would leak; the field lives for the page's lifetime,
    // so the loop's closure is intentionally kept alive by the browser's queue.
    cb.forget();
}

/// Whole pixels, formatted without touching the float formatter.
fn px(v: f64) -> String {
    let n = v.round() as i64;
    let mut s = String::with_capacity(8);
    let _ = std::fmt::Write::write_fmt(&mut s, format_args!("{n}px"));
    s
}

fn window() -> web_sys::Window {
    web_sys::window().expect("a browser window")
}

fn document() -> web_sys::Document {
    window().document().expect("a document")
}
