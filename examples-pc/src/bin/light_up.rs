use ftdi_embedded_hal as hal;
use ftdi_embedded_hal::eh0::prelude::_embedded_hal_blocking_i2c_Write;
use libftd2xx::{self as ftdi};

fn main() {
    let devices = ftdi::list_devices().expect("failed to list devices");
    println!("list_devices: {:?}", devices);

    let serial = devices.first().unwrap().serial_number.clone();
    let device = ftdi::Ft232h::with_serial_number(&serial).unwrap();

    let hal = hal::FtHal::init_freq(device, 400_000).unwrap();
    let mut i2c = hal.i2c().unwrap();
    i2c.write(0x74, &[0xFD, 0x0B]).unwrap(); // PAGE 9
    i2c.write(0x74, &[0x0A, 0xFF]).unwrap(); // enable
    i2c.write(0x74, &[0xFD, 0x00]).unwrap(); // PAGE 1
    i2c.write(0x74, &[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]).unwrap(); // all on

    let mut arr = vec![0x24];
    arr.extend_from_slice(&[0xFFu8; 144]);
    i2c.write(0x74, arr.as_slice()).unwrap(); // full blast
}