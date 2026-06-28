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
const LINE_ART_SAMPLE_SCALE: usize = 4;

#[derive(Clone, Copy)]
struct FrameTransform {
    zoom: f64,
    pan_x: f64,
    pan_y: f64,
}

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
                                        option { value: "threshold", "Solid threshold" }
                                        option { value: "line-art", "Line art" }
                                        option { value: "bayer", "Bayer 8×8" }
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
                            RangeControl { id: "zoom-input", label: "Content zoom", min: "100", max: "400", value: "200", suffix: "%" }
                            RangeControl { id: "pan-x-input", label: "Horizontal pan", min: "-64", max: "64", value: "0", suffix: " px" }
                            RangeControl { id: "pan-y-input", label: "Vertical pan", min: "-32", max: "32", value: "0", suffix: " px" }
                            RangeControl { id: "threshold-input", label: "B/W bias", min: "0", max: "255", value: "128", suffix: "" }
                            RangeControl { id: "contrast-input", label: "Contrast", min: "50", max: "300", value: "135", suffix: "%" }
                            RangeControl { id: "gamma-input", label: "Gamma", min: "30", max: "300", value: "85", suffix: "%" }
                            label { class: "checkbox-row",
                                span { "Invert colors" }
                                input { id: "invert-input", r#type: "checkbox" }
                            }
                            label { class: "checkbox-row",
                                span { "Beam splitter compensation (flip Y)" }
                                input { id: "beam-splitter-input", r#type: "checkbox" }
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
    let dither = read_select_input(document, "dither-input", "line-art");
    let scale = read_select_input(document, "scale-input", "cover");
    let threshold = read_number_input("threshold-input", 128).clamp(0, 255) as u8;
    let contrast = read_number_input("contrast-input", 135).clamp(50, 300) as f32 / 100.0;
    let gamma = read_number_input("gamma-input", 85).clamp(30, 300) as f32 / 100.0;
    let invert = read_checkbox_input("invert-input", false);
    let beam_splitter = read_checkbox_input("beam-splitter-input", false);
    let zoom = read_number_input("zoom-input", 200).clamp(100, 400) as f64 / 100.0;
    let pan_x = read_signed_number_input("pan-x-input", 0).clamp(-64, 64) as f64;
    let pan_y = read_signed_number_input("pan-y-input", 0).clamp(-32, 32) as f64;
    let transform = FrameTransform { zoom, pan_x, pan_y };

    if dither == "line-art" || dither == "threshold" {
        let sample_canvas = document
            .create_element("canvas")
            .map_err(|err| format!("Could not create sampling canvas: {err:?}"))?
            .dyn_into::<HtmlCanvasElement>()
            .map_err(|_| "Invalid sampling canvas")?;
        sample_canvas.set_width((WIDTH * LINE_ART_SAMPLE_SCALE) as u32);
        sample_canvas.set_height((HEIGHT * LINE_ART_SAMPLE_SCALE) as u32);
        let sample_context = sample_canvas
            .get_context("2d")
            .map_err(|err| format!("Sampling canvas error: {err:?}"))?
            .ok_or("Sampling canvas is unavailable")?
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| "Invalid sampling canvas context")?;
        sample_context.set_image_smoothing_enabled(true);
        draw_video_to_canvas(
            &sample_context,
            video,
            &scale,
            (WIDTH * LINE_ART_SAMPLE_SCALE) as f64,
            (HEIGHT * LINE_ART_SAMPLE_SCALE) as f64,
            transform,
        )?;
        let samples = sample_context
            .get_image_data(
                0.0,
                0.0,
                (WIDTH * LINE_ART_SAMPLE_SCALE) as f64,
                (HEIGHT * LINE_ART_SAMPLE_SCALE) as f64,
            )
            .map_err(|err| format!("Could not read line-art samples: {err:?}"))?
            .data()
            .0;
        let (mut frame, mut preview_rgba) = if dither == "line-art" {
            supersampled_line_art(&samples, threshold, contrast, gamma, invert)
        } else {
            supersampled_solid_threshold(&samples, threshold, contrast, gamma, invert)
        };
        if beam_splitter {
            flip_output_y(&mut frame, &mut preview_rgba);
        }
        put_preview(&context, &preview_rgba)?;
        return Ok(frame);
    }

    context.set_image_smoothing_enabled(true);
    draw_video_to_canvas(
        &context,
        video,
        &scale,
        WIDTH as f64,
        HEIGHT as f64,
        transform,
    )?;
    let image_data = context
        .get_image_data(0.0, 0.0, WIDTH as f64, HEIGHT as f64)
        .map_err(|err| format!("Could not read preview pixels: {err:?}"))?;
    let mut rgba = image_data.data().0;
    let mut frame = rgba_to_oled_frame(&mut rgba, threshold, contrast, gamma, invert, &dither);
    if beam_splitter {
        flip_output_y(&mut frame, &mut rgba);
    }
    put_preview(&context, &rgba)?;
    Ok(frame)
}

fn put_preview(context: &CanvasRenderingContext2d, rgba: &[u8]) -> Result<(), String> {
    let preview =
        ImageData::new_with_u8_clamped_array_and_sh(Clamped(rgba), WIDTH as u32, HEIGHT as u32)
            .map_err(|err| format!("Could not build preview: {err:?}"))?;
    context
        .put_image_data(&preview, 0.0, 0.0)
        .map_err(|err| format!("Could not draw preview: {err:?}"))
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

fn read_signed_number_input(id: &str, fallback: i32) -> i32 {
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
    input.value().parse::<i32>().unwrap_or(fallback)
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
    let mut pixels = vec![false; WIDTH * HEIGHT];

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let pixel_index = (y * WIDTH + x) * 4;
            let r = rgba[pixel_index] as f32;
            let g = rgba[pixel_index + 1] as f32;
            let b = rgba[pixel_index + 2] as f32;
            let mut luma = 0.299 * r + 0.587 * g + 0.114 * b;
            luma = ((luma - 128.0) * contrast + 128.0).clamp(0.0, 255.0);
            luma = 255.0 * (luma / 255.0).powf(gamma);
            let cutoff = match dither_mode {
                "bayer" => threshold as f32 + (BAYER_8X8[y % 8][x % 8] as f32 - 31.5) * 3.0,
                // A thin black source line may cover only a small part of an
                // output pixel and therefore downscale to a very light gray.
                "line-art" => (u16::from(threshold) + 112).min(250) as f32,
                _ => threshold as f32,
            };
            let mut on = luma > cutoff;
            if invert {
                on = !on;
            }
            pixels[y * WIDTH + x] = on;
        }
    }

    if dither_mode == "line-art" {
        pixels = repair_black_strokes(&pixels);
    }

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let pixel_index = (y * WIDTH + x) * 4;
            let on = pixels[y * WIDTH + x];
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

fn supersampled_line_art(
    samples: &[u8],
    threshold: u8,
    contrast: f32,
    gamma: f32,
    invert: bool,
) -> ([u8; FRAME_SIZE], Vec<u8>) {
    let sample_width = WIDTH * LINE_ART_SAMPLE_SCALE;
    let mut pixels = vec![false; WIDTH * HEIGHT];

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let mut dark_samples = 0;
            for sample_y in 0..LINE_ART_SAMPLE_SCALE {
                for sample_x in 0..LINE_ART_SAMPLE_SCALE {
                    let sx = x * LINE_ART_SAMPLE_SCALE + sample_x;
                    let sy = y * LINE_ART_SAMPLE_SCALE + sample_y;
                    let index = (sy * sample_width + sx) * 4;
                    let mut luma = 0.299 * samples[index] as f32
                        + 0.587 * samples[index + 1] as f32
                        + 0.114 * samples[index + 2] as f32;
                    luma = ((luma - 128.0) * contrast + 128.0).clamp(0.0, 255.0);
                    luma = 255.0 * (luma / 255.0).powf(gamma);
                    if luma <= threshold as f32 {
                        dark_samples += 1;
                    }
                }
            }
            // Two dark samples are enough to retain a thin stroke, while one
            // isolated sample is treated as scaling noise.
            let mut on = dark_samples < 2;
            if invert {
                on = !on;
            }
            pixels[y * WIDTH + x] = on;
        }
    }

    pack_monochrome_pixels(&repair_black_strokes(&pixels))
}

fn supersampled_solid_threshold(
    samples: &[u8],
    threshold: u8,
    contrast: f32,
    gamma: f32,
    invert: bool,
) -> ([u8; FRAME_SIZE], Vec<u8>) {
    let sample_width = WIDTH * LINE_ART_SAMPLE_SCALE;
    let samples_per_pixel = (LINE_ART_SAMPLE_SCALE * LINE_ART_SAMPLE_SCALE) as f32;
    let cutoff = (u16::from(threshold) + 64).min(255) as f32;
    let mut pixels = vec![false; WIDTH * HEIGHT];

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let mut luma_sum = 0.0;
            for sample_y in 0..LINE_ART_SAMPLE_SCALE {
                for sample_x in 0..LINE_ART_SAMPLE_SCALE {
                    let sx = x * LINE_ART_SAMPLE_SCALE + sample_x;
                    let sy = y * LINE_ART_SAMPLE_SCALE + sample_y;
                    let index = (sy * sample_width + sx) * 4;
                    let mut luma = 0.299 * samples[index] as f32
                        + 0.587 * samples[index + 1] as f32
                        + 0.114 * samples[index + 2] as f32;
                    luma = ((luma - 128.0) * contrast + 128.0).clamp(0.0, 255.0);
                    luma = 255.0 * (luma / 255.0).powf(gamma);
                    luma_sum += luma;
                }
            }
            let mut on = luma_sum / samples_per_pixel > cutoff;
            if invert {
                on = !on;
            }
            pixels[y * WIDTH + x] = on;
        }
    }

    pack_monochrome_pixels(&pixels)
}

fn pack_monochrome_pixels(pixels: &[bool]) -> ([u8; FRAME_SIZE], Vec<u8>) {
    let mut frame = [0; FRAME_SIZE];
    let mut preview = vec![255; WIDTH * HEIGHT * 4];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let on = pixels[y * WIDTH + x];
            let color = if on { 255 } else { 0 };
            let rgba_index = (y * WIDTH + x) * 4;
            preview[rgba_index] = color;
            preview[rgba_index + 1] = color;
            preview[rgba_index + 2] = color;
            preview[rgba_index + 3] = 255;
            set_pixel(&mut frame, x, y, on);
        }
    }
    (frame, preview)
}

fn flip_output_y(frame: &mut [u8; FRAME_SIZE], preview: &mut [u8]) {
    let source_frame = *frame;
    let source_preview = preview.to_vec();
    frame.fill(0);

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let source_page = y / 8;
            let source_bit = y % 8;
            let source_index = source_page * WIDTH + x;
            let on = source_frame[source_index] & (1 << source_bit) != 0;
            let target_x = x;
            let target_y = HEIGHT - 1 - y;
            set_pixel(frame, target_x, target_y, on);

            let source_rgba = (y * WIDTH + x) * 4;
            let target_rgba = (target_y * WIDTH + target_x) * 4;
            preview[target_rgba..target_rgba + 4]
                .copy_from_slice(&source_preview[source_rgba..source_rgba + 4]);
        }
    }
}

fn repair_black_strokes(pixels: &[bool]) -> Vec<bool> {
    let mut repaired = pixels.to_vec();

    // Only repair an unambiguous one-pixel break. Expanding pixels based on
    // neighbor counts fills intentional highlights in detailed drawings.
    for y in 1..HEIGHT - 1 {
        for x in 1..WIDTH - 1 {
            let index = y * WIDTH + x;
            if pixels[index]
                && ((!pixels[index - 1] && !pixels[index + 1])
                    || (!pixels[index - WIDTH] && !pixels[index + WIDTH]))
            {
                repaired[index] = false;
            }
        }
    }

    repaired
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
    target_width: f64,
    target_height: f64,
    transform: FrameTransform,
) -> Result<(), String> {
    let vw = video.video_width() as f64;
    let vh = video.video_height() as f64;
    let tw = target_width;
    let th = target_height;
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
    let center_x = dx + dw / 2.0;
    let center_y = dy + dh / 2.0;
    let dw = dw * transform.zoom;
    let dh = dh * transform.zoom;
    let dx = center_x - dw / 2.0 + transform.pan_x * (tw / WIDTH as f64);
    let dy = center_y - dh / 2.0 + transform.pan_y * (th / HEIGHT as f64);
    context
        .draw_image_with_html_video_element_and_dw_and_dh(video, dx, dy, dw, dh)
        .map_err(|err| format!("Could not draw video: {err:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_pixels_in_vertical_pages() {
        let mut frame = [0; FRAME_SIZE];
        set_pixel(&mut frame, 7, 0, true);
        set_pixel(&mut frame, 7, 7, true);
        set_pixel(&mut frame, 7, 8, true);

        assert_eq!(frame[7], 0b1000_0001);
        assert_eq!(frame[WIDTH + 7], 0b0000_0001);
    }

    #[test]
    fn line_art_repairs_a_white_hole_inside_a_black_stroke() {
        let mut pixels = vec![true; WIDTH * HEIGHT];
        let x = 40;
        let y = 30;
        let index = y * WIDTH + x;
        pixels[index - 1] = false;
        pixels[index + 1] = false;

        let repaired = repair_black_strokes(&pixels);

        assert!(!repaired[index]);
        assert!(repaired[index - WIDTH]);
    }

    #[test]
    fn line_art_preserves_open_white_areas() {
        let pixels = vec![true; WIDTH * HEIGHT];
        let repaired = repair_black_strokes(&pixels);

        assert!(repaired.iter().all(|pixel| *pixel));
    }

    #[test]
    fn line_art_does_not_expand_a_wide_break() {
        let mut pixels = vec![true; WIDTH * HEIGHT];
        let y = 30;
        let start = y * WIDTH + 40;
        pixels[start] = false;
        pixels[start + 4] = false;

        let repaired = repair_black_strokes(&pixels);

        assert!(repaired[start + 1..start + 4].iter().all(|pixel| *pixel));
    }

    #[test]
    fn solid_threshold_retains_a_one_sample_thick_line() {
        let sample_width = WIDTH * LINE_ART_SAMPLE_SCALE;
        let sample_height = HEIGHT * LINE_ART_SAMPLE_SCALE;
        let mut samples = vec![255; sample_width * sample_height * 4];
        let output_y = 20;
        let sample_y = output_y * LINE_ART_SAMPLE_SCALE;
        for x in 0..sample_width {
            let index = (sample_y * sample_width + x) * 4;
            samples[index] = 0;
            samples[index + 1] = 0;
            samples[index + 2] = 0;
        }

        let (frame, _) = supersampled_solid_threshold(&samples, 128, 1.0, 1.0, false);
        let page = output_y / 8;
        let bit = output_y % 8;

        assert_eq!(frame[page * WIDTH + 30] & (1 << bit), 0);
        assert_ne!(frame[(page - 1) * WIDTH + 30] & (1 << 7), 0);
    }

    #[test]
    fn beam_splitter_compensation_flips_only_y() {
        let mut frame = [0; FRAME_SIZE];
        set_pixel(&mut frame, 0, 0, true);
        let mut preview = vec![0; WIDTH * HEIGHT * 4];
        preview[..4].fill(255);

        flip_output_y(&mut frame, &mut preview);

        assert_eq!(frame[0] & 1, 0);
        let target_index = (HEIGHT / 8 - 1) * WIDTH;
        assert_ne!(frame[target_index] & (1 << 7), 0);
        let target_rgba = (HEIGHT - 1) * WIDTH * 4;
        assert_eq!(&preview[target_rgba..target_rgba + 4], &[255; 4]);
    }
}
