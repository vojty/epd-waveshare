//! A simple Driver for the Waveshare 4.2" E-Ink Display via SPI
//!
//!
//! Build with the help of documentation/code from [Waveshare](https://www.waveshare.com/wiki/4.2inch_e-Paper_Module),
//! [Ben Krasnows partial Refresh tips](https://benkrasnow.blogspot.de/2017/10/fast-partial-refresh-on-42-e-paper.html) and
//! the driver documents in the `pdfs`-folder as orientation.
//!
//! # Examples
//!
//!```rust, no_run
//!# use embedded_hal_mock::*;
//!# fn main() -> Result<(), MockError> {
//!use embedded_graphics::{
//!    prelude::*, primitives::{Line, PrimitiveStyle},
//!};
//!use epd_waveshare::{epd4in2bc::*, prelude::*};
//!#
//!# let expectations = [];
//!# let mut spi = spi::Mock::new(&expectations);
//!# let expectations = [];
//!# let cs_pin = pin::Mock::new(&expectations);
//!# let busy_in = pin::Mock::new(&expectations);
//!# let dc = pin::Mock::new(&expectations);
//!# let rst = pin::Mock::new(&expectations);
//!# let mut delay = delay::MockNoop::new();
//!
//!// Setup EPD
//!let mut epd = Epd4in2bc::new(&mut spi, cs_pin, busy_in, dc, rst, &mut delay, None)?;
//!
//!// Use display graphics from embedded-graphics
//!let mut display = Display4in2bc::default();
//!
//!// Use embedded graphics for drawing a line
//!let _ = Line::new(Point::new(0, 120), Point::new(0, 295))
//!    .into_styled(PrimitiveStyle::with_stroke(TriColor::Chromatic, 1))
//!    .draw(&mut display);
//!
//!    // Display updated frame
//!epd.update_frame(&mut spi, &display.buffer(), &mut delay)?;
//!epd.display_frame(&mut spi, &mut delay)?;
//!
//!// Set the EPD to sleep
//!epd.sleep(&mut spi, &mut delay)?;
//!# Ok(())
//!# }
//!```
//!
//!
//!
//! BE CAREFUL! The screen can get ghosting/burn-ins through the Partial Fast Update Drawing.

use embedded_hal::{
    blocking::{delay::*, spi::Write},
    digital::v2::*,
};

use crate::interface::DisplayInterface;
use crate::traits::{
    InternalWiAdditions, QuickRefresh, RefreshLut, WaveshareDisplay, WaveshareThreeColorDisplay,
};

use crate::color::TriColor;

//The Lookup Tables for the Display
use crate::epd4in2::command::Command;
use crate::epd4in2::constants::*;

use crate::epd4in2::HEIGHT;
use crate::epd4in2::WIDTH;
/// Default Background Color
pub const DEFAULT_BACKGROUND_COLOR: TriColor = TriColor::White;
const IS_BUSY_LOW: bool = true;

use crate::buffer_len;

/// Full size buffer for use with the 4in2 EPD
#[cfg(feature = "graphics")]
pub type Display4in2bc = crate::graphics::Display<
    WIDTH,
    HEIGHT,
    { crate::graphics::DisplayMode::BwrBitOnColorInverted as u8 },
    { buffer_len(WIDTH as usize, HEIGHT as usize * 2) },
    TriColor,
>;

/// Epd4in2bc driver
///
pub struct Epd4in2bc<SPI, CS, BUSY, DC, RST, DELAY> {
    /// Connection Interface
    interface: DisplayInterface<SPI, CS, BUSY, DC, RST, DELAY>,
    /// Background Color
    color: TriColor,
    /// Refresh LUT
    refresh: RefreshLut,
}

impl<SPI, CS, BUSY, DC, RST, DELAY> InternalWiAdditions<SPI, CS, BUSY, DC, RST, DELAY>
    for Epd4in2bc<SPI, CS, BUSY, DC, RST, DELAY>
where
    SPI: Write<u8>,
    CS: OutputPin,
    BUSY: InputPin,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayUs<u32>,
{
    fn init(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        // reset the device
        self.interface.reset(delay, 10_000, 10_000);

        // set the power settings
        self.interface.cmd_with_data(
            spi,
            Command::PowerSetting,
            &[0x03, 0x00, 0x2b, 0x2b, 0xff],
        )?;

        // start the booster
        self.interface
            .cmd_with_data(spi, Command::BoosterSoftStart, &[0x17, 0x17, 0x17])?;

        // set the panel settings
        self.cmd_with_data(spi, Command::PanelSetting, &[0x0F])?;

        // // Set Frequency, 200 Hz didn't work on my board
        // // 150Hz and 171Hz wasn't tested yet
        // // TODO: Test these other frequencies
        // // 3A 100HZ   29 150Hz 39 200HZ  31 171HZ DEFAULT: 3c 50Hz
        self.cmd_with_data(spi, Command::PllControl, &[0x3C])?;

        self.send_resolution(spi)?;

        self.interface
            .cmd_with_data(spi, Command::VcmDcSetting, &[0x12])?;

        self.interface
            .cmd_with_data(spi, Command::VcomAndDataIntervalSetting, &[0x7f])?;

        self.set_lut(spi, delay, None)?;

        // power on
        self.command(spi, Command::PowerOn)?;
        delay.delay_us(5000);

        self.wait_until_idle(spi, delay)?;
        Ok(())
    }
}

impl<SPI, CS, BUSY, DC, RST, DELAY> WaveshareThreeColorDisplay<SPI, CS, BUSY, DC, RST, DELAY>
    for Epd4in2bc<SPI, CS, BUSY, DC, RST, DELAY>
where
    SPI: Write<u8>,
    CS: OutputPin,
    BUSY: InputPin,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayUs<u32>,
{
    fn update_color_frame(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        black: &[u8],
        chromatic: &[u8],
    ) -> Result<(), SPI::Error> {
        self.update_achromatic_frame(spi, delay, black)?;
        self.update_chromatic_frame(spi, delay, chromatic)
    }

    /// Update only the black/white data of the display.
    ///
    /// Finish by calling `update_chromatic_frame`.
    fn update_achromatic_frame(
        &mut self,
        spi: &mut SPI,
        _delay: &mut DELAY,
        black: &[u8],
    ) -> Result<(), SPI::Error> {
        self.interface.cmd(spi, Command::DataStartTransmission1)?;
        self.interface.data(spi, black)?;
        Ok(())
    }

    /// Update only chromatic data of the display.
    ///
    /// This data takes precedence over the black/white data.
    fn update_chromatic_frame(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        chromatic: &[u8],
    ) -> Result<(), SPI::Error> {
        self.interface.cmd(spi, Command::DataStartTransmission2)?;
        self.interface.data(spi, chromatic)?;

        self.wait_until_idle(spi, delay)?;
        Ok(())
    }
}

impl<SPI, CS, BUSY, DC, RST, DELAY> WaveshareDisplay<SPI, CS, BUSY, DC, RST, DELAY>
    for Epd4in2bc<SPI, CS, BUSY, DC, RST, DELAY>
where
    SPI: Write<u8>,
    CS: OutputPin,
    BUSY: InputPin,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayUs<u32>,
{
    type DisplayColor = TriColor;

    fn new(
        spi: &mut SPI,
        cs: CS,
        busy: BUSY,
        dc: DC,
        rst: RST,
        delay: &mut DELAY,
        delay_us: Option<u32>,
    ) -> Result<Self, SPI::Error> {
        let interface = DisplayInterface::new(cs, busy, dc, rst, delay_us);

        let mut epd = Epd4in2bc {
            interface,
            color: DEFAULT_BACKGROUND_COLOR,
            refresh: RefreshLut::Quick,
        };

        epd.init(spi, delay)?;

        Ok(epd)
    }

    fn sleep(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;
        self.interface
            .cmd_with_data(spi, Command::VcomAndDataIntervalSetting, &[0x7f])?; //border floating
        self.command(spi, Command::VcmDcSetting)?; // VCOM to 0V
        self.command(spi, Command::PanelSetting)?;

        self.command(spi, Command::PowerSetting)?; //VG&VS to 0V fast
        for _ in 0..4 {
            self.send_data(spi, &[0x00])?;
        }

        self.command(spi, Command::PowerOff)?;
        self.wait_until_idle(spi, delay)?;
        self.interface
            .cmd_with_data(spi, Command::DeepSleep, &[0xA5])?;
        Ok(())
    }

    fn wake_up(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.init(spi, delay)
    }

    fn set_background_color(&mut self, color: TriColor) {
        self.color = color;
    }

    fn background_color(&self) -> &TriColor {
        &self.color
    }

    fn width(&self) -> u32 {
        WIDTH
    }

    fn height(&self) -> u32 {
        HEIGHT
    }

    fn update_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
        delay: &mut DELAY,
    ) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;

        self.interface.cmd(spi, Command::DataStartTransmission1)?;
        self.interface.data(spi, buffer)?;

        let color_value = self.color.get_byte_value();

        self.interface
            .data_x_times(spi, color_value, WIDTH / 8 * HEIGHT)?;

        self.interface
            .cmd_with_data(spi, Command::DataStartTransmission2, buffer)?;

        self.command(spi, Command::DataStop)?;

        Ok(())
    }

    fn update_partial_frame(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        buffer: &[u8],
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;
        if buffer.len() as u32 != width / 8 * height {
            //TODO: panic!! or sth like that
            // return Err("Wrong buffersize");
        }

        self.command(spi, Command::PartialIn)?;
        self.command(spi, Command::PartialWindow)?;

        self.send_data(spi, &[(x >> 8) as u8])?;
        self.send_data(spi, &[(x & 0xf8) as u8])?; // x should be the multiple of 8, the last 3 bit will always be ignored
        self.send_data(spi, &[(((x & 0xf8) + width - 1) >> 8) as u8])?;
        self.send_data(spi, &[(((x & 0xf8) + width - 1) | 0x07) as u8])?;

        self.send_data(spi, &[(y >> 8) as u8])?;
        self.send_data(spi, &[(y & 0xff) as u8])?;
        self.send_data(spi, &[((y + height - 1) >> 8) as u8])?;
        self.send_data(spi, &[((y + height - 1) & 0xff) as u8])?;

        self.send_data(spi, &[0x01])?; // Gates scan both inside and outside of the partial window. (default)

        delay.delay_us(2000);
        self.command(spi, Command::DataStartTransmission1)?;
        self.send_data(spi, buffer)?;

        let color_value = self.color.get_bit_value();
        self.interface.cmd(spi, Command::DataStartTransmission2)?;
        self.interface
            .data_x_times(spi, color_value, WIDTH / 8 * HEIGHT)?;

        delay.delay_us(2000);
        self.command(spi, Command::PartialOut)?;
        Ok(())
    }

    fn display_frame(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.set_lut(spi, delay, None)?;
        self.command(spi, Command::DisplayRefresh)?;
        delay.delay_us(100_000);
        self.wait_until_idle(spi, delay)?;
        Ok(())
    }

    fn update_and_display_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
        delay: &mut DELAY,
    ) -> Result<(), SPI::Error> {
        self.update_frame(spi, buffer, delay)?;
        self.command(spi, Command::DisplayRefresh)?;
        Ok(())
    }

    fn clear_frame(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;
        self.send_resolution(spi)?;

        let color_value = self.color.get_bit_value();

        self.interface.cmd(spi, Command::DataStartTransmission1)?;
        self.interface
            .data_x_times(spi, color_value, WIDTH / 8 * HEIGHT)?;

        self.interface.cmd(spi, Command::DataStartTransmission2)?;
        self.interface
            .data_x_times(spi, color_value, WIDTH / 8 * HEIGHT)?;
        Ok(())
    }

    fn set_lut(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        refresh_rate: Option<RefreshLut>,
    ) -> Result<(), SPI::Error> {
        if let Some(refresh_lut) = refresh_rate {
            self.refresh = refresh_lut;
        }
        match self.refresh {
            RefreshLut::Full => {
                self.set_lut_helper(spi, delay, &LUT_VCOM0, &LUT_WW, &LUT_BW, &LUT_WB, &LUT_BB)
            }
            RefreshLut::Quick => self.set_lut_helper(
                spi,
                delay,
                &LUT_VCOM0_QUICK,
                &LUT_WW_QUICK,
                &LUT_BW_QUICK,
                &LUT_WB_QUICK,
                &LUT_BB_QUICK,
            ),
        }
    }

    fn wait_until_idle(&mut self, _spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.interface.wait_until_idle(delay, IS_BUSY_LOW);
        Ok(())
    }
}

impl<SPI, CS, BUSY, DC, RST, DELAY> Epd4in2bc<SPI, CS, BUSY, DC, RST, DELAY>
where
    SPI: Write<u8>,
    CS: OutputPin,
    BUSY: InputPin,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayUs<u32>,
{
    fn command(&mut self, spi: &mut SPI, command: Command) -> Result<(), SPI::Error> {
        self.interface.cmd(spi, command)
    }

    fn send_data(&mut self, spi: &mut SPI, data: &[u8]) -> Result<(), SPI::Error> {
        self.interface.data(spi, data)
    }

    fn cmd_with_data(
        &mut self,
        spi: &mut SPI,
        command: Command,
        data: &[u8],
    ) -> Result<(), SPI::Error> {
        self.interface.cmd_with_data(spi, command, data)
    }

    fn send_resolution(&mut self, spi: &mut SPI) -> Result<(), SPI::Error> {
        let w = self.width();
        let h = self.height();

        self.command(spi, Command::ResolutionSetting)?;
        self.send_data(spi, &[(w >> 8) as u8])?;
        self.send_data(spi, &[w as u8])?;
        self.send_data(spi, &[(h >> 8) as u8])?;
        self.send_data(spi, &[h as u8])
    }

    #[allow(clippy::too_many_arguments)]
    fn set_lut_helper(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        lut_vcom: &[u8],
        lut_ww: &[u8],
        lut_bw: &[u8],
        lut_wb: &[u8],
        lut_bb: &[u8],
    ) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;
        // LUT VCOM
        self.cmd_with_data(spi, Command::LutForVcom, lut_vcom)?;

        // LUT WHITE to WHITE
        self.cmd_with_data(spi, Command::LutWhiteToWhite, lut_ww)?;

        // LUT BLACK to WHITE
        self.cmd_with_data(spi, Command::LutBlackToWhite, lut_bw)?;

        // LUT WHITE to BLACK
        self.cmd_with_data(spi, Command::LutWhiteToBlack, lut_wb)?;

        // LUT BLACK to BLACK
        self.cmd_with_data(spi, Command::LutBlackToBlack, lut_bb)?;
        Ok(())
    }

    /// Helper function. Sets up the display to send pixel data to a custom
    /// starting point.
    pub fn shift_display(
        &mut self,
        spi: &mut SPI,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), SPI::Error> {
        self.send_data(spi, &[(x >> 8) as u8])?;
        let tmp = x & 0xf8;
        self.send_data(spi, &[tmp as u8])?; // x should be the multiple of 8, the last 3 bit will always be ignored
        let tmp = tmp + width - 1;
        self.send_data(spi, &[(tmp >> 8) as u8])?;
        self.send_data(spi, &[(tmp | 0x07) as u8])?;

        self.send_data(spi, &[(y >> 8) as u8])?;
        self.send_data(spi, &[y as u8])?;

        self.send_data(spi, &[((y + height - 1) >> 8) as u8])?;
        self.send_data(spi, &[(y + height - 1) as u8])?;

        self.send_data(spi, &[0x01])?; // Gates scan both inside and outside of the partial window. (default)

        Ok(())
    }
}

impl<SPI, CS, BUSY, DC, RST, DELAY> QuickRefresh<SPI, CS, BUSY, DC, RST, DELAY>
    for Epd4in2bc<SPI, CS, BUSY, DC, RST, DELAY>
where
    SPI: Write<u8>,
    CS: OutputPin,
    BUSY: InputPin,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayUs<u32>,
{
    /// To be followed immediately after by `update_old_frame`.
    fn update_old_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
        delay: &mut DELAY,
    ) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;

        self.interface.cmd(spi, Command::DataStartTransmission1)?;

        self.interface.data(spi, buffer)?;

        Ok(())
    }

    /// To be used immediately after `update_old_frame`.
    fn update_new_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
        delay: &mut DELAY,
    ) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;
        self.send_resolution(spi)?;

        self.interface.cmd(spi, Command::DataStartTransmission2)?;

        self.interface.data(spi, buffer)?;

        Ok(())
    }

    /// This is a wrapper around `display_frame` for using this device as a true
    /// `QuickRefresh` device.
    fn display_new_frame(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.display_frame(spi, delay)
    }

    /// This is wrapper around `update_new_frame` and `display_frame` for using
    /// this device as a true `QuickRefresh` device.
    ///
    /// To be used immediately after `update_old_frame`.
    fn update_and_display_new_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
        delay: &mut DELAY,
    ) -> Result<(), SPI::Error> {
        self.update_new_frame(spi, buffer, delay)?;
        self.display_frame(spi, delay)
    }

    fn update_partial_old_frame(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        buffer: &[u8],
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;

        if buffer.len() as u32 != width / 8 * height {
            //TODO: panic!! or sth like that
            //return Err("Wrong buffersize");
        }

        self.interface.cmd(spi, Command::PartialIn)?;
        self.interface.cmd(spi, Command::PartialWindow)?;

        self.shift_display(spi, x, y, width, height)?;

        self.interface.cmd(spi, Command::DataStartTransmission1)?;

        self.interface.data(spi, buffer)?;

        Ok(())
    }

    /// Always call `update_partial_old_frame` before this, with buffer-updating code
    /// between the calls.
    fn update_partial_new_frame(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        buffer: &[u8],
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;
        if buffer.len() as u32 != width / 8 * height {
            //TODO: panic!! or sth like that
            //return Err("Wrong buffersize");
        }

        self.shift_display(spi, x, y, width, height)?;

        self.interface.cmd(spi, Command::DataStartTransmission2)?;

        self.interface.data(spi, buffer)?;

        self.interface.cmd(spi, Command::PartialOut)?;
        Ok(())
    }

    fn clear_partial_frame(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;
        self.send_resolution(spi)?;

        let color_value = self.color.get_byte_value();

        self.interface.cmd(spi, Command::PartialIn)?;
        self.interface.cmd(spi, Command::PartialWindow)?;

        self.shift_display(spi, x, y, width, height)?;

        self.interface.cmd(spi, Command::DataStartTransmission1)?;
        self.interface
            .data_x_times(spi, color_value, width / 8 * height)?;

        self.interface.cmd(spi, Command::DataStartTransmission2)?;
        self.interface
            .data_x_times(spi, color_value, width / 8 * height)?;

        self.interface.cmd(spi, Command::PartialOut)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epd_size() {
        assert_eq!(WIDTH, 400);
        assert_eq!(HEIGHT, 300);
        assert_eq!(DEFAULT_BACKGROUND_COLOR, TriColor::White);
    }
}
