// the library for the embedded linux device
extern crate linux_embedded_hal as lin_hal;

// the eink library
extern crate eink_waveshare_rs;


use eink_waveshare_rs::{epd4in2::EPD4in2};

use lin_hal::spidev::{self, SpidevOptions};
use lin_hal::{Pin, Spidev};
use lin_hal::sysfs_gpio::Direction;
use lin_hal::Delay;




// DigitalIn Hack as long as it's not in the linux_embedded_hal
// from https://github.com/rudihorn/max31865/blob/extra_examples/examples/rpi.rs
extern crate embedded_hal;
use embedded_hal::digital::{InputPin, OutputPin};

struct HackInputPin<'a> {
    pin: &'a OutputPin
}

impl<'a> HackInputPin<'a> {
    fn new(p : &'a OutputPin) -> HackInputPin {
        HackInputPin {
            pin: p
        }
    }
}

impl<'a> InputPin for HackInputPin<'a> {
    fn is_low(&self) -> bool {
        self.pin.is_low()
    }

    fn is_high(&self) -> bool {
        self.pin.is_high()
    }
}


/*
*
* BE CAREFUL: this wasn't tested yet, and the pins are also not choosen correctly (just some random ones atm)
*
*/

fn main() {

    // Configure SPI
    let mut spi = Spidev::open("/dev/spidev0.0").unwrap();
    let options = SpidevOptions::new()
        .bits_per_word(8)
        .max_speed_hz(1_000_000)
        .mode(spidev::SPI_MODE_0)
        .build();
    spi.configure(&options).unwrap();

    // Configure Digital I/O Pin to be used as Chip Select for SPI
    let cs = Pin::new(23);
    cs.export().unwrap();
    while !cs.is_exported() {}
    cs.set_direction(Direction::Out).unwrap();
    cs.set_value(1).unwrap();

    let busy = Pin::new(26);
    busy.export().unwrap();
    while !busy.is_exported() {}
    busy.set_direction(Direction::In).unwrap();
    busy.set_value(1).unwrap();
    let busy_in = HackInputPin::new(&busy);

    let dc = Pin::new(27);
    dc.export().unwrap();
    while !dc.is_exported() {}
    dc.set_direction(Direction::Out).unwrap();
    dc.set_value(1).unwrap();

    let rst = Pin::new(28);
    rst.export().unwrap();
    while !rst.is_exported() {}
    rst.set_direction(Direction::Out).unwrap();
    rst.set_value(1).unwrap();   

    let delay = Delay {};


 
    

    //TODO: wait for Digital::InputPin
    //fixed currently with the HackInputPin, see further above
    let mut epd4in2 = EPD4in2::new(spi, cs, busy_in, dc, rst, delay).unwrap();

    //let mut buffer =  [0u8, epd4in2.get_width() / 8 * epd4in2.get_height()];
    let mut buffer = [0u8; 15000];
    // draw something into the buffer
    buffer[0] = 0xFF;
 
    epd4in2.display_and_transfer_frame(&buffer, None).unwrap();
 
    epd4in2.delay_ms(3000);

    epd4in2.clear_frame(None).unwrap();

    epd4in2.sleep().unwrap();
}