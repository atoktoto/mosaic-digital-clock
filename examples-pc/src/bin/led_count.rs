use std::io::stdin;
use std::thread::sleep;
use std::time::Duration;
use ftdi_embedded_hal as hal;
use libftd2xx::{self as ftdi};
use is31fl3731_driver::IS31FL3731;

fn main() {
    let devices = ftdi::list_devices().expect("failed to list devices");
    println!("list_devices: {:?}", devices);

    let serial = devices.first().unwrap().serial_number.clone();
    let device = ftdi::Ft232h::with_serial_number(&serial).unwrap();
    let hal = hal::FtHal::init_freq(device, 400_000).unwrap();
    let i2c = hal.i2c().unwrap();

    let mut leds = IS31FL3731::new(i2c, 0x74);
    leds.setup().unwrap();
    leds.shutdown(false).unwrap();
    leds.clear_color().unwrap();
    leds.enable_leds(&[128, 135, 136]).unwrap();

    let mut buffer = String::new();

    for i in 0..143 {
        leds.set_color_byte(i, 0xFF).unwrap();
        println!("Led: {}", i);
        stdin().read_line(&mut buffer).unwrap();
        leds.set_color_byte(i, 0x00).unwrap();
    }

    sleep(Duration::from_secs(3));

    leds.set_color(0, &[0x00; 144]).unwrap();
}