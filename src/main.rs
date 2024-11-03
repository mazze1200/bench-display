use anyhow::{bail, Error, Result};
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
use std::sync::mpsc::channel;
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

    info!("Connecting to MQTT broker: {}", broker_url);

    let client_id = format!("display-{}", mac);

    let mqtt_config = MqttClientConfiguration {
        client_id: Some(&client_id),
        keep_alive_interval: Some(Duration::from_secs(25)),
        ..Default::default()
    };

    let (mut mqtt_client, mut mqtt_connection) =
        EspMqttClient::new(&broker_url, &mqtt_config).unwrap();

    loop {
        let res = run(&mut mqtt_client, &mut mqtt_connection, &mac, ip_info);
        if let Err(error) = res {
            info!("Error: {}", error);
        }

        info!("Waiting");
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

struct Bench {
    name: String,
    topic: String,
}

struct SW {
    bench: String,
    version: String,
    description: String,
}
enum MQTTEvent {
    Connected,
    Received((String, String)),
}

fn run(
    client: &mut EspMqttClient<'_>,
    connection: &mut EspMqttConnection,
    mac_address: &str,
    ip_info: IpInfo,
) -> Result<(), EspError> {
    std::thread::scope(|s| {
        let (sender, receiver) = channel::<MQTTEvent>();

        std::thread::Builder::new()
            .stack_size(6000)
            .spawn_scoped(s, move || {
                info!("MQTT Listening for messages");

                // for event in connection.next() {
                while let Ok(event) = connection.next() {
                    let payload = event.payload();
                    match payload {
                        EventPayload::Connected(_) => {
                            info!("Connected");
                            sender.send(MQTTEvent::Connected).unwrap();
                        }
                        EventPayload::Received {
                            id: _,
                            topic,
                            data,
                            details: _,
                        } => {
                            if let Some(topic) = topic {
                                let data = String::from_utf8_lossy(data).into_owned();
                                info!("Received {} = {}", topic, data);
                                sender
                                    .send(MQTTEvent::Received((topic.to_string(), data)))
                                    .unwrap();
                            }
                        }
                        _ => {
                            info!("Receving other event {:?}", payload);
                        }
                    };
                }

                info!("Connection closed");
            })
            .unwrap();

        std::thread::Builder::new()
            .stack_size(6000)
            .spawn_scoped(s, move || {
                let bench_topic = format!("display/serial/{}/bench", mac_address);
                let mut bench: Option<String> = None;
                let mut description: Option<String> = None;
                let mut description_topic: Option<String> = None;
                let mut software: Option<String> = None;
                let mut software_topic: Option<String> = None;

                for event in receiver {
                    match event {
                        MQTTEvent::Connected => {
                            info!("Subscribing to {}", &bench_topic);
                            client.subscribe(&bench_topic, QoS::AtLeastOnce).unwrap();

                            let ip_topic = format!("display/serial/{}/ip", mac_address);
                            info!("Publishing to {}", &ip_topic);
                            client
                                .publish(
                                    &ip_topic,
                                    QoS::AtLeastOnce,
                                    true,
                                    ip_info.ip.to_string().as_bytes(),
                                )
                                .unwrap();
                        }
                        MQTTEvent::Received((topic, data)) => {
                            if topic == bench_topic {
                                if let Some(bench) = &bench {
                                    if bench == &data {
                                        // No change, do nothing
                                        continue;
                                    }
                                }
                                if let Some(topic) = description_topic {
                                    info!("Unsubscribing from {}", topic);
                                    client.unsubscribe(&topic).unwrap();
                                }

                                if let Some(topic) = software_topic {
                                    info!("Unsubscribing from {}", topic);
                                    client.unsubscribe(&topic).unwrap();
                                }

                                description = None;
                                software = None;

                                let new_description_topic = format!("benches/{}/description", data);
                                info!("Subscribing to {}", new_description_topic);
                                client
                                    .subscribe(&new_description_topic, QoS::AtLeastOnce)
                                    .unwrap();
                                description_topic = Some(new_description_topic);

                                let new_software_topic = format!("diagnosis/{}/software", data);
                                info!("Subscribing to {}", new_software_topic);
                                client
                                    .subscribe(&new_software_topic, QoS::AtLeastOnce)
                                    .unwrap();
                                software_topic = Some(new_software_topic);

                                bench = Some(data);
                            } else {
                                let mut need_update = false;
                                let option_topic = Some(topic);
                                if option_topic == description_topic {
                                    if let Some(desc) = &description {
                                        if desc == &data {
                                            // No change, do nothing
                                            continue;
                                        }
                                    }

                                    info!("Updating desciprion: {}", data);
                                    description = Some(data.to_string());
                                    need_update = true;
                                } else if option_topic == software_topic {
                                    if let Some(sw) = &software {
                                        if sw == &data {
                                            // No change, do nothing
                                            continue;
                                        }
                                    }

                                    info!("Updating software: {}", data);
                                    software = Some(data.to_string());
                                    need_update = true;
                                }

                                if need_update {
                                    if let (Some(bench), Some(decription), Some(software)) =
                                        (&bench, &description, &software)
                                    {
                                        info!(
                                            "Update Display: Bench={}, Software={}, Description={}",
                                            bench, decription, software
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            })
            .unwrap();

        loop {
            info!("Waiting");
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });

    loop {
        info!("outer Waiting");
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
