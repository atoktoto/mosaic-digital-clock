use crate::time::DateTime;
use clock_display_lib::display::MAX_BRIGHTNESS_LEVEL;
use defmt::{unwrap, Format};
use heapless::Vec;

#[derive(Debug, Clone, Format)]
pub enum State {
    Normal,
    SettingHours,
    SettingMinutes,
}

#[derive(Format, Clone)]
pub enum Button {
    Select,
    Adjust,
}

#[derive(Format, Clone)]
pub enum ButtonAction {
    ShortPress,
    LongPress,
}

#[derive(Format, Clone)]
pub enum InputEvent {
    /// 1Hz pulse for updating local time in-between RTC syncs
    Tick,

    /// Pulse for blinking
    Blink,

    ButtonEvent(Button, ButtonAction),
    SetTimeFromRTC(DateTime),
}

#[derive(Format, Clone)]
pub enum OutputEvents {
    UpdateDisplay(ClockState),
    SetRTC(DateTime),
    InvalidTransition(State, InputEvent),
    StartBlinking,
    TriggerRTCSync,
}

#[derive(Format, Clone)]
pub enum Dimmed {
    None,
    Hours,
    Minutes,
    Both,
}

impl Dimmed {
    pub fn is_dimmed(&self) -> bool {
        match self {
            Dimmed::None => false,
            Dimmed::Hours | Dimmed::Minutes | Dimmed::Both => true
        }
    }
}

#[derive(Clone, Format)]
pub struct ClockState {
    pub state: State,
    pub time: DateTime,
    pub brightness_step: usize,
    pub dimming: Dimmed,
}

impl ClockState {
    pub const fn new() -> Self {
        ClockState {
            state: State::Normal,
            time: DateTime::default(),
            brightness_step: 2,
            dimming: Dimmed::None,
        }
    }

    pub fn update(&mut self, input: InputEvent) -> (Option<State>, Vec<OutputEvents, 10>) {
        let state = self.state.clone();

        let mut events = Vec::new();
        let mut transition = None;

        let mut emit = |event: OutputEvents| {
            unwrap!(events.push(event));
        };

        let mut set = |next_state: State| {
            transition.replace(next_state.clone());
            self.state = next_state;
        };

        match (state, input) {
            // Run the clock in-between RTC syncs
            (_, InputEvent::Tick) => {
                self.time.advance_seconds();
                emit(OutputEvents::UpdateDisplay(self.clone()));
            }

            // Sync time from RTC
            (State::Normal, InputEvent::SetTimeFromRTC(time)) => {
                self.time = time.clone();
            }

            // Brightness
            (State::Normal, InputEvent::ButtonEvent(Button::Adjust, ButtonAction::ShortPress)) => {
                self.brightness_step = (self.brightness_step + 1) % MAX_BRIGHTNESS_LEVEL;
                //TODO save brightness in EEPROM
                emit(OutputEvents::UpdateDisplay(self.clone()));
            }

            // Time setting
            (State::Normal, InputEvent::ButtonEvent(Button::Select, ButtonAction::LongPress)) => {
                self.state = State::SettingHours;
                emit(OutputEvents::StartBlinking);
            }

            // Set hours
            (State::SettingHours, InputEvent::ButtonEvent(Button::Adjust, ButtonAction::ShortPress)) => {
                self.time.advance_hours();
                self.state = State::SettingHours;
                self.dimming = Dimmed::Hours;
                emit(OutputEvents::SetRTC(self.time.clone()));
                emit(OutputEvents::UpdateDisplay(self.clone()));
            }

            // Go to minutes
            (State::SettingHours, InputEvent::ButtonEvent(Button::Select, _)) => self.state = State::SettingMinutes,

            // Set minutes
            (State::SettingMinutes, InputEvent::ButtonEvent(Button::Adjust, ButtonAction::ShortPress)) => {
                self.time.advance_minutes();
                self.state = State::SettingMinutes;
                self.dimming = Dimmed::Minutes;
                emit(OutputEvents::SetRTC(self.time.clone()));
                emit(OutputEvents::UpdateDisplay(self.clone()));
            }

            // Finish time setting
            (State::SettingMinutes, InputEvent::ButtonEvent(Button::Select, _)) => {
                self.dimming = Dimmed::None;
                set(State::Normal);
            }

            // Blink screen when setting time
            (State::SettingHours, InputEvent::Blink) => {
                if self.dimming.is_dimmed() {
                    self.dimming = Dimmed::None;
                } else {
                    self.dimming = Dimmed::Hours;
                }
                emit(OutputEvents::UpdateDisplay(self.clone()));
            }
            (State::SettingMinutes, InputEvent::Blink) => {
                if self.dimming.is_dimmed() {
                    self.dimming = Dimmed::None;
                } else {
                    self.dimming = Dimmed::Minutes;
                }
                emit(OutputEvents::UpdateDisplay(self.clone()));
            }
            (_, InputEvent::Blink) => {}

            (state, input) => {
                emit(OutputEvents::InvalidTransition(state, input.clone()));
            }
        };

        (transition, events)
    }
}
