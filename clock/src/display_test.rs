use crate::{init_display_modules, I2c0Bus};
use clock_display_lib::display::ClockDisplay;
use embassy_time::{Duration, Timer};

#[embassy_executor::task]
pub async fn display_test(i2c_bus: &'static I2c0Bus) {
    let modules = init_display_modules(i2c_bus);
    let mut clock = ClockDisplay::new(modules);

    loop {
        for cnt in 0..=9 {
            for i in 0..=4 {
                clock.draw_symbol(i, cnt, 0x70).unwrap();
            }

            Timer::after(Duration::from_millis(1000)).await;
        }

        Timer::after(Duration::from_millis(1000)).await;

        for cnt in 0..144 {
            for driver in clock.drivers.iter_mut() {
                driver.set_color_byte(cnt, 0x00).unwrap();
            }
        }

        Timer::after(Duration::from_millis(1000)).await;

        for cnt in 0..144 {
            for driver in clock.drivers.iter_mut() {
                driver.set_color_byte(cnt, 0x36).unwrap();
                if cnt > 0 {
                    driver.set_color_byte(cnt - 1, 0x00).unwrap();
                }
            }

            Timer::after(Duration::from_millis(200)).await;
        }
    }
}
