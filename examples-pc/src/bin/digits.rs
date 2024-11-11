use clock_display_lib::display::ClockDisplay;
use clock_display_lib::is31fl3731::IS31FL3731;
use ftdi_embedded_hal as hal;
use heapless::Vec;
use libftd2xx::{self as ftdi};
use std::thread::sleep;
use std::time::Duration;

fn main() {
    let devices = ftdi::list_devices().expect("failed to list devices");
    let serial = devices.first().unwrap().serial_number.clone();
    let device = ftdi::Ft232h::with_serial_number(&serial).unwrap();
    let hal = hal::FtHal::init_freq(device, 400_000).unwrap();
    let i2c = hal.i2c().unwrap();

    let leds = IS31FL3731::new(i2c, 0x74);

    let mut v = Vec::new();
    _ = v.push(leds);
    let mut clock = ClockDisplay::new(v);

    clock.setup().unwrap();

    for _ in 0..=4 {
        for number in 0..=99 {
            clock.draw_symbol(0, number / 10, 0xFF).unwrap();
            clock.draw_symbol(1, number % 10, 0xFF).unwrap();
            sleep(Duration::from_millis(20));
        }
    }
}
