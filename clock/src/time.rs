use defmt::{write, Format, Formatter};

#[derive(Clone)]
pub struct DateTime(pcf8563::DateTime);

impl DateTime {
    pub fn normalise(&mut self) {
        let time = &mut self.0;
        time.minutes += time.seconds / 60;
        time.seconds %= 60;
        time.hours += time.minutes / 60;
        time.minutes %= 60;
        time.hours %= 24;

        //TODO calendar?
    }

    pub fn time_as_digits(&self) -> [usize; 6] {
        let time = self.0;
        [
            (time.hours / 10).into(),
            (time.hours % 10).into(),
            (time.minutes / 10).into(),
            (time.minutes % 10).into(),
            (time.seconds / 10).into(),
            (time.seconds % 10).into(),
        ]
    }

    pub fn advance_minutes(&mut self) {
        self.0.minutes += 1;
        self.normalise();
    }

    pub fn advance_hours(&mut self) {
        self.0.hours += 1;
        self.normalise();
    }

    pub fn advance_seconds(&mut self) {
        self.0.seconds += 1;
        self.normalise();
    }

    pub const fn default() -> DateTime {
        let time = pcf8563::DateTime {
            year: 0,
            month: 1,
            weekday: 0,
            day: 1,
            hours: 0,
            minutes: 0,
            seconds: 0,
        };
        DateTime(time)
    }
}

impl From<pcf8563::DateTime> for DateTime {
    fn from(value: pcf8563::DateTime) -> Self {
        DateTime(value)
    }
}

impl Into<pcf8563::DateTime> for DateTime {
    fn into(self) -> pcf8563::DateTime {
        self.0
    }
}

impl Format for DateTime {
    fn format(&self, fmt: Formatter) {
        let date = self.0;
        write!(
            fmt,
            "{}.{}.{} {}:{}:{}, weekday: {}",
            date.year, date.month, date.day, date.hours, date.minutes, date.seconds, date.weekday
        )
    }
}

impl Default for DateTime {
    fn default() -> Self {
        DateTime::default()
    }
}
