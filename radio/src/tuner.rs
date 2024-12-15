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

pub fn spawn_tuner_thread(
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
