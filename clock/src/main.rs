#![no_std]
#![no_main]

use core::cell::RefCell;

use defmt::*;
use embassy_embedded_hal::shared_bus::blocking::i2c::I2cDevice;
use embassy_executor::Spawner;
use embassy_futures::select::select;
use embassy_futures::select::Either;
use embassy_rp::gpio;
use embassy_rp::gpio::AnyPin;
use embassy_rp::gpio::Pin;
use embassy_rp::i2c::{Blocking, I2c};
use embassy_rp::i2c;
use embassy_rp::peripherals::I2C0;
use embassy_sync::blocking_mutex::NoopMutex;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Ticker, Timer};
use gpio::{Input, Pull};
use pcf8563::*;
use static_cell::{StaticCell};
use {defmt_rtt as _, panic_probe as _};
use clock_logic::display::ClockDisplay;
use clock_logic::is31fl3731::IS31FL3731;

type I2c0 = I2c<'static, I2C0, Blocking>;
type I2cBus = NoopMutex<RefCell<I2c0>>;
type StaticClockDisplay = ClockDisplay<I2c0>;
type RtcDevice<'a> = PCF8563<I2cDevice<'a, NoopRawMutex, I2c0>>;

static CURRENT_STATE: Mutex<CriticalSectionRawMutex, State> = Mutex::new(State::Idle);
static CURRENT_TIME: Mutex<ThreadModeRawMutex, DateTime> = Mutex::new(default_datetime());

const MAX_BRIGHTNESS_LEVEL: usize = 6;
const BRIGHTNESS_MAP: [u8; MAX_BRIGHTNESS_LEVEL] = [0x05, 0x10, 0x20, 0x40, 0x60, 0x90];
static CURRENT_BRIGHTNESS: Mutex<ThreadModeRawMutex, usize> = Mutex::new(3); // 0 - 6

static EVENT_CHANNEL: Channel<ThreadModeRawMutex, Event, 10> = Channel::new();
static SCREEN_REFRESH_SIGNAL: Signal<ThreadModeRawMutex, ScreenRefresh> = Signal::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    
    let mut config = i2c::Config::default();
    config.frequency = 400_000;
    static I2C_BUS: StaticCell<NoopMutex<RefCell<I2c<I2C0, Blocking>>>> = StaticCell::new();
    let i2c = I2c::new_blocking(p.I2C0, p.PIN_29, p.PIN_28, i2c::Config::default());
    let i2c_bus = NoopMutex::new(RefCell::new(i2c));
    let i2c_bus = I2C_BUS.init(i2c_bus);
    
    unwrap!(spawner.spawn(rtc_task(i2c_bus)));
    unwrap!(spawner.spawn(run_time()));
    Timer::after(Duration::from_millis(10)).await;
    unwrap!(spawner.spawn(screen_update(i2c_bus)));

    unwrap!(spawner.spawn(button1_task(p.PIN_2.degrade())));
    unwrap!(spawner.spawn(button2_task(p.PIN_3.degrade())));
    unwrap!(spawner.spawn(blink_task()));
    // unwrap!(spawner.spawn(process_events(i2c_bus)));

    info!("Init OK");
}


#[embassy_executor::task]
async fn screen_update(i2c_bus: &'static I2cBus) {
    
    // let leds1 = IS31FL3731::new(shared_i2c.acquire_i2c(), 0x76);
    let i2c_dev2 = I2cDevice::new(i2c_bus);
    let leds2 = IS31FL3731::new(i2c_dev2, 0x74);
    let mut clock = ClockDisplay::new([Some(leds2), None, None]);
    // let mut clock = ClockDisplay::new([Some(leds1), Some(leds2), None]);
    clock.setup().unwrap();
    
    let mut blink: Option<BlinkData> = None;

    loop {
        let time = *CURRENT_TIME.lock().await;
        let brightness_level = *CURRENT_BRIGHTNESS.lock().await;
        let brightness = BRIGHTNESS_MAP[brightness_level];

        info!(
            "Screen refresh: {}:{}:{} {})",
            time.hours, time.minutes, time.seconds, blink
        );

        let digits: [usize; 4] = [
            (time.hours / 10).into(),
            (time.hours % 10).into(),
            (time.minutes / 10).into(),
            (time.minutes % 10).into(),
        ];

        for (i, digit) in digits.iter().enumerate() {
            let mut color = brightness;

            if let Some(blink) = &blink {
                match (blink.frame, blink.position, i) {
                    (0, 0, 2..=3) => {
                        color = 0x02;
                    }
                    (0, 1, 0..=1) => {
                        color = 0x02;
                    }
                    _ => {}
                }
            }

            clock.draw_symbol(i as u8, *digit, color).unwrap();
        }

        let refresh_signal = select(
            Timer::after(Duration::from_millis(20 * 1000)),
            SCREEN_REFRESH_SIGNAL.wait(),
        )
        .await;

        match refresh_signal {
            Either::Second(ScreenRefresh::Blink(blink_data)) => {
                blink.replace(blink_data);
            }
            Either::Second(ScreenRefresh::TimeChanged) => {}
            Either::First(_) | Either::Second(ScreenRefresh::Normal) => blink = None,
        }
    }
}

#[embassy_executor::task]
async fn rtc_task(i2c_bus: &'static I2cBus) {
    let i2c_dev = I2cDevice::new(i2c_bus);
    let mut rtc = PCF8563::new(i2c_dev);
    rtc.rtc_init().unwrap();
    rtc.control_clkout(Control::Off).unwrap();
    info!("RTC init OK");
    
    loop {
        let synced_time = {
            rtc.get_datetime().unwrap()
        };

        {
            let mut time = CURRENT_TIME.lock().await;
            *time = synced_time;
        }

        match select(Timer::after(Duration::from_secs(60 * 10)), EVENT_CHANNEL.receive()).await {
            Either::First(_) => {}
            Either::Second(event) => on_event(event, &mut rtc).await
        };
    }
}

#[embassy_executor::task]
async fn led_test(mut clock: StaticClockDisplay) {
    let mut cnt: u8 = 0;

    loop {
        for driver in clock.drivers.iter_mut().flatten() {
            driver.set_color_byte(cnt, 0x36).unwrap();
            if cnt > 0 {
                driver.set_color_byte(cnt - 1, 0x00).unwrap();
            }
        }

        Timer::after(Duration::from_millis(200)).await;
        cnt = (cnt + 1) % 144;
    }
}

#[embassy_executor::task]
async fn led_numbers_test(mut clock: StaticClockDisplay) {
    let mut cnt: usize = 0;

    loop {
        for i in 0..=4 {
            clock.draw_symbol(i, cnt, 0x70).unwrap();
        }

        Timer::after(Duration::from_millis(200)).await;
        cnt = (cnt + 1) % 10;
    }
}

async fn wait_for_low_debounced(button: &mut Input<'_>) {
    loop {
        button.wait_for_low().await;
        let b = select(
            button.wait_for_high(),
            Timer::after(Duration::from_millis(50)),
        )
        .await;

        if let Either::First(_) = b {
            // info!("Bounce");
            continue;
        } else {
            break;
        }
    }
}

#[derive(Format)]
enum Event {
    SetButton(ButtonPress),
    AdjustButton(ButtonPress),
}

#[derive(Format)]
enum ButtonPress {
    Short,
    Long,
}

async fn on_event(event: Event, rtc: &mut RtcDevice<'_>) {
    let state = { CURRENT_STATE.lock().await.clone() };

    match (event, state) {
        // Enter time setting mode
        (Event::SetButton(ButtonPress::Long), State::Idle) => to_state(State::SettingTime(0)).await,

        // Exit time setting mode
        (Event::SetButton(ButtonPress::Long), State::SettingTime(_)) => to_state(State::Idle).await,

        // Move to next position
        (Event::SetButton(ButtonPress::Short), State::SettingTime(digit)) => {
            let next_digit = (digit + 1) % 3; // hours, minutes, done
            if next_digit == 2 {
                SCREEN_REFRESH_SIGNAL.signal(ScreenRefresh::Normal);
                to_state(State::Idle).await;
            } else {
                to_state(State::SettingTime(next_digit)).await;
            }
        }

        // Advance hours or minutes
        (Event::AdjustButton(ButtonPress::Short), State::SettingTime(digit)) => {
            advance_time(digit, rtc).await;
        }

        // Adjust brightness
        (Event::AdjustButton(ButtonPress::Short), State::Idle) => adjust_brightness().await,

        (_, _) => {}
    }
}

async fn advance_time(position: u8, rtc: &mut RtcDevice<'_>) {
    // let mut rtc = rtc.lock().await;
    let mut current = rtc.get_datetime().unwrap();
    let defaut = default_datetime();

    current.month = defaut.month;
    current.weekday = defaut.weekday;
    current.year = defaut.year;

    info!("Advancing time");

    if position == 0 {
        let minutes = (current.minutes + 1) % 60;
        current.minutes = minutes;
        current.seconds = 0;
    } else if position == 1 {
        let hours = (current.hours + 1) % 24;
        current.hours = hours;
    }

    rtc.set_datetime(&current).unwrap();
    *CURRENT_TIME.lock().await = current;
    SCREEN_REFRESH_SIGNAL.signal(ScreenRefresh::TimeChanged);
}

async fn adjust_brightness() {
    let mut brightness = CURRENT_BRIGHTNESS.lock().await;
    *brightness = (*brightness + 1) % 6;
    SCREEN_REFRESH_SIGNAL.signal(ScreenRefresh::Normal);
}

async fn to_state(new_state: State) {
    let state = { CURRENT_STATE.lock().await.clone() };
    info!("State change: {} -> {}", state, new_state);

    let mut state = CURRENT_STATE.lock().await;
    *state = new_state;
}

#[embassy_executor::task]
async fn blink_task() {
    let mut blink_frame: u8 = 0;

    loop {
        let state = { CURRENT_STATE.lock().await.clone() };
        if let State::SettingTime(digit) = state {
            SCREEN_REFRESH_SIGNAL.signal(ScreenRefresh::Blink(BlinkData {
                position: digit as usize,
                frame: blink_frame,
            }));
            blink_frame = (blink_frame + 1) % 2;
        } else {
            blink_frame = 0;
        }
        Timer::after(Duration::from_millis(500)).await;
    }
}

#[embassy_executor::task]
async fn button1_task(button_pin1: AnyPin) {
    let mut button = Input::new(button_pin1, Pull::Up);

    loop {
        wait_for_low_debounced(&mut button).await;
        // info!("Button1 pressed!");

        let a = select(button.wait_for_high(), Timer::after(Duration::from_secs(3))).await;

        match a {
            Either::First(_) => {
                send_event(Event::SetButton(ButtonPress::Short)).await;
            }
            Either::Second(_) => {
                send_event(Event::SetButton(ButtonPress::Long)).await;
                button.wait_for_high().await;
            }
        }
    }
}

#[embassy_executor::task]
async fn button2_task(button_pin: AnyPin) {
    let mut button = Input::new(button_pin, Pull::Up);

    loop {
        wait_for_low_debounced(&mut button).await;
        // info!("Button2 pressed!");

        let a = select(button.wait_for_high(), Timer::after(Duration::from_secs(3))).await;

        match a {
            Either::First(_) => {
                send_event(Event::AdjustButton(ButtonPress::Short)).await;
            }
            Either::Second(_) => {
                send_event(Event::AdjustButton(ButtonPress::Long)).await;
                button.wait_for_high().await;
            }
        }
    }
}



#[embassy_executor::task]
async fn run_time() {
    let mut ticker = Ticker::every(Duration::from_secs(1));
    loop {
        {
            let mut time = CURRENT_TIME.lock().await;
            time.seconds += 1;

            if time.seconds >= 60 {
                time.seconds = 0;
                time.minutes += 1;
            }

            if time.minutes >= 60 {
                time.minutes = 0;
                time.hours += 1;
            }

            if time.hours >= 24 {
                time.hours = 0;
                time.day += 1;
            }

            // ignoring calendar for now
        }

        ticker.next().await;
    }
}

async fn send_event(event: Event) {
    EVENT_CHANNEL.send(event).await;
}

#[derive(Clone, Format)]
enum State {
    Idle,
    SettingTime(u8), //position: minutes, hours
}

#[derive(Clone, Format)]
enum ScreenRefresh {
    TimeChanged,
    Blink(BlinkData),
    Normal,
}

#[derive(Clone, Format)]
struct BlinkData {
    position: usize,
    frame: u8,
}

const fn default_datetime() -> DateTime {
    DateTime {
        year: 0,
        month: 1,
        weekday: 0,
        day: 1,
        hours: 0,
        minutes: 0,
        seconds: 0,
    }
}

