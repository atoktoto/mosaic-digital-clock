use bitvec::prelude::*;
use embedded_hal::i2c;

pub struct IS31FL3731<I2C> {
    pub i2c: I2C,
    pub address: u8,
}

impl<I2C> IS31FL3731<I2C>
where
    I2C: i2c::I2c,
{
    pub fn new(i2c: I2C, address: u8) -> IS31FL3731<I2C> {
        IS31FL3731 { i2c, address }
    }

    pub fn setup(&mut self) -> Result<(), Error<I2C::Error>> {
        self.shutdown(false)?;
        self.display_frame(0)?;
        self.select_mode(modes::PICTURE_MODE)?;
        self.select_page(0)?;
        self.set_color(0, &[0x00; 144])?;
        Ok(())
    }

    pub fn enable_leds(&mut self, disabled_leds: &[u8]) -> Result<(), Error<I2C::Error>> {
        let mut all_on = [0xFF; 18];
        let all_on_bits = all_on.view_bits_mut::<Lsb0>();
        for &disabled in disabled_leds {
            all_on_bits.set(disabled as usize, false);
        }

        self.set_onoff(0, &all_on)?;

        Ok(())
    }

    pub fn display_frame(&mut self, frame: u8) -> Result<(), Error<I2C::Error>> {
        if frame > 8 {
            return Err(Error::InvalidFrame(frame));
        }
        self.write_register(addresses::CONFIG_BANK, config_registers::FRAME, frame)?;
        Ok(())
    }

    pub fn select_mode(&mut self, mode: u8) -> Result<(), Error<I2C::Error>> {
        self.write_register(addresses::CONFIG_BANK, config_registers::MODE, mode)?;
        Ok(())
    }

    pub fn write_register(&mut self, bank: u8, register: u8, value: u8) -> Result<(), Error<I2C::Error>> {
        self.select_page(bank)?;
        self.i2c.write(self.address, &[register, value])?;
        Ok(())
    }

    pub fn select_page(&mut self, bank: u8) -> Result<(), Error<I2C::Error>> {
        self.i2c.write(self.address, &[addresses::BANK_ADDRESS, bank])?;
        Ok(())
    }

    pub fn shutdown(&mut self, shutdown: bool) -> Result<(), Error<I2C::Error>> {
        self.select_page(addresses::CONFIG_BANK)?;
        let value = if shutdown { 0x00 } else { 0xff };
        self.i2c.write(self.address, &[config_registers::SHUTDOWN, value])?;
        Ok(())
    }

    pub fn fill(&mut self, shade: u8) -> Result<(), Error<I2C::Error>> {
        let color = [shade; 144];
        self.set_color(0, &color)?;

        let onoff_one: u8 = if shade > 0 { 0xFF } else { 0x00 };
        self.set_onoff(0, &[onoff_one; 18])?;

        Ok(())
    }

    pub fn clear_color(&mut self) -> Result<(), Error<I2C::Error>> {
        self.set_color(0, &[0x00; 144])
    }

    pub fn set_color(&mut self, page: u8, color: &[u8; 144]) -> Result<(), Error<I2C::Error>> {
        self.select_page(page)?;
        let mut buf = [0u8; 145];
        buf[0] = addresses::COLOR_OFFSET;
        buf[1..].copy_from_slice(color);
        self.i2c.write(self.address, &buf)?;
        Ok(())
    }

    pub fn set_color_byte(&mut self, index: u8, value: u8) -> Result<(), Error<I2C::Error>> {
        self.i2c
            .write(self.address, &[addresses::COLOR_OFFSET + index, value])?;
        Ok(())
    }

    pub fn set_onoff(&mut self, page: u8, onoff: &[u8; 18]) -> Result<(), Error<I2C::Error>> {
        self.select_page(page)?;
        let mut buf = [0u8; 19];
        buf[0] = addresses::ENABLE_OFFSET;
        buf[1..].copy_from_slice(onoff);
        self.i2c.write(self.address, &buf)?;
        Ok(())
    }

    pub fn set_onoff_byte(&mut self, index: u8, value: u8) -> Result<(), Error<I2C::Error>> {
        self.i2c
            .write(self.address, &[addresses::ENABLE_OFFSET + index, value])?;
        Ok(())
    }
}

pub mod config_registers {
    pub const MODE: u8 = 0x00;
    pub const FRAME: u8 = 0x01;
    pub const AUTOPLAY1: u8 = 0x02;
    pub const AUTOPLAY2: u8 = 0x03;
    pub const BLINK: u8 = 0x05;
    pub const AUDIOSYNC: u8 = 0x06;
    pub const FRAME_STATE: u8 = 0x07;
    pub const BREATH1: u8 = 0x08;
    pub const BREATH2: u8 = 0x09;
    pub const SHUTDOWN: u8 = 0x0A;
    pub const AGC_CONTROL: u8 = 0x0B;
    pub const ADC_RATE: u8 = 0x0C;
}

pub mod modes {
    pub const PICTURE_MODE: u8 = 0x00;
    pub const AUTOPLAY_MODE: u8 = 0x08;
    pub const AUDIOPLAY_MODE: u8 = 0x18;
}

pub mod addresses {
    pub const CONFIG_BANK: u8 = 0x0B;
    pub const BANK_ADDRESS: u8 = 0xFD;

    pub const ENABLE_OFFSET: u8 = 0x00;
    pub const BLINK_OFFSET: u8 = 0x12;
    pub const COLOR_OFFSET: u8 = 0x24;
}

#[derive(Clone, Copy, Debug)]
pub enum Error<I2cError> {
    I2cError(I2cError),
    InvalidLocation(u8),
    InvalidFrame(u8),
}

impl<E> From<E> for Error<E> {
    fn from(error: E) -> Self {
        Error::I2cError(error)
    }
}
