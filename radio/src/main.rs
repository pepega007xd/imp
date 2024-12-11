use std::{
    fmt::Write,
    io::{stdin, Read},
    sync::mpsc::Sender,
    thread,
    time::Duration,
};

use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::{Point, *},
    primitives::{PrimitiveStyleBuilder, Rectangle, RoundedRectangle},
    text::{Baseline, Text},
};
use esp_idf_hal::{
    gpio::{Gpio0, PinDriver},
    i2c::{I2cConfig, I2cDriver},
    prelude::*,
    spi,
};
use esp_idf_svc::{
    hal::{
        self as esp_idf_hal,
        gpio::{InputPin, OutputPin, Pin},
    },
    sys as esp_idf_sys,
};

use rda5807m::{Address, Rda5708m};
use ssd1306::{mode::BufferedGraphicsMode, prelude::*, Ssd1306};

#[derive(Debug)]
enum InputEvent {
    ButtonPress,
    ScrollDown,
    ScrollUp,
}

enum UIElement {
    VolumeControl,
    FreqControl,
}

struct AppState {
    freq_khz: u32,
    volume: u8,

    cursor_at: UIElement,
    cursor_selected: bool,
}

fn spawn_button_listener(button_pin: impl InputPin, event_sender: Sender<InputEvent>) {
    thread::spawn(move || {
        let mut rot_enc_sw = PinDriver::input(button_pin).unwrap();

        loop {
            esp_idf_hal::task::block_on(rot_enc_sw.wait_for_rising_edge()).unwrap();
            event_sender.send(InputEvent::ButtonPress).unwrap();
        }
    });
}

fn spawn_encoder_listener(
    encoder_clock: impl InputPin,
    encoder_data: impl InputPin,
    event_sender: Sender<InputEvent>,
) {
    thread::spawn(move || {
        let mut encoder_clock = PinDriver::input(encoder_clock).unwrap();
        let encoder_data = PinDriver::input(encoder_data).unwrap();

        let mut second = false;

        loop {
            esp_idf_hal::task::block_on(async {
                encoder_clock.wait_for_rising_edge().await.unwrap();
            });
            let clk = encoder_clock.get_level();
            let data = encoder_data.get_level();
            if !second {
                if data == clk {
                    event_sender.send(InputEvent::ScrollUp).unwrap();
                } else {
                    event_sender.send(InputEvent::ScrollDown).unwrap();
                }
            }
            second = !second;
        }
    });
}

fn draw_mandelbrot<DI: WriteOnlyDataCommand, SIZE: DisplaySize>(
    display: &mut Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>>,
) {
    for y_int in 0..64 {
        for x_int in 0..128 {
            let orig_x = (x_int as f32 / 128.) * 3. - 2.;
            let orig_y = (y_int as f32 / 64.) * 2. - 1.;

            let (mut x, mut y) = (orig_x, orig_y);

            for _ in 0..50 {
                let tmp_x = x;
                x = x * x - y * y + orig_x;
                y = 2. * tmp_x * y + orig_y;
                if x * x + y * y > 4. {
                    display.set_pixel(x_int, y_int, true);
                    break;
                }
            }
        }
        display.flush().unwrap();
    }
}

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Starting...");

    let peripherals = Peripherals::take().unwrap();

    let sclk = peripherals.pins.gpio18;
    let sdo = peripherals.pins.gpio23;

    log::info!("initializing the SPI driver...");

    let spi_driver = spi::SpiDriver::new(
        peripherals.spi3, // TODO: really?
        sclk,
        sdo,
        None as Option<Gpio0>,
        &spi::SpiDriverConfig::default(),
    )
    .unwrap();

    log::info!("initializing the SPI device driver...");

    let spi_device_driver = spi::SpiDeviceDriver::new(
        spi_driver,
        None as Option<Gpio0>,
        &spi::SpiConfig::default(),
    )
    .unwrap();

    log::info!("SPI device driver initialized");
    let data_command = PinDriver::output(peripherals.pins.gpio13).unwrap();
    let interface = SPIInterface::new(spi_device_driver, data_command);
    log::info!("SPI display interface initialized");

    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();

    log::info!("SPI display driver created");

    let mut display_reset = PinDriver::output(peripherals.pins.gpio12).unwrap();

    display_reset.set_low().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    display_reset.set_high().unwrap();

    display.init().unwrap();
    display.set_invert(true).unwrap();

    display.flush().unwrap();

    let (event_sender, event_receiver) = std::sync::mpsc::channel::<InputEvent>();

    // unsafe {
    //     esp_idf_sys::esp_task_wdt_delete(esp_idf_sys::xTaskGetIdleTaskHandleForCore(
    //         esp_idf_hal::cpu::core().into(),
    //     ))
    // };

    spawn_button_listener(peripherals.pins.gpio17, event_sender.clone());

    spawn_encoder_listener(
        peripherals.pins.gpio25,
        peripherals.pins.gpio26,
        event_sender.clone(),
    );

    // stdin().lock().bytes().for_each(|c| {
    //     if let Ok(c) = c {
    //         display.write_char(c as char).unwrap()
    //     }
    // });

    /////////////////////////////////////////////////////////////////

    let i2c_driver = I2cDriver::new(
        peripherals.i2c0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio22,
        &I2cConfig::new().baudrate(KiloHertz(100).into()),
    )
    .unwrap();

    let mut tuner = Rda5708m::new(i2c_driver, Address::default());

    tuner.start().unwrap();
    std::thread::sleep(Duration::from_millis(100));

    tuner.set_volume(8).unwrap();
    tuner.set_frequency(88_200).unwrap();
    log::info!(
        "initialized tuner at {} kHz",
        tuner.get_frequency().unwrap()
    );

    let mut led_pin = PinDriver::output(peripherals.pins.gpio2).unwrap();

    while let Ok(event) = event_receiver.recv() {
        let current_frequency = tuner.get_frequency().unwrap();

        match event {
            InputEvent::ButtonPress => {
                led_pin.toggle().unwrap();
            }
            InputEvent::ScrollDown => {
                tuner.set_frequency(current_frequency - 100).unwrap();
            }
            InputEvent::ScrollUp => {
                tuner.set_frequency(current_frequency + 100).unwrap();
            }
        }

        let current_frequency = tuner.get_frequency().unwrap();
        // log::info!("Received event: {:?}", event);

        log::info!("frequency: {current_frequency}");

        let status = tuner.get_status().unwrap();
        log::info!("{status:?}");
        let rssi = (tuner.get_rssi().unwrap() / 16).into();
        log::info!("[{}{}]", "*".repeat(rssi), " ".repeat(16 - rssi));
        // let blocks = tuner.get_rds_registers().unwrap();
        // log::info!("{:?}", blocks);
    }

    loop {
        led_pin.set_high().unwrap();
        std::thread::sleep(Duration::from_secs(1));
        led_pin.set_low().unwrap();
        std::thread::sleep(Duration::from_secs(1));
    }
}
