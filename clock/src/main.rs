#![no_std]
#![no_main]

mod state;
mod time;

#[cfg(feature = "display-test")]
mod display_test;
mod flash;

#[cfg(feature = "display-test")]
use crate::display_test::display_test;

use crate::flash::RpFlash;
use crate::state::{Button, ButtonAction, ClockState, Dimmed, InputEvent, OutputEvents};
use clock_display_lib::display::{ClockDisplay, BRIGHTNESS_MAP, DISPLAY_ADDRESS_RANGE, DISPLAY_ADDRESS_RANGE_CNT};
use clock_display_lib::is31fl3731::IS31FL3731;
use core::cell::RefCell;
use core::iter::Iterator;
use cortex_m_rt::entry;
use defmt::*;
use embassy_embedded_hal::shared_bus::blocking::i2c::I2cDevice;
use embassy_executor::{Executor, InterruptExecutor};
use embassy_futures::select::select;
use embassy_futures::select::Either;
use embassy_rp::gpio::AnyPin;
use embassy_rp::gpio::Pin;
use embassy_rp::i2c;
use embassy_rp::i2c::{Blocking, I2c};
use embassy_rp::peripherals::I2C0;
use embassy_rp::{gpio, interrupt};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::channel::Channel;
use embassy_sync::pubsub::PubSubChannel;
use embassy_time::{Duration, Ticker, Timer};
use gpio::{Input, Pull};
use heapless::Vec;
use pcf8563::*;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

/// Locking method used across the codebase
type RawMutex = CriticalSectionRawMutex;

type I2c0 = I2c<'static, I2C0, Blocking>;
type I2c0Bus = Mutex<RawMutex, RefCell<I2c0>>;
type I2c0Device<'a> = I2cDevice<'a, RawMutex, I2c0>;

type RtcChip<'a> = PCF8563<I2c0Device<'a>>;
type DisplayChip<'a> = IS31FL3731<I2c0Device<'a>>;

static STATE_INPUT_CHANNEL: Channel<RawMutex, InputEvent, 32> = Channel::new();
static STATE_OUTPUT_CHANNEL: PubSubChannel<RawMutex, OutputEvents, 32, 32, 32> = PubSubChannel::new();

static EXECUTOR_HIGH: InterruptExecutor = InterruptExecutor::new();
static EXECUTOR_MED: InterruptExecutor = InterruptExecutor::new();
static EXECUTOR_LOW: StaticCell<Executor> = StaticCell::new();

#[interrupt]
unsafe fn SWI_IRQ_1() {
    EXECUTOR_HIGH.on_interrupt()
}

#[interrupt]
unsafe fn SWI_IRQ_0() {
    EXECUTOR_MED.on_interrupt()
}

#[entry]
fn main() -> ! {
    let p = embassy_rp::init(Default::default());

    let mut config = i2c::Config::default();
    config.frequency = 800_000;
    static I2C_BUS: StaticCell<I2c0Bus> = StaticCell::new();
    let i2c = I2c::new_blocking(p.I2C0, p.PIN_29, p.PIN_28, i2c::Config::default());
    let i2c_bus = Mutex::new(RefCell::new(i2c));
    let i2c_bus = I2C_BUS.init(i2c_bus);

    let flash: RpFlash = embassy_rp::flash::Flash::new(p.FLASH, p.DMA_CH0);

    {
        // HIGH priority
        let spawner = EXECUTOR_HIGH.start(interrupt::SWI_IRQ_1);
        unwrap!(spawner.spawn(run_state_machine()));
        unwrap!(spawner.spawn(tick_every_second()));
    }

    #[cfg(feature = "display-test")]
    {
        EXECUTOR_LOW
            .init(Executor::new())
            .run(|spawner| unwrap!(spawner.spawn(display_test(i2c_bus))));
    }

    #[cfg(feature = "clock")]
    {
        {
            // MID priority
            let spawner = EXECUTOR_MED.start(interrupt::SWI_IRQ_0);
            unwrap!(spawner.spawn(button_task(p.PIN_2.degrade(), Button::Adjust)));
            unwrap!(spawner.spawn(button_task(p.PIN_3.degrade(), Button::Select)));
        }

        EXECUTOR_LOW.init(Executor::new()).run(|spawner| {
            // LOW priority
            unwrap!(spawner.spawn(flash_storage_task(flash)));
            unwrap!(spawner.spawn(display_update_task(i2c_bus)));
            unwrap!(spawner.spawn(rtc_task(i2c_bus)));
            unwrap!(spawner.spawn(blink_task()));
            unwrap!(spawner.spawn(init()));
        });
    }
}

#[embassy_executor::task]
async fn init() {
    Timer::after(Duration::from_millis(50)).await;
    STATE_OUTPUT_CHANNEL
        .publisher()
        .unwrap()
        .publish(OutputEvents::TriggerRTCSync)
        .await;
    info!("Init complete");
}

/// This task owns [ClockState] end executes state transitions triggered by [InputEvent]s.
#[embassy_executor::task]
async fn run_state_machine() {
    let mut state = ClockState::new();
    let input_receiver = STATE_INPUT_CHANNEL.receiver();
    let output_sender = STATE_OUTPUT_CHANNEL.publisher().unwrap();

    loop {
        let input = input_receiver.receive().await;

        let (next_state, events) = state.update(input);
        if let Some(next_state) = next_state {
            info!("State change: {:?}", next_state);
        }

        for event in events {
            match event {
                OutputEvents::InvalidTransition(state, event) => {
                    error!("Invalid state transition: {:?} + {:?}", state, event);
                }
                output => {
                    output_sender.publish(output).await;
                }
            }
        }
    }
}

#[embassy_executor::task]
async fn flash_storage_task(mut flash: RpFlash<'static>) {
    let sender = STATE_INPUT_CHANNEL.sender();
    let mut subscriber = unwrap!(STATE_OUTPUT_CHANNEL.subscriber());

    let read = flash::read_with_default(&mut flash, 0);
    sender.send(InputEvent::SetStateFromFlash(read)).await;

    let mut data_to_save = None;

    loop {
        if let Some(data) = data_to_save.as_mut() {
            match select(
                Timer::after(Duration::from_millis(1000)),
                subscriber.next_message_pure(),
            )
            .await
            {
                Either::First(_timer_expired) => {
                    unwrap!(flash::write(&mut flash, 0, &data));
                    data_to_save = None;
                }
                Either::Second(OutputEvents::SaveStateToFlash(data)) => {
                    data_to_save = Some(data);
                }
                Either::Second(_) => continue,
            }
        } else {
            match subscriber.next_message_pure().await {
                OutputEvents::SaveStateToFlash(data) => {
                    data_to_save = Some(data);
                }
                _ => continue,
            }
        }
    }
}

pub fn init_display_modules(i2c_bus: &'static I2c0Bus) -> Vec<DisplayChip<'_>, 4> {
    DISPLAY_ADDRESS_RANGE
        .into_iter()
        .rev() // For compatibility with prev hardware
        .map(|addr| {
            let i2c_dev = I2cDevice::new(i2c_bus);
            IS31FL3731::new(i2c_dev, addr)
        })
        .filter_map(|mut d| d.setup().ok().map(|_| d))
        .collect::<Vec<DisplayChip, DISPLAY_ADDRESS_RANGE_CNT>>()
}

#[embassy_executor::task]
async fn display_update_task(i2c_bus: &'static I2c0Bus) {
    let mut state_receiver = STATE_OUTPUT_CHANNEL.subscriber().unwrap();

    loop {
        let modules = init_display_modules(i2c_bus);

        info!("Initialized {} display modules", modules.len());
        let mut clock = ClockDisplay::new(modules);
        clock.setup().unwrap();

        loop {
            let output = state_receiver.next_message_pure().await; //TODO
            let state = match output {
                OutputEvents::UpdateDisplay(state) => state,
                _ => continue,
            };

            info!("Display update: {}", &state);

            let c = BRIGHTNESS_MAP[state.brightness_step];
            let time = state.time;
            let digits = time.time_as_digits();

            let display_dimmed = state.dimming;
            let dc = 0x02;

            let colors = match display_dimmed {
                Dimmed::None => [c; 6],
                Dimmed::Hours => [dc, dc, c, c, c, c],
                Dimmed::Minutes => [c, c, dc, dc, c, c],
            };

            let r = draw_digits(&digits, &colors, &mut clock);
            if let Err(_) = r {
                error!("I2cError");
                break;
            }
        }
    }
}

#[derive(Format)]
struct I2cError;

fn draw_digits(digits: &[usize; 6], colors: &[u8; 6], clock: &mut ClockDisplay<I2c0Device>) -> Result<(), I2cError> {
    for (i, (digit, color)) in digits.iter().zip(colors).enumerate() {
        clock.draw_symbol(i as u8, *digit, *color).map_err(|_err| I2cError)?;
    }
    Ok(())
}

async fn rtc_init(i2c_bus: &'static I2c0Bus) -> Result<RtcChip, I2cError> {
    let i2c_dev = I2cDevice::new(i2c_bus);
    let mut rtc: RtcChip = PCF8563::new(i2c_dev);
    rtc.rtc_init().map_err(|_err| I2cError)?;
    rtc.control_clkout(Control::Off).map_err(|_err| I2cError)?;
    Ok(rtc)
}

#[embassy_executor::task]
async fn rtc_task(i2c_bus: &'static I2c0Bus) {
    let mut output_receiver = STATE_OUTPUT_CHANNEL.subscriber().unwrap();
    let input_sender = STATE_INPUT_CHANNEL.sender();
    let mut ticker = Ticker::every(Duration::from_secs(60 * 10));

    loop {
        let mut rtc = loop {
            match rtc_init(i2c_bus).await {
                Ok(device) => {
                    info!("RTC init OK");
                    break device;
                }
                Err(err) => {
                    error!("RTC initialization failed: {}. Retrying", err);
                    continue;
                }
            }
        };

        let err = loop {
            let time = match select(ticker.next(), output_receiver.next_message_pure()).await {
                Either::First(_timer) => None,
                Either::Second(OutputEvents::TriggerRTCSync) => None,
                Either::Second(OutputEvents::SetRTC(time)) => Some(time),
                _ => continue,
            };

            match time {
                None => {
                    info!("RTC sync");
                    let synced_time = match rtc.get_datetime() {
                        Ok(time) => time,
                        Err(err) => break err,
                    };
                    input_sender.send(InputEvent::SetTimeFromRTC(synced_time.into())).await;
                }
                Some(time) => {
                    info!("RTC set");
                    rtc.set_datetime(&time.into()).unwrap()
                }
            }
        };

        match err {
            Error::I2C(err) => {
                error!("RTC error: {}, reinitializing", err);
            }
            Error::InvalidInputData => {
                error!("RTC error: InvalidInputData, reinitializing");
            }
        };
    }
}

/// Emits [InputEvent::Blink] events. Restarts the timer on [OutputEvents::StartBlinking] event..
#[embassy_executor::task]
async fn blink_task() {
    let mut event_receiver = STATE_OUTPUT_CHANNEL.subscriber().unwrap();
    let event_sender = STATE_INPUT_CHANNEL.sender();
    let mut ticker = Ticker::every(Duration::from_millis(500));

    loop {
        match select(event_receiver.next_message_pure(), ticker.next()).await {
            Either::First(OutputEvents::StartBlinking) => {
                ticker.reset();
            }
            Either::First(_) => continue, //ignore event
            Either::Second(_) => {}
        }
        event_sender.send(InputEvent::Blink).await;
    }
}

#[embassy_executor::task]
async fn tick_every_second() {
    let event_sender = STATE_INPUT_CHANNEL.sender();
    let mut ticker = Ticker::every(Duration::from_secs(1));

    loop {
        event_sender.send(InputEvent::Tick).await;
        ticker.next().await;
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn button_task(button_pin1: AnyPin, button: Button) {
    let mut button_input = Input::new(button_pin1, Pull::Up);
    let input_sender = STATE_INPUT_CHANNEL.sender();

    loop {
        wait_for_low_debounced(&mut button_input).await;
        match select(button_input.wait_for_high(), Timer::after(Duration::from_secs(3))).await {
            Either::First(_) => {
                input_sender
                    .send(InputEvent::ButtonEvent(button.clone(), ButtonAction::ShortPress))
                    .await;
            }
            Either::Second(_) => {
                input_sender
                    .send(InputEvent::ButtonEvent(button.clone(), ButtonAction::LongPress))
                    .await;
                button_input.wait_for_high().await;
            }
        }
    }
}

async fn wait_for_low_debounced(button: &mut Input<'_>) {
    loop {
        button.wait_for_low().await;
        let b = select(button.wait_for_high(), Timer::after(Duration::from_millis(50))).await;

        if let Either::First(_) = b {
            // info!("Bounce");
            continue;
        } else {
            break;
        }
    }
}
