use bincode::error::{DecodeError, EncodeError};
use bincode::{BorrowDecode, Decode, Encode};
use defmt::{debug, info, Format};
use embassy_rp::flash;
use embassy_rp::flash::{Async, Flash, ERASE_SIZE};
use embassy_rp::peripherals::FLASH;

pub type RpFlash<'a> = Flash<'a, FLASH, Async, FLASH_SIZE>;

const FLASH_SIZE: usize = 2 * 1024 * 1024;
const ADDR_OFFSET: u32 = 0x100000;
const CRC: crc::Crc<u16> = crc::Crc::<u16>::new(&crc::CRC_16_IBM_SDLC);
const BINCODE_CONFIG: bincode::config::Configuration = bincode::config::standard();

#[derive(Decode, Encode, Clone, Format, Default)]
pub struct FlashData {
    pub brightness: usize,
}

#[derive(BorrowDecode, Encode, Clone, Format)]
pub struct FlashDataWrapper<'a> {
    data: &'a [u8],
    crc: u16,
}

pub fn read_with_default(flash: &mut RpFlash, offset: u32) -> FlashData {
    read(flash, offset).unwrap_or_else(|err| {
        info!("Error decoding flash data: {:?}. Substituting default.", err);
        FlashData::default()
    })
}

pub fn read(flash: &mut RpFlash, offset: u32) -> Result<FlashData, Error> {
    let mut read_buf = [0u8; ERASE_SIZE];
    flash.blocking_read(ADDR_OFFSET + offset, &mut read_buf)?;

    let (wrapper, _size): (FlashDataWrapper, usize) = bincode::borrow_decode_from_slice(&read_buf, BINCODE_CONFIG)?;
    let calc_crc = CRC.checksum(&wrapper.data);
    if calc_crc != wrapper.crc {
        return Err(Error::CrcError);
    }
    let data: FlashData = bincode::decode_from_slice(&wrapper.data, BINCODE_CONFIG)?.0;
    Ok(data)
}

pub fn write(flash: &mut RpFlash, offset: u32, data: &FlashData) -> Result<(), Error> {
    let mut buf = [0u8; ERASE_SIZE - 4];
    let s = bincode::encode_into_slice(data, &mut buf, BINCODE_CONFIG)?;
    debug!("Data size: {} bytes", s);

    let wrapper = FlashDataWrapper {
        data: &buf[..s],
        crc: CRC.checksum(&buf[..s]),
    };

    let mut wrapper_buf = [0u8; ERASE_SIZE];
    bincode::encode_into_slice(&wrapper, &mut wrapper_buf, BINCODE_CONFIG)?;

    flash.blocking_erase(ADDR_OFFSET + offset, ADDR_OFFSET + offset + ERASE_SIZE as u32)?;
    flash.blocking_write(ADDR_OFFSET + offset, &wrapper_buf)?;
    Ok(())
}

#[derive(Format)]
pub enum Error {
    DecodeError,
    EncodeError,
    FlashError(flash::Error),
    CrcError,
}

impl From<DecodeError> for Error {
    fn from(_err: DecodeError) -> Error {
        Error::DecodeError
    }
}

impl From<flash::Error> for Error {
    fn from(err: flash::Error) -> Error {
        Error::FlashError(err)
    }
}

impl From<EncodeError> for Error {
    fn from(_err: EncodeError) -> Self {
        Error::EncodeError
    }
}
