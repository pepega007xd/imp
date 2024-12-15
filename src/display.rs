use std::time::Duration;

use esp_idf_svc::hal::{
    gpio::{Gpio0, InputPin, OutputPin, PinDriver},
    spi::{SpiConfig, SpiDeviceDriver, SpiDriver, SpiDriverConfig, SPI3},
};
use ssd1306::{
    mode::{BufferedGraphicsMode, DisplayConfig},
    prelude::{DisplayRotation, SPIInterface, WriteOnlyDataCommand},
    size::DisplaySize128x64,
    Ssd1306,
};

/// Setup the SSD1306 display connected through SPI and clear the screen.
pub fn setup_display(
    spi: SPI3,
    sclk: impl InputPin + OutputPin,
    sdo: impl InputPin + OutputPin,
    dc: impl OutputPin,
    reset: impl OutputPin,
) -> Box<
    Ssd1306<impl WriteOnlyDataCommand, DisplaySize128x64, BufferedGraphicsMode<DisplaySize128x64>>,
> {
    let spi_driver = SpiDriver::new(
        spi,
        sclk,
        sdo,
        None as Option<Gpio0>,
        &SpiDriverConfig::default(),
    )
    .unwrap();

    let spi_device_driver =
        SpiDeviceDriver::new(spi_driver, None as Option<Gpio0>, &SpiConfig::default()).unwrap();

    let data_command = PinDriver::output(dc).unwrap();

    let interface = SPIInterface::new(spi_device_driver, data_command);

    // driver struct contains a large buffer with display content,
    // putting the object on the heap avoids stack overflows
    let mut display = Box::new(
        Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode(),
    );

    let mut display_reset = PinDriver::output(reset).unwrap();

    // display must be reset before initialization
    display_reset.set_low().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    display_reset.set_high().unwrap();

    // pin must not be dropped, the pin will otherwise be set
    // back to output => floating, and the display will reset itself
    std::mem::forget(display_reset);

    display.init().unwrap();
    display.flush().unwrap();

    display
}
