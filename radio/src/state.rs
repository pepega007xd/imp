use esp_idf_svc::nvs::{EspNvs, NvsDefault};
use std::sync::mpsc::Sender;

use crate::{AppState, InputEvent, OutputCommand, UIElement, NUM_PRESETS};

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

impl AppState {
    pub fn new() -> AppState {
        AppState {
            freq_khz: 100_000,
            volume: 5,
            station_info: "".to_string(),
            rssi: 0,
            cursor_at: UIElement::SeekDown,
            cursor_selected: false,
        }
    }

    pub fn process_event(
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
}
