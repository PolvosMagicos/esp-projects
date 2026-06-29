mod frame;
mod oled;
mod wifi;

use anyhow::Result;
use std::sync::{Arc, Mutex};

use embedded_graphics::{pixelcolor::BinaryColor, prelude::*};

use embedded_svc::{
    http::{Headers, Method},
    io::Write,
    ws::FrameType,
};

use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        delay::FreeRtos,
        i2c::{I2cConfig, I2cDriver},
        peripherals::Peripherals,
        units::*,
    },
    http::server::{Configuration as HttpServerConfig, EspHttpServer},
    nvs::EspDefaultNvsPartition,
    wifi::{BlockingWifi, EspWifi},
};

use log::info;

use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};

use crate::{
    frame::{checkerboard_frame, FRAME_SIZE},
    oled::draw_raw_frame,
    wifi::connect_wifi,
};

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;

    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    connect_wifi(&mut wifi)?;

    let sda = peripherals.pins.gpio8;
    let scl = peripherals.pins.gpio9;

    let config = I2cConfig::new().baudrate(400_u32.kHz().into());
    let i2c = I2cDriver::new(peripherals.i2c0, sda, scl, &config)?;

    let interface = I2CDisplayInterface::new(i2c);

    let mut display: Ssd1306<_, DisplaySize128x64, BufferedGraphicsMode<DisplaySize128x64>> =
        Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();

    display.init().unwrap();
    display.clear(BinaryColor::Off).unwrap();

    let startup_frame = checkerboard_frame();
    draw_raw_frame(&mut display, &startup_frame)?;
    display.flush().unwrap();

    let display = Arc::new(Mutex::new(display));

    let mut server = EspHttpServer::new(&HttpServerConfig {
        stack_size: 8192,
        ..Default::default()
    })?;

    server.fn_handler("/health", Method::Get, |req| {
        req.into_ok_response()?.write_all(b"ok").map(|_| ())
    })?;

    let display_for_frame = display.clone();

    server.fn_handler::<anyhow::Error, _>("/frame", Method::Options, |req| {
        req.into_response(
            204,
            Some("No Content"),
            &[
                ("Access-Control-Allow-Origin", "*"),
                ("Access-Control-Allow-Methods", "POST, OPTIONS"),
                ("Access-Control-Allow-Headers", "Content-Type"),
                ("Access-Control-Max-Age", "86400"),
            ],
        )?
        .write_all(b"")?;

        Ok(())
    })?;

    server.fn_handler::<anyhow::Error, _>("/frame", Method::Post, move |mut req| {
        let len = req.content_len().unwrap_or(0) as usize;

        if len != FRAME_SIZE {
            let message = format!("Expected {} bytes, got {}", FRAME_SIZE, len);

            req.into_response(
                400,
                Some("Bad Request"),
                &[
                    ("Access-Control-Allow-Origin", "*"),
                    ("Content-Type", "text/plain"),
                ],
            )?
            .write_all(message.as_bytes())?;

            return Ok(());
        }

        let mut frame = [0u8; FRAME_SIZE];
        let mut offset = 0;

        while offset < FRAME_SIZE {
            let read = req.read(&mut frame[offset..])?;

            if read == 0 {
                anyhow::bail!("Connection closed before full frame was received");
            }

            offset += read;
        }

        {
            let mut display = display_for_frame.lock().unwrap();
            draw_raw_frame(&mut *display, &frame)?;
            display.flush().unwrap();
        }

        req.into_response(
            200,
            Some("OK"),
            &[
                ("Access-Control-Allow-Origin", "*"),
                ("Content-Type", "text/plain"),
            ],
        )?
        .write_all(b"frame ok")?;

        Ok(())
    })?;

    let display_for_stream = display.clone();

    server.ws_handler("/frames", None, move |ws| {
        if ws.is_new() {
            info!("WebSocket frame stream connected");
            return Ok::<(), anyhow::Error>(());
        }
        if ws.is_closed() {
            info!("WebSocket frame stream disconnected");
            return Ok(());
        }

        let (frame_type, len) = ws.recv(&mut [])?;
        if frame_type != FrameType::Binary(false) || len != FRAME_SIZE {
            ws.send(FrameType::Close, &[])?;
            anyhow::bail!("Expected one unfragmented {}-byte binary frame", FRAME_SIZE);
        }

        let mut frame = [0u8; FRAME_SIZE];
        ws.recv(&mut frame)?;

        let mut display = display_for_stream.lock().unwrap();
        draw_raw_frame(&mut *display, &frame)?;
        display
            .flush()
            .map_err(|err| anyhow::anyhow!("OLED flush failed: {err:?}"))?;

        Ok(())
    })?;

    info!("HTTP and WebSocket server started");

    loop {
        FreeRtos::delay_ms(1000);
    }
}
