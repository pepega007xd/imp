use esp_idf_hal::gpio::PinDriver;
use esp_idf_svc::hal::{self as esp_idf_hal, gpio::InputPin};
use std::{sync::mpsc::Sender, thread};

use crate::InputEvent;

pub fn spawn_button_listener(button_pin: impl InputPin, event_sender: Sender<InputEvent>) {
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

pub fn spawn_encoder_listener(
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
