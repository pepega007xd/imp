use esp_idf_svc::nvs::{EspNvs, NvsDefault};
use std::sync::mpsc::Sender;

use crate::{AppState, InputEvent, OutputCommand, UIElement, NUM_PRESETS};

impl UIElement {
    /// returns the previous UI element in a loop
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

    /// returns the next UI element in a loop
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

impl AppState {
    pub fn new() -> AppState {
        AppState {
            freq_khz: 100_000,
            volume: 0,
            station_info: "".to_string(),
            rssi: 0,
            cursor_at: UIElement::SeekDown,
            element_is_active: false,
        }
    }

    /// Updates the application state based on the current state and the given input event.
    pub fn process_event(
        &mut self,
        event: InputEvent,
        command: &Sender<OutputCommand>,
        nvs: &mut EspNvs<NvsDefault>,
    ) {
        // names for NVS variables
        const PRESET_NAMES: [&str; 4] = ["preset1", "preset2", "preset3", "preset4"];

        use InputEvent as I;
        match (self.cursor_at, self.element_is_active, event) {
            // scrolling through UI elements
            (_, false, I::ScrollDown) => self.cursor_at = self.cursor_at.prev(),
            (_, false, I::ScrollUp) => self.cursor_at = self.cursor_at.next(),

            // events from radio
            (_, _, I::ChangeFrequency(freq)) => self.freq_khz = freq,
            (_, _, I::ChangeStationInfo(info)) => self.station_info = info,
            (_, _, I::ChangeRSSI(rssi)) => self.rssi = rssi,

            // seek down
            (UIElement::SeekDown, false, I::ShortPress) => {
                command.send(OutputCommand::SeekDown).unwrap()
            }

            // de/selecting frequency or volume control
            (UIElement::FreqControl | UIElement::VolumeControl, _, I::ShortPress) => {
                self.element_is_active = !self.element_is_active
            }

            // frequency control
            (UIElement::FreqControl, true, I::ScrollDown) => {
                if self.freq_khz > 76_000 {
                    self.freq_khz -= 100;
                    command
                        .send(OutputCommand::SetFrequency(self.freq_khz))
                        .unwrap();
                }
            }
            (UIElement::FreqControl, true, I::ScrollUp) => {
                if self.freq_khz < 108_000 {
                    self.freq_khz = self.freq_khz + 100;
                    command
                        .send(OutputCommand::SetFrequency(self.freq_khz))
                        .unwrap();
                }
            }

            // seek up
            (UIElement::SeekUp, false, I::ShortPress) => {
                command.send(OutputCommand::SeekUp).unwrap()
            }

            // select preset
            (UIElement::Preset(preset), false, I::ShortPress) => {
                if let Ok(Some(freq)) = nvs.get_u32(PRESET_NAMES[preset as usize]) {
                    self.freq_khz = freq;
                    command
                        .send(OutputCommand::SetFrequency(self.freq_khz))
                        .unwrap();
                }
            }
            // set preset
            (UIElement::Preset(preset), false, I::LongPress) => nvs
                .set_u32(PRESET_NAMES[preset as usize], self.freq_khz)
                .unwrap(),

            // volume control
            (UIElement::VolumeControl, true, I::ScrollDown) => {
                if self.volume > 0 {
                    self.volume -= 1;
                    command.send(OutputCommand::SetVolume(self.volume)).unwrap();
                }
            }
            (UIElement::VolumeControl, true, I::ScrollUp) => {
                if self.volume < 15 {
                    self.volume += 1;
                    command.send(OutputCommand::SetVolume(self.volume)).unwrap();
                }
            }

            // ignore all other user inputs
            (_, _, I::LongPress) => (),

            // any other combinations should be unreachable
            _ => unreachable!(),
        }
    }
}
