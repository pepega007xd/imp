use esp_idf_hal::gpio::PinDriver;
use esp_idf_svc::hal::{self as esp_idf_hal, gpio::InputPin};
use std::{sync::mpsc::Sender, thread};

use crate::InputEvent;

/// Spawns a new thread which waits on a button press using interrupt, then measures
/// the press length, removes bounces and sends an input event to the event loop.
pub fn spawn_button_listener(button_pin: impl InputPin, event_sender: Sender<InputEvent>) {
    thread::spawn(move || {
        let mut encoder_button = PinDriver::input(button_pin).unwrap();

        loop {
            esp_idf_hal::task::block_on(encoder_button.wait_for_falling_edge()).unwrap();
            let start = std::time::Instant::now();

            esp_idf_hal::task::block_on(encoder_button.wait_for_rising_edge()).unwrap();
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

/// Spawns a new thread which waits on a turn of the rotary encoder using interrupt,
/// then sends an input event to the event loop.
pub fn spawn_encoder_listener(
    s1: impl InputPin,
    s2: impl InputPin,
    event_sender: Sender<InputEvent>,
) {
    thread::spawn(move || {
        let mut s1 = PinDriver::input(s1).unwrap();
        let s2 = PinDriver::input(s2).unwrap();

        let mut second = false;

        loop {
            esp_idf_hal::task::block_on(async {
                s1.wait_for_rising_edge().await.unwrap();
            });

            // the rotary encoder generates two rising edges each turn, filter out every other
            // (this is consistent, given by the construction of the module, not by any bouncing)
            if !second {
                if s2.get_level() == s1.get_level() {
                    event_sender.send(InputEvent::ScrollUp).unwrap();
                } else {
                    event_sender.send(InputEvent::ScrollDown).unwrap();
                }
            }
            second = !second;
        }
    });
}
