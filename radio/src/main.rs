mod display;
mod gui;
mod input;
mod state;
mod tuner;

use esp_idf_svc::{hal::prelude::Peripherals, nvs::EspDefaultNvsPartition};
use std::sync::mpsc::channel;

use display::setup_display;
use input::{spawn_button_listener, spawn_encoder_listener};
use tuner::spawn_tuner_thread;

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

struct AppState {
    freq_khz: u32,
    volume: u8,
    station_info: String,
    rssi: u8,

    cursor_at: UIElement,
    cursor_selected: bool,
}

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    // initialize nonvolatile storage
    let mut nvs =
        esp_idf_svc::nvs::EspNvs::new(EspDefaultNvsPartition::take().unwrap(), "namespace", true)
            .unwrap();

    // initialize GPIO
    let peripherals = Peripherals::take().unwrap();

    // create channels for sending inputs and outputs
    let (event_sender, event_receiver) = channel::<InputEvent>();
    let (command_sender, command_receiver) = channel::<OutputCommand>();

    // setup listener for button presses
    spawn_button_listener(peripherals.pins.gpio17, event_sender.clone());

    // setup listener for rotary encoder inputs
    spawn_encoder_listener(
        peripherals.pins.gpio25,
        peripherals.pins.gpio26,
        event_sender.clone(),
    );

    // setup RDA5807M tuner
    spawn_tuner_thread(
        peripherals.i2c0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio22,
        event_sender,
        command_receiver,
    );

    // setup SSD1306 display
    let mut display = setup_display(
        peripherals.spi3,
        peripherals.pins.gpio18,
        peripherals.pins.gpio23,
        peripherals.pins.gpio13,
        peripherals.pins.gpio12,
    );

    // initialize application state
    let mut state = AppState::new();

    // draw GUI
    state.update_ui(&mut display).unwrap();

    // event loop - wait for next input event, process it, and update GUI
    while let Ok(event) = event_receiver.recv() {
        state.process_event(event, command_sender.clone(), &mut nvs);
        state.update_ui(&mut display).unwrap();
    }
}
