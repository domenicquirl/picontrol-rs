//! # picontrol
//!
//! A library to control the Revolution Pi industrial PLC based on the Raspberry Pi.

#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use nix::libc::c_int;
use nix::Result;
use std::ffi::CStr;
use std::fs::File;
use std::io;
use std::str;

use byteorder::{ByteOrder, LittleEndian};
use nix::errno::Errno;
use nix::errno::Errno::ENODEV;
use std::fs::OpenOptions;
use std::io::prelude::*;
use std::io::ErrorKind;
use std::io::SeekFrom;
use std::io::Write;
use std::iter;
use std::os::unix::io::AsRawFd;

#[allow(dead_code)]
mod ioctl;
mod picontrol;
pub use crate::picontrol::*;

#[derive(Debug)]
pub enum CstrToStrError {
    FromBytesWithNul(std::ffi::FromBytesWithNulError),
    Utf8(std::str::Utf8Error),
}

impl From<std::str::Utf8Error> for CstrToStrError {
    fn from(err: std::str::Utf8Error) -> CstrToStrError {
        CstrToStrError::Utf8(err)
    }
}

impl From<std::ffi::FromBytesWithNulError> for CstrToStrError {
    fn from(err: std::ffi::FromBytesWithNulError) -> CstrToStrError {
        CstrToStrError::FromBytesWithNul(err)
    }
}

fn convert_cstr_to_str(
    cstr: &[::std::os::raw::c_char],
) -> std::result::Result<&str, CstrToStrError> {
    let u8slice = unsafe { &*(cstr as *const _ as *const [u8]) };
    let c_str = CStr::from_bytes_with_nul(u8slice).map_err(CstrToStrError::FromBytesWithNul)?;
    c_str.to_str().map_err(CstrToStrError::Utf8)
}

impl SPIVariable {
    pub fn name(&self) -> std::result::Result<&str, CstrToStrError> {
        convert_cstr_to_str(&self.strVarName[..])
    }
}

/// RevPiControl is an object representing an open file handle to the piControl driver file descriptor.
pub struct RevPiControl {
    path: String,
    handle: Option<File>,
}

impl Default for picontrol::SDeviceInfo {
    fn default() -> picontrol::SDeviceInfo {
        unsafe { std::mem::zeroed() }
    }
}

impl Default for picontrol::SPIVariable {
    fn default() -> picontrol::SPIVariable {
        unsafe { std::mem::zeroed() }
    }
}

impl Default for picontrol::SPIValue {
    fn default() -> picontrol::SPIValue {
        unsafe { std::mem::zeroed() }
    }
}

fn byte_to_int8_array(name: &str) -> [::std::os::raw::c_char; 32] {
    let i8slice = unsafe { &*(name.as_bytes() as *const [u8] as *const [::std::os::raw::c_char]) };
    let mut bname: [::std::os::raw::c_char; 32] = Default::default();
    let (left, _) = bname.split_at_mut(i8slice.len());
    left.copy_from_slice(i8slice);
    bname
}

// numToBytes converts a generic fixed-size value to its byte representation.
pub fn num_to_bytes(
    num: u64,
    size: usize,
) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
    match size {
        8 => Ok(vec![num as u8]),
        16 => {
            let mut buf = [0; 2];
            LittleEndian::write_u16(&mut buf, num as u16);
            Ok(buf.to_vec())
        }
        32 => {
            let mut buf = [0; 4];
            LittleEndian::write_u32(&mut buf, num as u32);
            Ok(buf.to_vec())
        }
        64 => {
            let mut buf = [0; 8];
            LittleEndian::write_u64(&mut buf, num as u64);
            Ok(buf.to_vec())
        }
        _ => Err(From::from(format!("invalid size {}", size))),
    }
}

impl RevPiControl {
    pub fn new() -> Self {
        let c_str = CStr::from_bytes_with_nul(picontrol::PICONTROL_DEVICE).unwrap();
        let path = String::from(c_str.to_str().unwrap());
        RevPiControl { handle: None, path }
    }

    pub fn new_at(path: &str) -> Self {
        RevPiControl {
            handle: None,
            path: path.to_owned(),
        }
    }

    /// Open the Pi Control interface.
    pub fn open(&mut self) -> io::Result<bool> {
        if self.handle.as_mut().is_some() {
            return Ok(true);
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "can not open picontrol file descriptor at {}, error: {}",
                        &self.path, e
                    ),
                )
            })?;
        self.handle = Some(file);
        Ok(true)
    }

    /// Close the Pi Control interface.
    pub fn close(&mut self) {
        let f = self.handle.take();
        std::mem::drop(f);
    }

    /// Reset Pi Control Interface.
    pub fn reset(&self) -> Result<c_int> {
        let f = self.handle.as_ref().ok_or(ENODEV)?;
        unsafe { ioctl::reset(f.as_raw_fd()) }
    }

    // Gets process data from a specific position, reads @length bytes from file.
    // Returns a result containing the bytes read or error.
    pub fn read(&mut self, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        let f = self
            .handle
            .as_mut()
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "error reading file"))?;
        /* seek */
        f.seek(SeekFrom::Start(offset))?;
        let mut v = vec![0u8; length];
        f.read_exact(&mut v)?;
        Ok(v)
    }

    /// Writes process data at a specific position and a returns a boolean result.
    pub fn write(&mut self, offset: u64, data: &[u8]) -> std::io::Result<bool> {
        let f = self
            .handle
            .as_mut()
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "error reading file"))?;
        /* seek */
        f.seek(SeekFrom::Start(offset))?;
        f.write_all(data)?;
        Ok(true)
    }

    /// Get the info for a variable.
    pub fn get_variable_info(&self, name: &str) -> Result<picontrol::SPIVariable> {
        let f = self.handle.as_ref().ok_or(ENODEV)?;
        let mut v = picontrol::SPIVariable {
            strVarName: byte_to_int8_array(name),
            ..Default::default()
        };
        let res = unsafe { ioctl::get_variable_info(f.as_raw_fd(), &mut v) }?;
        if res < 0 {
            return Err(Errno::last());
        }
        Ok(v)
    }

    /// Gets a description of connected devices.
    pub fn get_device_info_list(&self) -> Result<Vec<picontrol::SDeviceInfo>> {
        let f = self.handle.as_ref().ok_or(ENODEV)?;
        // let mut pDev: picontrol::SDeviceInfo = unsafe { mem::uninitialized() };
        let mut pDev = [picontrol::SDeviceInfo {
            ..Default::default()
        }; picontrol::REV_PI_DEV_CNT_MAX as usize];
        let res = unsafe { ioctl::get_device_info_list(f.as_raw_fd(), &mut pDev[0]) }?;
        if res < 0 {
            return Err(Errno::last());
        }
        Ok(pDev[..res as usize].to_vec())
    }

    /// Gets the value of one bit in the process image.
    pub fn get_bit_value(&self, pSpiValue: &mut picontrol::SPIValue) -> Result<bool> {
        self.handle_bit_value(pSpiValue, ioctl::get_bit_value)
    }

    /// Sets the value of one bit in the process image.
    pub fn set_bit_value(&self, pSpiValue: &mut picontrol::SPIValue) -> Result<bool> {
        self.handle_bit_value(pSpiValue, ioctl::set_bit_value)
    }

    fn handle_bit_value(
        &self,
        pSpiValue: &mut picontrol::SPIValue,
        func: unsafe fn(i32, *mut picontrol::SPIValueStr) -> std::result::Result<i32, nix::Error>,
    ) -> Result<bool> {
        let f = self.handle.as_ref().ok_or(ENODEV)?;

        pSpiValue.i16uAddress += (pSpiValue.i8uBit as u16) / 8;
        pSpiValue.i8uBit %= 8;

        let res = unsafe { func(f.as_raw_fd(), pSpiValue) }?;
        if res < 0 {
            return Err(Errno::last());
        }
        Ok(true)
    }

    const SMALL_BUFFER_SIZE: usize = 256;
    const LARGE_BUFFER_SIZE: usize = 64 * 1024;

    /// dumps the process image to a file.
    ///
    /// # Arguments
    ///
    /// * `fp` - The file path
    ///
    pub fn dump(&mut self, fp: &str) -> std::io::Result<bool> {
        let f = self
            .handle
            .as_mut()
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "error reading file"))?;
        /* seek */
        f.seek(SeekFrom::Start(0))?;

        let mut outfile = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(fp)?;
        // f.write(data)?;
        let buffer = &mut vec![0; Self::SMALL_BUFFER_SIZE];

        // We create a buffered writer from the file we get
        // let mut writer = BufWriter::new(&outfile);
        Self::redirect_stream(f, &mut outfile, buffer)?;
        Ok(true)
    }

    fn redirect_stream<R, W>(reader: &mut R, writer: &mut W, buffer: &mut Vec<u8>) -> io::Result<()>
    where
        R: Read,
        W: Write,
    {
        loop {
            let len_read = reader.read(buffer)?;

            if len_read == 0 {
                return Ok(());
            }

            writer.write_all(&buffer[..len_read])?;

            if len_read == buffer.len() && len_read < Self::LARGE_BUFFER_SIZE {
                buffer.extend(iter::repeat(0).take(len_read));
            }
        }
    }
}

impl Default for RevPiControl {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RevPiControl {
    fn drop(&mut self) {
        self.close();
    }
}

// get_module_name returns a friendly name for a RevPi module type.
pub fn get_module_name(moduletype: u32) -> &'static str {
    let moduletype = moduletype & picontrol::PICONTROL_NOT_CONNECTED_MASK;
    match moduletype {
        95 => "RevPi Core",
        96 => "RevPi DIO",
        97 => "RevPi DI",
        98 => "RevPi DO",
        103 => "RevPi AIO",
        picontrol::PICONTROL_SW_MODBUS_TCP_SLAVE => "ModbusTCP Slave Adapter",
        picontrol::PICONTROL_SW_MODBUS_RTU_SLAVE => "ModbusRTU Slave Adapter",
        picontrol::PICONTROL_SW_MODBUS_TCP_MASTER => "ModbusTCP Master Adapter",
        picontrol::PICONTROL_SW_MODBUS_RTU_MASTER => "ModbusRTU Master Adapter",
        100 => "Gateway DMX",
        71 => "Gateway CANopen",
        73 => "Gateway DeviceNet",
        74 => "Gateway EtherCAT",
        75 => "Gateway EtherNet/IP",
        93 => "Gateway ModbusTCP",
        76 => "Gateway Powerlink",
        77 => "Gateway Profibus",
        79 => "Gateway Profinet IRT",
        81 => "Gateway SercosIII",
        _ => "unknown moduletype",
    }
}

// IsModuleConnected checks whether a RevPi module is conneted.
pub fn is_module_connected(moduletype: u32) -> bool {
    moduletype & picontrol::PICONTROL_NOT_CONNECTED > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picontrol_constants() {
        assert_eq!(picontrol::PICONTROL_DEVICE, b"/dev/piControl0\0");
    }
}
