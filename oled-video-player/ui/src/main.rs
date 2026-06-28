use dioxus::prelude::*;
use gloo_net::http::Request;
use gloo_timers::future::TimeoutFuture;
use js_sys::Uint8Array;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use web_sys::{
    CanvasRenderingContext2d, HtmlCanvasElement, HtmlInputElement, HtmlVideoElement, Url,
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
    let status = use_signal(|| "Ready".to_string());

    rsx! {
        div {
            style: "font-family: system-ui, sans-serif; padding: 24px; max-width: 760px; margin: auto;",

            h1 { "OLED Video Player" }
            label { "ESP32 IP:" }
            br {}
            input {
                style: "padding: 8px; width: 240px; margin-top: 8px;",
                value: "{esp_ip}",
                oninput: move |event| esp_ip.set(event.value())
            }

            div {
                style: "margin-top: 16px; display: flex; gap: 8px; flex-wrap: wrap;",
                button {
                    onclick: move |_| {
                        let ip = esp_ip();
                        let mut status = status;
                        spawn(async move {
                            status.set("Sending white frame...".to_string());
                            match post_frame(&ip, &white_frame()).await {
                                Ok(_) => status.set("White frame sent".to_string()),
                                Err(err) => status.set(format!("Error: {err}")),
                            }
                        });
                    },
                    "White"
                }
                button {
                    onclick: move |_| {
                        let ip = esp_ip();
                        let mut status = status;
                        spawn(async move {
                            status.set("Sending black frame...".to_string());
                            match post_frame(&ip, &black_frame()).await {
                                Ok(_) => status.set("Black frame sent".to_string()),
                                Err(err) => status.set(format!("Error: {err}")),
                            }
                        });
                    },
                    "Black"
                }
                button {
                    onclick: move |_| {
                        let ip = esp_ip();
                        let mut status = status;
                        spawn(async move {
                            status.set("Sending checkerboard...".to_string());
                            match post_frame(&ip, &checkerboard_frame()).await {
                                Ok(_) => status.set("Checkerboard sent".to_string()),
                                Err(err) => status.set(format!("Error: {err}")),
                            }
                        });
                    },
                    "Checkerboard"
                }
                button {
                    onclick: move |_| {
                        let ip = esp_ip();
                        let mut status = status;
                        spawn(async move {
                            status.set("Playing moving bar...".to_string());
                            for step in 0..64 {
                                let frame = moving_bar_frame(step);
                                if let Err(err) = post_frame(&ip, &frame).await {
                                    status.set(format!("Error: {err}"));
                                    return;
                                }
                                TimeoutFuture::new(80).await;
                            }
                            status.set("Animation done".to_string());
                        });
                    },
                    "Moving bar"
                }
            }

            div {
                style: "margin-top: 24px;",
                h2 { "Video upload" }
                input {
                    id: "video-file",
                    r#type: "file",
                    accept: "video/*",
                }
                div {
                    style: "margin-top: 12px; display: flex; gap: 8px; align-items: center;",
                    label { "FPS:" }
                    input { style: "width: 80px;", r#type: "number", value: "10", id: "fps-input" }
                    label { "Threshold:" }
                    input { style: "width: 80px;", r#type: "number", value: "128", id: "threshold-input" }
                    label { "Contrast:" }
                    input { style: "width: 80px;", r#type: "number", value: "135", id: "contrast-input" }
                    label { "Gamma:" }
                    input { style: "width: 80px;", r#type: "number", value: "85", id: "gamma-input" }
                    label { "Invert:" }
                    input { r#type: "checkbox", id: "invert-input" }
                    label { "B/W:" }
                    input { r#type: "checkbox", id: "black-white-input" }
                }
                button {
                    style: "margin-top: 12px;",
                    onclick: move |_| {
                        let ip = esp_ip();
                        let mut status = status;
                        spawn(async move {
                            status.set("Starting video stream...".to_string());
                            match stream_selected_video(ip, status).await {
                                Ok(_) => status.set("Video finished".to_string()),
                                Err(err) => status.set(format!("Video error: {err}")),
                            }
                        });
                    },
                    "Play uploaded video"
                }
            }
            p { style: "margin-top: 16px;", "Status: {status}" }
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

fn moving_bar_frame(step: usize) -> [u8; FRAME_SIZE] {
    let mut frame = [0u8; FRAME_SIZE];
    let bar_x = step * 2;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if x.abs_diff(bar_x) < 4 {
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
    let window = web_sys::window().ok_or("No window")?;
    let document = window.document().ok_or("No document")?;
    let input = document
        .get_element_by_id("video-file")
        .ok_or("Missing video-file input")?
        .dyn_into::<HtmlInputElement>()
        .map_err(|_| "video-file is not an input")?;
    let files = input.files().ok_or("No file list")?;
    let file = files.get(0).ok_or("Select a video first")?;

    let fps = read_number_input("fps-input", 10).clamp(1, 20);
    let threshold = read_number_input("threshold-input", 128).clamp(0, 255) as u8;
    let contrast = read_number_input("contrast-input", 135).clamp(50, 300) as f32 / 100.0;
    let gamma = read_number_input("gamma-input", 85).clamp(30, 300) as f32 / 100.0;
    let invert = read_checkbox_input("invert-input", false);
    let black_white = read_checkbox_input("black-white-input", false);

    let object_url = Url::create_object_url_with_blob(file.as_ref())
        .map_err(|err| format!("Failed to create object URL: {err:?}"))?;
    let video = document
        .create_element("video")
        .map_err(|err| format!("Failed to create video element: {err:?}"))?
        .dyn_into::<HtmlVideoElement>()
        .map_err(|_| "Created element is not a video")?;
    video.set_src(&object_url);
    video.set_muted(true);
    video.set_loop(false);
    video
        .set_attribute("playsinline", "true")
        .map_err(|err| format!("Failed to set playsinline: {err:?}"))?;

    let canvas = document
        .create_element("canvas")
        .map_err(|err| format!("Failed to create canvas: {err:?}"))?
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| "Created element is not a canvas")?;
    canvas.set_width(WIDTH as u32);
    canvas.set_height(HEIGHT as u32);
    let context = canvas
        .get_context("2d")
        .map_err(|err| format!("Canvas context error: {err:?}"))?
        .ok_or("Missing 2D canvas context")?
        .dyn_into::<CanvasRenderingContext2d>()
        .map_err(|_| "Context is not CanvasRenderingContext2d")?;
    context.set_image_smoothing_enabled(true);
    video.load();
    let play_promise = video
        .play()
        .map_err(|err| format!("Video play failed: {err:?}"))?;
    JsFuture::from(play_promise)
        .await
        .map_err(|err| format!("Video play promise rejected: {err:?}"))?;
    let delay_ms = 1000 / fps;

    loop {
        if video.ended() {
            break;
        }
        draw_video_cover_to_canvas(&context, &video)?;
        let image_data = context
            .get_image_data(0.0, 0.0, WIDTH as f64, HEIGHT as f64)
            .map_err(|err| format!("Failed to get image data: {err:?}"))?;
        let rgba = image_data.data().0;
        let frame = if black_white {
            rgba_to_ssd1306_bw_frame(&rgba, threshold, invert)
        } else {
            rgba_to_ssd1306_frame(&rgba, threshold, contrast, gamma, invert)
        };
        post_frame(&ip, &frame).await?;
        status.set(format!("Streaming... {:.1}s", video.current_time()));
        TimeoutFuture::new(delay_ms).await;
    }
    Url::revoke_object_url(&object_url)
        .map_err(|err| format!("Failed to revoke object URL: {err:?}"))?;
    Ok(())
}

fn read_number_input(id: &str, fallback: u32) -> u32 {
    let Some(window) = web_sys::window() else { return fallback };
    let Some(document) = window.document() else { return fallback };
    let Some(element) = document.get_element_by_id(id) else { return fallback };
    let Ok(input) = element.dyn_into::<HtmlInputElement>() else { return fallback };
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

fn rgba_to_ssd1306_frame(
    rgba: &[u8],
    threshold: u8,
    contrast: f32,
    gamma: f32,
    invert: bool,
) -> [u8; FRAME_SIZE] {
    let mut frame = [0u8; FRAME_SIZE];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let pixel_index = (y * WIDTH + x) * 4;
            if pixel_index + 2 >= rgba.len() { continue }
            let r = rgba[pixel_index] as f32;
            let g = rgba[pixel_index + 1] as f32;
            let b = rgba[pixel_index + 2] as f32;
            let mut luma = 0.299 * r + 0.587 * g + 0.114 * b;
            luma = ((luma - 128.0) * contrast + 128.0).clamp(0.0, 255.0);
            luma = 255.0 * (luma / 255.0).powf(gamma);
            let dither = BAYER_8X8[y % 8][x % 8] as f32;
            let dither_threshold = threshold as f32 + ((dither - 31.5) * 3.0);
            let mut on = luma > dither_threshold;
            if invert { on = !on }
            if on { set_pixel(&mut frame, x, y, true) }
        }
    }
    frame
}

fn rgba_to_ssd1306_bw_frame(rgba: &[u8], threshold: u8, invert: bool) -> [u8; FRAME_SIZE] {
    let mut frame = [0u8; FRAME_SIZE];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let pixel_index = (y * WIDTH + x) * 4;
            if pixel_index + 2 >= rgba.len() { continue }
            let r = rgba[pixel_index] as u16;
            let g = rgba[pixel_index + 1] as u16;
            let b = rgba[pixel_index + 2] as u16;
            let luma = ((r * 77 + g * 150 + b * 29) >> 8) as u8;
            let mut on = luma > threshold;
            if invert { on = !on }
            if on { set_pixel(&mut frame, x, y, true) }
        }
    }
    frame
}

fn read_checkbox_input(id: &str, fallback: bool) -> bool {
    let Some(window) = web_sys::window() else { return fallback };
    let Some(document) = window.document() else { return fallback };
    let Some(element) = document.get_element_by_id(id) else { return fallback };
    let Ok(input) = element.dyn_into::<HtmlInputElement>() else { return fallback };
    input.checked()
}

fn draw_video_cover_to_canvas(
    context: &CanvasRenderingContext2d,
    video: &HtmlVideoElement,
) -> Result<(), String> {
    let video_width = video.video_width() as f64;
    let video_height = video.video_height() as f64;
    if video_width <= 0.0 || video_height <= 0.0 { return Ok(()) }
    let target_width = WIDTH as f64;
    let target_height = HEIGHT as f64;
    let video_aspect = video_width / video_height;
    let target_aspect = target_width / target_height;
    let (sx, sy, sw, sh) = if video_aspect > target_aspect {
        let source_width = video_height * target_aspect;
        ((video_width - source_width) / 2.0, 0.0, source_width, video_height)
    } else {
        let source_height = video_width / target_aspect;
        (0.0, (video_height - source_height) / 2.0, video_width, source_height)
    };
    context
        .draw_image_with_html_video_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
            video, sx, sy, sw, sh, 0.0, 0.0, target_width, target_height,
        )
        .map_err(|err| format!("Failed to draw cropped video frame: {err:?}"))?;
    Ok(())
}
