extern crate byteorder;
extern crate embedded_hal as hal;
extern crate linux_embedded_hal;

use std::{thread, time};

use byteorder::{BigEndian, ByteOrder};
use hal::blocking::i2c;
use linux_embedded_hal::I2cdev;

use crate::common;

const NAMES: [&str; 3] = ["voltage", "current", "power"];

struct Ina219<I2C> {
    i2c: I2C,
    address: u8,
}

impl<I2C, E> Ina219<I2C>
where
    I2C: i2c::Write<Error = E> + i2c::Read<Error = E>,
{
    fn new(i2c: I2C, address: u8) -> Ina219<I2C> {
        Ina219 { i2c, address }
    }

    pub fn calibrate(&mut self, value: u16) -> Result<(), E> {
        self.i2c
            .write(self.address, &[0x05_u8, (value >> 8) as u8, value as u8])?;
        Ok(())
    }

    fn read(&mut self, register: u8) -> Result<u16, E> {
        let mut buf: [u8; 2] = [0x00; 2];
        self.i2c.write(self.address, &[register])?;
        self.i2c.read(self.address, &mut buf)?;
        Ok(BigEndian::read_u16(&buf))
    }

    fn sleep(&mut self) -> Result<(), E> {
        let mut buf: [u8; 2] = [0x00; 2];
        self.i2c.write(self.address, &[0x00_u8])?;
        self.i2c.read(self.address, &mut buf)?;
        let config = BigEndian::read_u16(&buf);
        let new_config = config & 65528_u16; // 65528 - 0xfff8
        self.i2c.write(
            self.address,
            &[0x00_u8, (new_config >> 8) as u8, new_config as u8],
        )?;
        Ok(())
    }

    fn wake(&mut self) -> Result<(), E> {
        let mut buf: [u8; 2] = [0x00; 2];
        self.i2c.write(self.address, &[0x00_u8])?;
        self.i2c.read(self.address, &mut buf)?;
        let config = BigEndian::read_u16(&buf);
        let new_config = config & 7_u16; // 0x0007
        self.i2c.write(
            self.address,
            &[0x00_u8, (new_config >> 8) as u8, new_config as u8],
        )?;
        thread::sleep(time::Duration::from_micros(40));
        Ok(())
    }
}

pub struct PowerSensor {
    name: String,
    dev_bus: String,
    address: u8,
    current_lsb: f64,
}

impl PowerSensor {
    pub(crate) fn new(name: String, dev_bus: String, address: u8, exp_current: f64) -> PowerSensor {
        let current_lsb: f64 = exp_current / 32800.0; // 1.0 == max expected amps.
        PowerSensor {
            name,
            dev_bus,
            address,
            current_lsb,
        }
    }
}

impl common::Sensor for PowerSensor {
    fn get_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for item in NAMES {
            names.push(format!("{}_{}", self.name, item));
        }
        names
    }
    fn measure(&mut self) -> Vec<f64> {
        let device = I2cdev::new(self.dev_bus.clone()).unwrap();
        let mut ina = Ina219::new(device, self.address);
        let calibration = (0.04096_f64 / (self.current_lsb * 0.1)).trunc(); // 0.1 = shunt amps
        ina.calibrate(calibration as u16).unwrap();

        ina.wake().unwrap();
        let voltage: f64 = (ina.read(0x02).unwrap() >> 3) as f64 * 4.0 / 1000.0;
        let current: f64 = ina.read(0x04).unwrap() as f64 * 1000.0 * self.current_lsb;
        let power: f64 = ina.read(0x03).unwrap() as f64 * 20.0 * self.current_lsb * 1000.0;
        ina.sleep().unwrap();
        if power <= 0.0 {
            return vec![0.0; 3];
        }

        vec![voltage, current, power]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Sensor;

    // Tests for success.

    #[test]
    fn test_get_names_for_success() {
        let sensor: PowerSensor = PowerSensor::new("".to_string(), "".to_string(), 0, 0.0);
        sensor.get_names();
    }

    // Tests for failure.

    // Tests for sanity.

    #[test]
    fn test_get_names_for_sanity() {
        let sensor: PowerSensor = PowerSensor::new("foo".to_string(), "".to_string(), 0, 0.0);
        let res: Vec<String> = sensor.get_names();
        assert_eq!(res, vec!["foo_voltage", "foo_current", "foo_power"]);
    }
}
