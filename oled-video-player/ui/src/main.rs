use dioxus::prelude::*;
use gloo_net::http::Request;
use gloo_timers::future::TimeoutFuture;
use js_sys::Uint8Array;
use wasm_bindgen::{Clamped, JsCast};
use wasm_bindgen_futures::JsFuture;

use web_sys::{
    CanvasRenderingContext2d, HtmlCanvasElement, HtmlInputElement, HtmlSelectElement,
    HtmlVideoElement, ImageData, Url,
};

const FRAME_SIZE: usize = 1024;
const WIDTH: usize = 128;
const HEIGHT: usize = 64;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut esp_ip = use_signal(|| "192.168.100.13".to_string());
    let mut status = use_signal(|| "Waiting for a video".to_string());
    let mut file_name = use_signal(|| "No video selected".to_string());
    let mut streaming = use_signal(|| false);

    rsx! {
        document::Link { rel: "stylesheet", href: asset!("/assets/main.css") }
        div { class: "topbar",
            div { class: "brand",
                span { class: "brand-mark", span {} span {} span {} span {} }
                "OLED Video Player"
            }
            div { class: "topbar-meta", "128 × 64 · 1-bit · ESP32" }
        }
        main { class: "app-shell",
            header { class: "hero",
                div {
                    div { class: "eyebrow", "Video → OLED frame buffer" }
                    h1 { "Stream video to your OLED, with no surprises." }
                    p { "Upload a video, tune the monochrome conversion, and inspect the exact 128 × 64 frame before it is sent to your ESP32." }
                    div { class: "chip-row",
                        span { class: "chip", "Page-major bytes" }
                        span { class: "chip", "Bayer 8×8" }
                        span { class: "chip", "Live preview" }
                    }
                }
                div { class: "hero-aside",
                    div { class: "stat-card",
                        strong { "Firmware-compatible output" }
                        span { "Every frame stays 1,024 bytes: eight vertical pages, one byte per X coordinate." }
                    }
                    div { class: "stat-card",
                        strong { "One conversion path" }
                        span { "The preview is generated from the same pixels and settings posted to the device." }
                    }
                }
            }
            section { class: "workspace",
                article { class: "panel",
                    PanelHeader { step: "1", title: "Input & settings", description: "Choose a source, connect the ESP32, and tune the conversion.", badge: "Controls" }
                    div { class: "panel-body control-stack",
                        div { class: "control-card",
                            h3 { "Video source" }
                            p { class: "hint", "{file_name}" }
                            input {
                                id: "video-file", r#type: "file", accept: "video/*",
                                onchange: move |_| match load_selected_video() {
                                    Ok(name) => {
                                        file_name.set(name);
                                        status.set("Video loaded. Press play to inspect it.".into());
                                    }
                                    Err(err) => status.set(err),
                                }
                            }
                        }
                        div { class: "control-card",
                            h3 { "Device" }
                            p { class: "hint", "Use the address printed by the firmware after it joins Wi-Fi." }
                            div { class: "control-group",
                                label { r#for: "esp-ip", "ESP32 IP address" }
                                input { id: "esp-ip", r#type: "text", value: "{esp_ip}", oninput: move |e| esp_ip.set(e.value()) }
                            }
                            div { class: "test-grid",
                                TestButton { label: "White", frame: white_frame(), ip: esp_ip(), status }
                                TestButton { label: "Black", frame: black_frame(), ip: esp_ip(), status }
                                TestButton { label: "Checker", frame: checkerboard_frame(), ip: esp_ip(), status }
                            }
                        }
                        div { class: "control-card",
                            h3 { "Conversion" }
                            p { class: "hint", "Changes are applied to the next rendered preview frame." }
                            div { class: "field-grid",
                                div { class: "control-group",
                                    label { r#for: "dither-input", "Dither mode" }
                                    select { id: "dither-input",
                                        option { value: "bayer", "Bayer 8×8" }
                                        option { value: "threshold", "Solid threshold" }
                                    }
                                }
                                div { class: "control-group",
                                    label { r#for: "scale-input", "Scale mode" }
                                    select { id: "scale-input",
                                        option { value: "cover", "Fill / cover" }
                                        option { value: "fit", "Best fit" }
                                        option { value: "stretch", "Stretch" }
                                    }
                                }
                            }
                            RangeControl { id: "fps-input", label: "Target FPS", min: "1", max: "20", value: "10", suffix: " fps" }
                            RangeControl { id: "threshold-input", label: "B/W bias", min: "0", max: "255", value: "128", suffix: "" }
                            RangeControl { id: "contrast-input", label: "Contrast", min: "50", max: "300", value: "135", suffix: "%" }
                            RangeControl { id: "gamma-input", label: "Gamma", min: "30", max: "300", value: "85", suffix: "%" }
                            label { class: "checkbox-row",
                                span { "Invert colors" }
                                input { id: "invert-input", r#type: "checkbox" }
                            }
                        }
                        button {
                            class: "action-btn primary", disabled: streaming(),
                            onclick: move |_| {
                                let ip = esp_ip();
                                let mut status = status;
                                streaming.set(true);
                                spawn(async move {
                                    status.set("Starting stream…".into());
                                    let result = stream_selected_video(ip, status).await;
                                    streaming.set(false);
                                    match result {
                                        Ok(()) => status.set("Video finished".into()),
                                        Err(err) => status.set(format!("Video error: {err}")),
                                    }
                                });
                            },
                            if streaming() { "Streaming…" } else { "Stream video to ESP32" }
                        }
                    }
                }
                article { class: "panel preview-panel",
                    PanelHeader { step: "2", title: "Live preview", description: "Source footage and the exact monochrome OLED output.", badge: "128 × 64" }
                    div { class: "preview-shell",
                        div { class: "media-label", "Source video" }
                        video {
                            id: "source-video", controls: true, muted: true, playsinline: true,
                            onplay: move |_| {
                                let mut status = status;
                                spawn(async move {
                                    if let Err(err) = preview_while_playing().await {
                                        status.set(format!("Preview error: {err}"));
                                    }
                                });
                            },
                            onseeked: move |_| {
                                spawn(async move { let _ = render_current_frame(); });
                            }
                        }
                        div { class: "media-label", "OLED output" }
                        div { class: "oled-bezel", canvas { id: "oled-preview", width: WIDTH, height: HEIGHT } }
                        div { class: "status-card",
                            div { class: "status-dot" }
                            div { span { "Status" } strong { "{status}" } }
                        }
                        div { class: "buffer-note",
                            code { "byte_index = (y / 8) * WIDTH + x" }
                            span { "Vertical page layout · bit = y % 8" }
                        }
                    }
                }
            }
            footer { "Frames are sent as raw " code { "[u8; 1024]" } " buffers to " code { "POST /frame" } "." }
        }
    }
}

#[component]
fn PanelHeader(
    step: &'static str,
    title: &'static str,
    description: &'static str,
    badge: &'static str,
) -> Element {
    rsx! {
        div { class: "panel-header",
            div { class: "panel-title",
                span { class: "step-label", "{step}" }
                div { h2 { "{title}" } p { "{description}" } }
            }
            span { class: "panel-badge", "{badge}" }
        }
    }
}

#[component]
fn RangeControl(
    id: &'static str,
    label: &'static str,
    min: &'static str,
    max: &'static str,
    value: &'static str,
    suffix: &'static str,
) -> Element {
    let mut current = use_signal(|| value.to_string());
    rsx! {
        div { class: "control-group",
            label { r#for: id, span { "{label}" } output { "{current}{suffix}" } }
            input {
                id, r#type: "range", min, max, value: "{current}",
                oninput: move |event| {
                    current.set(event.value());
                    spawn(async move { let _ = render_current_frame(); });
                }
            }
        }
    }
}

#[component]
fn TestButton(
    label: &'static str,
    frame: [u8; FRAME_SIZE],
    ip: String,
    mut status: Signal<String>,
) -> Element {
    rsx! {
        button {
            class: "action-btn secondary",
            onclick: move |_| {
                let ip = ip.clone();
                spawn(async move {
                    status.set(format!("Sending {label} frame…"));
                    match post_frame(&ip, &frame).await {
                        Ok(()) => status.set(format!("{label} frame sent")),
                        Err(err) => status.set(format!("Error: {err}")),
                    }
                });
            },
            "{label}"
        }
    }
}

async fn post_frame(ip: &str, frame: &[u8; FRAME_SIZE]) -> Result<(), String> {
    let url = format!("http://{ip}/frame");
    let body = Uint8Array::from(frame.as_slice());
    let response = Request::post(&url)
        .header("Content-Type", "application/octet-stream")
        .body(body)
        .map_err(|err| format!("{err:?}"))?
        .send()
        .await
        .map_err(|err| format!("{err:?}"))?;
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }
    Ok(())
}

fn white_frame() -> [u8; FRAME_SIZE] {
    [0xFF; FRAME_SIZE]
}

fn black_frame() -> [u8; FRAME_SIZE] {
    [0x00; FRAME_SIZE]
}

fn checkerboard_frame() -> [u8; FRAME_SIZE] {
    let mut frame = [0u8; FRAME_SIZE];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let block_x = x / 8;
            let block_y = y / 8;
            if (block_x + block_y) % 2 == 0 {
                set_pixel(&mut frame, x, y, true);
            }
        }
    }
    frame
}

fn set_pixel(frame: &mut [u8; FRAME_SIZE], x: usize, y: usize, on: bool) {
    if x >= WIDTH || y >= HEIGHT {
        return;
    }
    let page = y / 8;
    let bit = y % 8;
    let byte_index = page * WIDTH + x;
    if on {
        frame[byte_index] |= 1u8 << bit;
    } else {
        frame[byte_index] &= !(1u8 << bit);
    }
}

async fn stream_selected_video(ip: String, mut status: Signal<String>) -> Result<(), String> {
    let document = document()?;
    let video = element::<HtmlVideoElement>(&document, "source-video")?;
    let canvas = element::<HtmlCanvasElement>(&document, "oled-preview")?;
    if video.src().is_empty() {
        return Err("Select a video first".into());
    }
    let fps = read_number_input("fps-input", 10).clamp(1, 20);
    video.set_current_time(0.0);
    JsFuture::from(
        video
            .play()
            .map_err(|err| format!("Video play failed: {err:?}"))?,
    )
    .await
    .map_err(|err| format!("Video play promise rejected: {err:?}"))?;
    let delay_ms = 1000 / fps;
    while !video.ended() {
        let frame = render_video_frame(&document, &video, &canvas)?;
        post_frame(&ip, &frame).await?;
        status.set(format!("Streaming... {:.1}s", video.current_time()));
        TimeoutFuture::new(delay_ms).await;
    }
    Ok(())
}

fn load_selected_video() -> Result<String, String> {
    let document = document()?;
    let input = element::<HtmlInputElement>(&document, "video-file")?;
    let file = input
        .files()
        .and_then(|files| files.get(0))
        .ok_or("Select a video first")?;
    let video = element::<HtmlVideoElement>(&document, "source-video")?;
    if video.src().starts_with("blob:") {
        let _ = Url::revoke_object_url(&video.src());
    }
    let url = Url::create_object_url_with_blob(file.as_ref())
        .map_err(|err| format!("Could not open video: {err:?}"))?;
    video.set_src(&url);
    video.load();
    Ok(file.name())
}

async fn preview_while_playing() -> Result<(), String> {
    let video = element::<HtmlVideoElement>(&document()?, "source-video")?;
    while !video.paused() && !video.ended() {
        render_current_frame()?;
        TimeoutFuture::new(33).await;
    }
    Ok(())
}

fn render_current_frame() -> Result<[u8; FRAME_SIZE], String> {
    let document = document()?;
    let video = element::<HtmlVideoElement>(&document, "source-video")?;
    let canvas = element::<HtmlCanvasElement>(&document, "oled-preview")?;
    render_video_frame(&document, &video, &canvas)
}

fn render_video_frame(
    document: &web_sys::Document,
    video: &HtmlVideoElement,
    canvas: &HtmlCanvasElement,
) -> Result<[u8; FRAME_SIZE], String> {
    if video.video_width() == 0 || video.video_height() == 0 {
        return Ok([0; FRAME_SIZE]);
    }
    let context = canvas
        .get_context("2d")
        .map_err(|err| format!("Canvas error: {err:?}"))?
        .ok_or("2D canvas is unavailable")?
        .dyn_into::<CanvasRenderingContext2d>()
        .map_err(|_| "Invalid canvas context")?;
    context.set_image_smoothing_enabled(true);
    draw_video_to_canvas(
        &context,
        video,
        &read_select_input(document, "scale-input", "cover"),
    )?;
    let image_data = context
        .get_image_data(0.0, 0.0, WIDTH as f64, HEIGHT as f64)
        .map_err(|err| format!("Could not read preview pixels: {err:?}"))?;
    let mut rgba = image_data.data().0;
    let frame = rgba_to_oled_frame(
        &mut rgba,
        read_number_input("threshold-input", 128).clamp(0, 255) as u8,
        read_number_input("contrast-input", 135).clamp(50, 300) as f32 / 100.0,
        read_number_input("gamma-input", 85).clamp(30, 300) as f32 / 100.0,
        read_checkbox_input("invert-input", false),
        &read_select_input(document, "dither-input", "bayer"),
    );
    let preview = ImageData::new_with_u8_clamped_array_and_sh(
        Clamped(rgba.as_slice()),
        WIDTH as u32,
        HEIGHT as u32,
    )
    .map_err(|err| format!("Could not build preview: {err:?}"))?;
    context
        .put_image_data(&preview, 0.0, 0.0)
        .map_err(|err| format!("Could not draw preview: {err:?}"))?;
    Ok(frame)
}

fn document() -> Result<web_sys::Document, String> {
    web_sys::window()
        .and_then(|window| window.document())
        .ok_or("Browser document is unavailable".into())
}

fn element<T: JsCast>(document: &web_sys::Document, id: &str) -> Result<T, String> {
    document
        .get_element_by_id(id)
        .ok_or_else(|| format!("Missing #{id}"))?
        .dyn_into::<T>()
        .map_err(|_| format!("#{id} has an unexpected element type"))
}

fn read_select_input(document: &web_sys::Document, id: &str, fallback: &str) -> String {
    element::<HtmlSelectElement>(document, id)
        .map(|select| select.value())
        .unwrap_or_else(|_| fallback.into())
}

fn read_number_input(id: &str, fallback: u32) -> u32 {
    let Some(window) = web_sys::window() else {
        return fallback;
    };
    let Some(document) = window.document() else {
        return fallback;
    };
    let Some(element) = document.get_element_by_id(id) else {
        return fallback;
    };
    let Ok(input) = element.dyn_into::<HtmlInputElement>() else {
        return fallback;
    };
    input.value().parse::<u32>().unwrap_or(fallback)
}

const BAYER_8X8: [[u8; 8]; 8] = [
    [0, 48, 12, 60, 3, 51, 15, 63],
    [32, 16, 44, 28, 35, 19, 47, 31],
    [8, 56, 4, 52, 11, 59, 7, 55],
    [40, 24, 36, 20, 43, 27, 39, 23],
    [2, 50, 14, 62, 1, 49, 13, 61],
    [34, 18, 46, 30, 33, 17, 45, 29],
    [10, 58, 6, 54, 9, 57, 5, 53],
    [42, 26, 38, 22, 41, 25, 37, 21],
];

fn rgba_to_oled_frame(
    rgba: &mut [u8],
    threshold: u8,
    contrast: f32,
    gamma: f32,
    invert: bool,
    dither_mode: &str,
) -> [u8; FRAME_SIZE] {
    let mut frame = [0u8; FRAME_SIZE];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let pixel_index = (y * WIDTH + x) * 4;
            let r = rgba[pixel_index] as f32;
            let g = rgba[pixel_index + 1] as f32;
            let b = rgba[pixel_index + 2] as f32;
            let mut luma = 0.299 * r + 0.587 * g + 0.114 * b;
            luma = ((luma - 128.0) * contrast + 128.0).clamp(0.0, 255.0);
            luma = 255.0 * (luma / 255.0).powf(gamma);
            let cutoff = if dither_mode == "bayer" {
                threshold as f32 + (BAYER_8X8[y % 8][x % 8] as f32 - 31.5) * 3.0
            } else {
                threshold as f32
            };
            let mut on = luma > cutoff;
            if invert {
                on = !on;
            }
            let color = if on { 255 } else { 0 };
            rgba[pixel_index] = color;
            rgba[pixel_index + 1] = color;
            rgba[pixel_index + 2] = color;
            rgba[pixel_index + 3] = 255;
            set_pixel(&mut frame, x, y, on);
        }
    }
    frame
}

fn read_checkbox_input(id: &str, fallback: bool) -> bool {
    let Some(window) = web_sys::window() else {
        return fallback;
    };
    let Some(document) = window.document() else {
        return fallback;
    };
    let Some(element) = document.get_element_by_id(id) else {
        return fallback;
    };
    let Ok(input) = element.dyn_into::<HtmlInputElement>() else {
        return fallback;
    };
    input.checked()
}

fn draw_video_to_canvas(
    context: &CanvasRenderingContext2d,
    video: &HtmlVideoElement,
    mode: &str,
) -> Result<(), String> {
    let vw = video.video_width() as f64;
    let vh = video.video_height() as f64;
    let tw = WIDTH as f64;
    let th = HEIGHT as f64;
    context.set_fill_style_str("#000");
    context.fill_rect(0.0, 0.0, tw, th);
    let (dx, dy, dw, dh) = match mode {
        "stretch" => (0.0, 0.0, tw, th),
        "fit" => {
            let scale = (tw / vw).min(th / vh);
            let (dw, dh) = (vw * scale, vh * scale);
            ((tw - dw) / 2.0, (th - dh) / 2.0, dw, dh)
        }
        _ => {
            let scale = (tw / vw).max(th / vh);
            let (dw, dh) = (vw * scale, vh * scale);
            ((tw - dw) / 2.0, (th - dh) / 2.0, dw, dh)
        }
    };
    context
        .draw_image_with_html_video_element_and_dw_and_dh(video, dx, dy, dw, dh)
        .map_err(|err| format!("Could not draw video: {err:?}"))
}
