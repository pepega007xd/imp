use std::{
    any::Any,
    fmt::Write,
    io::{stdin, Read},
    sync::mpsc::{Receiver, Sender},
    thread,
    time::Duration,
};

use embedded_graphics::{
    mono_font::{
        ascii::{FONT_5X8, FONT_6X10},
        MonoTextStyle, MonoTextStyleBuilder,
    },
    pixelcolor::BinaryColor,
    prelude::{Point, *},
    primitives::{
        Arc, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, RoundedRectangle, StrokeAlignment,
        StyledDrawable, Triangle,
    },
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
        i2c::{I2c, I2C0},
        peripheral::Peripheral,
        spi::{Spi, SPI3},
    },
    handle::RawHandle,
    nvs::{EspDefaultNvsPartition, EspNvs, NvsDefault},
    sys::{self as esp_idf_sys, cc_t},
};

use rda5807m::{Address, Rda5708m};
use ssd1306::{mode::BufferedGraphicsMode, prelude::*, Ssd1306};

#[derive(Clone, Debug, PartialEq, Eq)]
enum InputEvent {
    ShortPress,
    LongPress,
    ScrollDown,
    ScrollUp,
    ChangeFrequency(u32),
    ChangeStationInfo(String),
    ChangeRSSI(u8),
}

enum OutputCommand {
    SetFrequency(u32),
    SetVolume(u8),
    SeekUp,
    SeekDown,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UIElement {
    SeekDown,
    FreqControl,
    SeekUp,
    Preset(u8),
    VolumeControl,
}

const NUM_PRESETS: u8 = 4;

impl UIElement {
    fn prev(self) -> Self {
        use UIElement as U;

        match self {
            U::SeekDown => U::VolumeControl,
            U::FreqControl => U::SeekDown,
            U::SeekUp => U::FreqControl,
            U::Preset(0) => U::SeekUp,
            U::Preset(x) => U::Preset(x - 1),
            U::VolumeControl => U::Preset(NUM_PRESETS - 1),
        }
    }

    fn next(self) -> Self {
        use UIElement as U;

        match self {
            U::SeekDown => U::FreqControl,
            U::FreqControl => U::SeekUp,
            U::SeekUp => U::Preset(0),
            U::Preset(x) if x < NUM_PRESETS - 1 => U::Preset(x + 1),
            U::Preset(_) => U::VolumeControl,
            U::VolumeControl => U::SeekDown,
        }
    }
}

struct AppState {
    freq_khz: u32,
    volume: u8,
    station_info: String,
    rssi: u8,

    cursor_at: UIElement,
    cursor_selected: bool,
}

impl AppState {
    fn new() -> AppState {
        AppState {
            freq_khz: 100_000,
            volume: 5,
            station_info: "".to_string(),
            rssi: 0,
            cursor_at: UIElement::SeekDown,
            cursor_selected: false,
        }
    }

    fn process_event(
        &mut self,
        event: InputEvent,
        command_sender: Sender<OutputCommand>,
        nvs: &mut EspNvs<NvsDefault>,
    ) {
        const PRESET_NAMES: [&str; 4] = ["preset1", "preset2", "preset3", "preset4"];
        match (self.cursor_at, self.cursor_selected, event) {
            // scrolling through UI elements
            (_, false, InputEvent::ScrollDown) => self.cursor_at = self.cursor_at.prev(),
            (_, false, InputEvent::ScrollUp) => self.cursor_at = self.cursor_at.next(),

            // events from radio
            (_, _, InputEvent::ChangeFrequency(freq)) => self.freq_khz = freq,
            (_, _, InputEvent::ChangeStationInfo(_)) => todo!(),
            (_, _, InputEvent::ChangeRSSI(rssi)) => self.rssi = rssi,

            // seek down
            (UIElement::SeekDown, false, InputEvent::ShortPress) => {
                command_sender.send(OutputCommand::SeekDown).unwrap()
            }

            // de/selecting frequency or volume control
            (UIElement::FreqControl | UIElement::VolumeControl, _, InputEvent::ShortPress) => {
                self.cursor_selected = !self.cursor_selected
            }

            // frequency control
            (UIElement::FreqControl, true, InputEvent::ScrollDown) => {
                // TODO: freq bounds
                self.freq_khz -= 100;
                command_sender
                    .send(OutputCommand::SetFrequency(self.freq_khz))
                    .unwrap();
            }
            (UIElement::FreqControl, true, InputEvent::ScrollUp) => {
                self.freq_khz = self.freq_khz + 100;
                command_sender
                    .send(OutputCommand::SetFrequency(self.freq_khz))
                    .unwrap();
            }

            // seek up
            (UIElement::SeekUp, false, InputEvent::ShortPress) => {
                command_sender.send(OutputCommand::SeekUp).unwrap()
            }

            // select preset
            (UIElement::Preset(preset), false, InputEvent::ShortPress) => {
                if let Ok(Some(freq)) = nvs.get_u32(PRESET_NAMES[preset as usize]) {
                    self.freq_khz = freq;
                    command_sender
                        .send(OutputCommand::SetFrequency(self.freq_khz))
                        .unwrap();
                }
            }
            // set preset
            (UIElement::Preset(preset), false, InputEvent::LongPress) => nvs
                .set_u32(PRESET_NAMES[preset as usize], self.freq_khz)
                .unwrap(),

            // volume control
            (UIElement::VolumeControl, true, InputEvent::ScrollDown) => {
                if self.volume > 0 {
                    self.volume -= 1;
                    command_sender
                        .send(OutputCommand::SetVolume(self.volume))
                        .unwrap();
                }
            }
            (UIElement::VolumeControl, true, InputEvent::ScrollUp) => {
                if self.volume < 15 {
                    self.volume += 1;
                    command_sender
                        .send(OutputCommand::SetVolume(self.volume))
                        .unwrap();
                }
            }

            // ignore all other user inputs
            (_, _, InputEvent::LongPress) => (),

            // any other combination of inputs should be unreachable
            _ => unreachable!(),
        }
    }

    fn update_ui<DI: WriteOnlyDataCommand, SIZE: DisplaySize>(
        &self,
        display: &mut Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>>,
    ) -> Result<(), <Ssd1306<DI, SIZE, BufferedGraphicsMode<SIZE>> as DrawTarget>::Error> {
        display.clear(BinaryColor::Off)?;

        let stroke_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
        let thick_stroke_style = PrimitiveStyleBuilder::new()
            .stroke_width(3)
            .stroke_alignment(StrokeAlignment::Inside)
            .stroke_color(BinaryColor::On)
            .build();

        let fill_style = PrimitiveStyle::with_fill(BinaryColor::On);

        let text_style = MonoTextStyle::new(
            &&embedded_graphics::mono_font::ascii::FONT_6X9,
            BinaryColor::On,
        );
        let big_text_style = MonoTextStyle::new(
            &embedded_graphics::mono_font::ascii::FONT_10X20,
            BinaryColor::On,
        );

        let left_arrow = Triangle::new(Point::new(0, 0), Point::new(5, -5), Point::new(5, 5))
            .into_styled(fill_style);
        let right_arrow = Triangle::new(Point::new(5, 0), Point::new(0, -5), Point::new(0, 5))
            .into_styled(fill_style);

        let selection_box = |ui_element, x, y, sx, sy, display: &mut _| {
            let style = if self.cursor_selected {
                thick_stroke_style
            } else {
                stroke_style
            };

            if self.cursor_at == ui_element {
                RoundedRectangle::with_equal_corners(
                    Rectangle::new(Point::new(x, y), Size::new(sx, sy)),
                    Size::new(3, 3),
                )
                .into_styled(style)
                .draw(display)
            } else {
                Ok(())
            }
        };

        // -- Seek down --
        selection_box(UIElement::SeekDown, 0, 0, 20, 20, display)?;
        left_arrow.translate(Point::new(4, 9)).draw(display)?;
        left_arrow.translate(Point::new(10, 9)).draw(display)?;

        // -- Frequency setting --
        selection_box(UIElement::FreqControl, 25, 0, 60, 20, display)?;

        let freq = self.freq_khz as f32 / 1000.;
        Text::new(
            format!("{freq:.1}").as_str(),
            Point::new(33, 15),
            big_text_style,
        )
        .draw(display)?;

        // -- Seek up button --
        selection_box(UIElement::SeekUp, 90, 0, 20, 20, display)?;
        right_arrow.translate(Point::new(94, 9)).draw(display)?;

        right_arrow.translate(Point::new(100, 9)).draw(display)?;

        // -- Volume control --

        selection_box(UIElement::VolumeControl, 115, 0, 13, 40, display)?;

        for level in 0..self.volume {
            Rectangle::new(Point::new(117, 34 - level as i32 * 2), Size::new(9, 1))
                .draw_styled(&fill_style, display)?;
        }

        // Level indicator
        selection_box(UIElement::VolumeControl, 109, 45, 19, 19, display)?;
        left_arrow.translate(Point::new(113, 54)).draw(display)?;
        Rectangle::new(Point::new(112, 52), Size::new(5, 5))
            .into_styled(fill_style)
            .draw(display)?;
        for line in 1..=2 {
            Arc::with_center(
                Point::new(118, 53),
                line * 6,
                Angle::from_degrees(-60.),
                Angle::from_degrees(120.),
            )
            .draw_styled(&stroke_style, display)?;
        }

        // station info
        Text::new("Station info", Point::new(5, 30), text_style).draw(display)?;

        // station info 2?
        Text::new("Something else ???", Point::new(5, 40), text_style).draw(display)?;

        // -- Preset stations --
        for preset in 0..NUM_PRESETS {
            let element = UIElement::Preset(preset);
            let preset = preset as i32;
            selection_box(element, preset * 25, 45, 19, 19, display)?;

            Text::new(
                format!("{preset}").as_str(),
                Point::new(preset * 25 + 5, 60),
                big_text_style,
            )
            .draw(display)?;
        }

        display.flush()
    }
}

fn spawn_button_listener(button_pin: impl InputPin, event_sender: Sender<InputEvent>) {
    thread::spawn(move || {
        let mut rot_enc_sw = PinDriver::input(button_pin).unwrap();

        loop {
            esp_idf_hal::task::block_on(rot_enc_sw.wait_for_falling_edge()).unwrap();
            let start = std::time::Instant::now();

            esp_idf_hal::task::block_on(rot_enc_sw.wait_for_rising_edge()).unwrap();
            let duration = std::time::Instant::now() - start;

            match duration.as_millis() {
                // debouncing
                0..50 => (),
                50..600 => event_sender.send(InputEvent::ShortPress).unwrap(),
                600.. => event_sender.send(InputEvent::LongPress).unwrap(),
            }
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

fn spawn_tuner_thread(
    i2c: I2C0,
    sda: impl InputPin + OutputPin,
    scl: impl InputPin + OutputPin,
    event_sender: Sender<InputEvent>,
    command_receiver: Receiver<OutputCommand>,
) {
    thread::spawn(move || {
        let mut config = I2cConfig::new().baudrate(KiloHertz(100).into());
        config.timeout = Some(Duration::from_millis(10).into());
        let i2c_driver = I2cDriver::new(i2c, sda, scl, &config).unwrap();

        let mut tuner = Rda5708m::new(i2c_driver, Address::default());

        tuner.start().unwrap();
        std::thread::sleep(Duration::from_millis(100));

        tuner.set_seek_threshold(35).unwrap();
        tuner.set_frequency(100_000).unwrap();
        tuner.set_volume(5).unwrap();

        let mut prev_freq = 0;
        let mut prev_rssi = 0;

        loop {
            let status = tuner.get_status().unwrap();

            if let Ok(command) = command_receiver.try_recv() {
                match command {
                    OutputCommand::SetFrequency(freq) => tuner.set_frequency(freq).unwrap(),
                    OutputCommand::SetVolume(volume) => tuner.set_volume(volume).unwrap(),
                    OutputCommand::SeekUp => tuner.seek_up(true).unwrap(),
                    OutputCommand::SeekDown => tuner.seek_down(true).unwrap(),
                }
            }

            let rssi = tuner.get_rssi().unwrap();
            if rssi.abs_diff(prev_rssi) > 5 {
                event_sender.send(InputEvent::ChangeRSSI(rssi)).unwrap();
                prev_rssi = rssi;
            }

            let freq = tuner.get_frequency().unwrap();

            // only send frequency updates when seeking
            if prev_freq != freq && !status.stc {
                event_sender
                    .send(InputEvent::ChangeFrequency(freq))
                    .unwrap();
                prev_freq = freq;
            }

            // TODO: read rds
            // let [a, b, c, d] = tuner.get_rds_registers().unwrap();
            // // println!("{a:x} {b:x} {c:x} {d:x}");
            // if status.rdss || status.rdsr {
            //     println!("AAAAAA");
            // }
            // let char1 = (c >> 8) as u8 as char;
            // let char2 = (d >> 8) as u8 as char;
            // // println!("{char1}{char2}");

            thread::sleep(Duration::from_millis(100));
        }
    });
}

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();
    log::info!("Starting...");

    // Initialize NVS
    let mut nvs =
        esp_idf_svc::nvs::EspNvs::new(EspDefaultNvsPartition::take().unwrap(), "namespace", true)
            .unwrap();

    let peripherals = Peripherals::take().unwrap();

    let (event_sender, event_receiver) = std::sync::mpsc::channel::<InputEvent>();
    let (command_sender, command_receiver) = std::sync::mpsc::channel::<OutputCommand>();

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

    spawn_tuner_thread(
        peripherals.i2c0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio22,
        event_sender,
        command_receiver,
    );

    let spi_driver = spi::SpiDriver::new(
        peripherals.spi3,
        peripherals.pins.gpio18,
        peripherals.pins.gpio23,
        None as Option<Gpio0>,
        &spi::SpiDriverConfig::default(),
    )
    .unwrap();

    let spi_device_driver = spi::SpiDeviceDriver::new(
        spi_driver,
        None as Option<Gpio0>,
        &spi::SpiConfig::default(),
    )
    .unwrap();

    let data_command = PinDriver::output(peripherals.pins.gpio13).unwrap();
    let interface = SPIInterface::new(spi_device_driver, data_command);

    // driver struct contains a large buffer with display content,
    // putting the object on the heap avoids stack overflows
    let mut display = Box::new(
        Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode(),
    );

    let mut display_reset = PinDriver::output(peripherals.pins.gpio12).unwrap();

    display_reset.set_low().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    display_reset.set_high().unwrap();

    display.init().unwrap();

    display.flush().unwrap();

    let mut led_pin = PinDriver::output(peripherals.pins.gpio2).unwrap();

    let mut state = AppState::new();

    state.update_ui(&mut display).unwrap();

    while let Ok(event) = event_receiver.recv() {
        if let InputEvent::ShortPress = event {
            dbg!("SHORT");
        };
        if let InputEvent::LongPress = event {
            dbg!("LONG");
        };
        state.process_event(event, command_sender.clone(), &mut nvs);
        state.update_ui(&mut display).unwrap();
    }
}
