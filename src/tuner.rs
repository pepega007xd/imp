use esp_idf_svc::hal::{
    gpio::{InputPin, OutputPin},
    i2c::{I2cConfig, I2cDriver, I2C0},
    units::KiloHertz,
};
use rda5807m::{Address, Rda5708m};
use std::{
    sync::mpsc::{Receiver, Sender},
    thread,
    time::Duration,
};

use crate::{InputEvent, OutputCommand};

/// Spawns a new thread, in which the RDA5708 tuner is initialized and
/// tuned to a default frequency.
///
/// Then, commands from the event loop are periodically processed, new data from
/// the tuner is fetched and sent back to the event loop.
pub fn spawn_tuner_thread(
    i2c: I2C0,
    sda: impl InputPin + OutputPin,
    scl: impl InputPin + OutputPin,
    event_sender: Sender<InputEvent>,
    command_receiver: Receiver<OutputCommand>,
) {
    thread::spawn(move || {
        let mut config = I2cConfig::new().baudrate(KiloHertz(100).into());

        // without setting a timeout value, the tuner would occasionally timeout
        config.timeout = Some(Duration::from_millis(10).into());
        let i2c_driver = I2cDriver::new(i2c, sda, scl, &config).unwrap();

        let mut tuner = Rda5708m::new(i2c_driver, Address::default());

        tuner.start().unwrap();
        std::thread::sleep(Duration::from_millis(100));

        // set default values
        tuner.set_seek_threshold(35).unwrap();
        tuner.set_frequency(100_000).unwrap();
        tuner.set_volume(0).unwrap(); // TODO: set to 5

        let mut prev_freq = 0;
        let mut prev_rssi = 0;

        const EMPTY_TEXT: [char; 64] = [' '; 64];
        let mut radio_text = EMPTY_TEXT;

        loop {
            let status = tuner.get_status().unwrap();
            let (blera, blerb) = tuner.get_block_errors().unwrap();

            if let Ok(command) = command_receiver.try_recv() {
                // if the command changes tuner frequency, reset the radio text
                if let OutputCommand::SetVolume(_) = command {
                } else {
                    radio_text = EMPTY_TEXT;
                    let info = radio_text.iter().collect::<String>();
                    event_sender
                        .send(InputEvent::ChangeStationInfo(info))
                        .unwrap();
                }

                // process command from event loop
                match command {
                    OutputCommand::SetFrequency(freq) => tuner.set_frequency(freq).unwrap(),
                    OutputCommand::SetVolume(volume) => tuner.set_volume(volume).unwrap(),
                    OutputCommand::SeekUp => tuner.seek_up(true).unwrap(),
                    OutputCommand::SeekDown => tuner.seek_down(true).unwrap(),
                }

                thread::sleep(Duration::from_millis(10));
            }

            // update RSSI
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

            // update Radio Text
            if status.rdss {
                let [_, block_b, block_c, block_d] = tuner.get_rds_registers().unwrap();
                let group_type = (block_b >> 12) & 0xF;

                // only display characters if there are no detected errors
                if group_type == 2 && blera == 0 && blerb == 0 {
                    let offset = block_b & 0xF;

                    radio_text[(offset * 4 + 0) as usize] = (block_c >> 8) as u8 as char;
                    radio_text[(offset * 4 + 1) as usize] = (block_c & 0xFF) as u8 as char;
                    radio_text[(offset * 4 + 2) as usize] = (block_d >> 8) as u8 as char;
                    radio_text[(offset * 4 + 3) as usize] = (block_d & 0xFF) as u8 as char;

                    let info = radio_text.iter().collect::<String>();
                    event_sender
                        .send(InputEvent::ChangeStationInfo(info))
                        .unwrap();
                }
            }

            thread::sleep(Duration::from_millis(100));
        }
    });
}
