use anyhow::{bail, Result};
use bench_display::wifi;
use core::str;
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, ascii::FONT_5X8, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::PrimitiveStyle,
    primitives::{Circle, Line, Rectangle},
    text::{Alignment, Text, TextStyleBuilder},
};
use embedded_svc::{
    http::{client::Client, Method},
    io::Read,
    wifi::Wifi,
};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        delay::Ets,
        gpio::{AnyIOPin, InputMode, PinDriver},
        prelude::Peripherals,
        reset,
        spi::{SpiDeviceDriver, SpiDriver, SpiDriverConfig},
    },
    http::client::{Configuration, EspHttpConnection},
    ipv4::IpInfo,
    sys::EspError,
};
use ssd1680::color::{Black, Red};
use ssd1680::{
    driver::Ssd1680,
    prelude::{Display, Display2in13, DisplayRotation},
};
use std::{
    fmt::Debug,
    num::{NonZeroI32, NonZeroU32},
    time::Duration,
};
use textwrap::Options;

use esp_idf_svc::mqtt::client::*;
use log::{error, info};

#[toml_cfg::toml_config]
pub struct Config {
    #[default("")]
    wifi_ssid: &'static str,
    #[default("")]
    wifi_psk: &'static str,
    #[default("localhost")]
    mqtt_host: &'static str,
    #[default("")]
    mqtt_user: &'static str,
    #[default("")]
    mqtt_pass: &'static str,
}

// type DisplayDriver<
//     'a,
//     RST: esp_idf_svc::hal::gpio::Pin,
//     DC: esp_idf_svc::hal::gpio::Pin,
//     BUSY: esp_idf_svc::hal::gpio::Pin,
//     OUT: esp_idf_svc::hal::gpio::OutputMode,
//     IN: esp_idf_svc::hal::gpio::InputMode,
// > = Ssd1680<
//     &'a mut SpiDeviceDriver<'a, SpiDriver<'a>>,
//     PinDriver<'a, BUSY, IN>,
//     PinDriver<'a, DC, OUT>,
//     PinDriver<'a, RST, OUT>,
// >;

// struct BenchDisplay<
//     'a,
//     RST: esp_idf_svc::hal::gpio::Pin,
//     DC: esp_idf_svc::hal::gpio::Pin,
//     BUSY: esp_idf_svc::hal::gpio::Pin,
//     OUT: esp_idf_svc::hal::gpio::OutputMode,
//     IN: esp_idf_svc::hal::gpio::InputMode,
// > {
//     ssd1680: Ssd1680<
//         &'a mut SpiDeviceDriver<'a, SpiDriver<'a>>,
//         PinDriver<'a, BUSY, IN>,
//         PinDriver<'a, DC, OUT>,
//         PinDriver<'a, RST, OUT>,
//     >,
// }

// impl<
//         'a,
//         RST: esp_idf_svc::hal::gpio::Pin,
//         DC: esp_idf_svc::hal::gpio::Pin,
//         BUSY: esp_idf_svc::hal::gpio::Pin,
//         OUT: esp_idf_svc::hal::gpio::OutputMode,
//         IN: esp_idf_svc::hal::gpio::InputMode,
//     > BenchDisplay<'a, RST, DC, BUSY, OUT, IN>
// {
//     pub fn new(spi: esp_idf_hal::peripherals::Peripherals   )-> Self {

//         BenchDisplay{
//             ssd1680 =
//         }

// }

// fn update_display<
//     'a,
//     RST: esp_idf_svc::hal::gpio::Pin,
//     DC: esp_idf_svc::hal::gpio::Pin,
//     BUSY: esp_idf_svc::hal::gpio::Pin,
//     OUT: esp_idf_svc::hal::gpio::OutputMode,
//     IN: esp_idf_svc::hal::gpio::InputMode,
// >(
//     ssd1680: &mut Ssd1680<
//         &mut SpiDeviceDriver<'a, SpiDriver<'a>>,
//         PinDriver<'a, BUSY, IN>,
//         PinDriver<'a, DC, OUT>,
//         PinDriver<'a, RST, OUT>,
//     >,
// ) {

fn text(
    display: &mut Display2in13,
    text: &str,
    x: i32,
    y: i32,
    width: usize,
    max_lines: usize,
    align: Alignment,
) {
    let wrapped_description = textwrap::wrap(text, width / 10);
    let merged = wrapped_description
        .into_iter()
        .take(max_lines)
        .collect::<Vec<std::borrow::Cow<str>>>()
        .join("\n");

    let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::Off);
    let _ = Text::with_text_style(
        merged.as_str(),
        Point::new(x, y),
        style,
        TextStyleBuilder::new().alignment(align).build(),
    )
    .draw(display);
}

type DisplayDriver<'a, 'b, BUSY, DC, RST, IN, OUT> = Ssd1680<
    &'b mut SpiDeviceDriver<'a, SpiDriver<'a>>,
    PinDriver<'a, BUSY, IN>,
    PinDriver<'a, DC, OUT>,
    PinDriver<'a, RST, OUT>,
>;

fn update_display<
    'a,
    RST: esp_idf_svc::hal::gpio::Pin,
    DC: esp_idf_svc::hal::gpio::Pin,
    BUSY: esp_idf_svc::hal::gpio::Pin,
    OUT: esp_idf_svc::hal::gpio::OutputMode,
    IN: esp_idf_svc::hal::gpio::InputMode,
>(
    ssd1680: &'a mut DisplayDriver<'_, 'a, BUSY, DC, RST, IN, OUT>,
    bench: &str,
    sw: &str,
    description: &str,
) {
    // Clear frames on the display driver
    ssd1680.clear_red_frame().unwrap();
    ssd1680.clear_bw_frame().unwrap();

    // Create buffer for black and white
    let mut display_bw = Display2in13::bw();

    display_bw.set_rotation(DisplayRotation::Rotate90);

    text(&mut display_bw, bench, 2, 20, 246, 1, Alignment::Left);
    text(&mut display_bw, sw, 246, 20, 246, 1, Alignment::Right);
    text(&mut display_bw, description, 2, 45, 246, 3, Alignment::Left);

    display_bw.set_rotation(DisplayRotation::Rotate0);

    // outer frame
    Rectangle::new(Point::new(0, 0), Size::new(122, 250))
        .into_styled(PrimitiveStyle::with_stroke(Black, 1))
        .draw(&mut display_bw)
        .unwrap();

    info!("Send bw frame to display");
    ssd1680.update_bw_frame(display_bw.buffer()).unwrap();

    info!("Update display");
    ssd1680.display_frame(&mut Ets).unwrap();

    info!("Done");
}

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let sysloop = EspSystemEventLoop::take()?;

    info!("Hello, world!");

    // The constant `CONFIG` is auto-generated by `toml_config`.
    let app_config = CONFIG;

    // Connect to the Wi-Fi network
    let (wifi, ip_info) = match wifi(
        app_config.wifi_ssid,
        app_config.wifi_psk,
        peripherals.modem,
        sysloop,
    ) {
        Ok(inner) => inner,
        Err(err) => {
            error!(
                "Could not connect to Wi-Fi network: {:?} | {:?}",
                app_config.wifi_ssid, err
            );
            std::thread::sleep(std::time::Duration::from_secs(5));
            reset::restart();
        }
    };

    let mac = hex::encode(wifi.get_mac(esp_idf_svc::wifi::WifiDeviceId::Sta).unwrap());

    let spi = peripherals.spi2;

    let rst = PinDriver::output(peripherals.pins.gpio20).unwrap();
    let dc = PinDriver::output(peripherals.pins.gpio10).unwrap();
    let busy = PinDriver::input(peripherals.pins.gpio21).unwrap();

    let sclk = peripherals.pins.gpio6;
    let sdo = peripherals.pins.gpio7;

    let spi = SpiDriver::new(
        spi,
        sclk,
        sdo,
        None::<AnyIOPin>,
        &SpiDriverConfig::default(),
    )
    .unwrap();

    let cs = peripherals.pins.gpio9;

    let mut spi =
        SpiDeviceDriver::new(spi, Some(cs), &esp_idf_svc::hal::spi::config::Config::new()).unwrap();

    // Initialise display controller
    let mut ssd1680 = Ssd1680::new(&mut spi, busy, dc, rst, &mut Ets).unwrap();

    update_display(
        &mut ssd1680,
        "Bench10",
        "Q352",
        "This is a very long long long long long long long long long long  text and even longer",
    );

    let broker_url = if app_config.mqtt_user != "" {
        format!(
            "mqtt://{}:{}@{}",
            app_config.mqtt_user, app_config.mqtt_pass, app_config.mqtt_host
        )
    } else {
        format!("mqtt://{}", app_config.mqtt_host)
    };

    let client_id = format!("display-{}", mac);

    let mqtt_config = MqttClientConfiguration {
        client_id: Some(&client_id),
        keep_alive_interval: Some(Duration::from_secs(25)),
        ..Default::default()
    };

    let (mut mqtt_client, mut mqtt_connection) =
        EspMqttClient::new(&broker_url, &mqtt_config).unwrap();
    run(&mut mqtt_client, &mut mqtt_connection, &mac, ip_info).unwrap();

    // let (mut client, mut conn) = mqtt_create(&broker_url, &client_id).unwrap();

    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        info!("Hello, world!");
    }
}

fn run(
    client: &mut EspMqttClient<'_>,
    connection: &mut EspMqttConnection,
    mac_address: &str,
    ip_info: IpInfo,
) -> Result<(), EspError> {
    std::thread::scope(|s| {
        info!("About to start the MQTT client");

        let topic = format!("devices/serial/display/{}", mac_address);

        // Need to immediately start pumping the connection for messages, or else subscribe() and publish() below will not work
        // Note that when using the alternative constructor - `EspMqttClient::new_cb` - you don't need to
        // spawn a new thread, as the messages will be pumped with a backpressure into the callback you provide.
        // Yet, you still need to efficiently process each message in the callback without blocking for too long.
        //
        // Note also that if you go to http://tools.emqx.io/ and then connect and send a message to topic
        // "esp-mqtt-demo", the client configured here should receive it.
        std::thread::Builder::new()
            .stack_size(6000)
            .spawn_scoped(s, move || {
                info!("MQTT Listening for messages");

                while let Ok(event) = connection.next() {
                    info!("[Queue] Event: {}", event.payload());
                }

                info!("Connection closed");
            })
            .unwrap();

        loop {
            if let Err(e) = client.subscribe(&topic, QoS::AtMostOnce) {
                error!("Failed to subscribe to topic \"{topic}\": {e}, retrying...");

                // Re-try in 0.5s
                std::thread::sleep(Duration::from_millis(500));

                continue;
            }

            info!("Subscribed to topic \"{topic}\"");

            // Just to give a chance of our connection to get even the first published message
            std::thread::sleep(Duration::from_millis(500));

            let payload = "Hello from esp-mqtt-demo!";

            loop {
                client.enqueue(&topic, QoS::AtMostOnce, false, payload.as_bytes())?;

                info!("Published \"{payload}\" to topic \"{topic}\"");

                let sleep_secs = 2;

                info!("Now sleeping for {sleep_secs}s...");
                std::thread::sleep(Duration::from_secs(sleep_secs));
            }
        }
    })
}
