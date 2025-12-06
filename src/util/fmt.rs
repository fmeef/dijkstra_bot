use humanize_duration::Formatter;
use humanize_duration::*;
pub struct TimeFormat;

unit!(MyYear, " year", " years");
unit!(MyMonth, " month", " months");
unit!(MyDay, " day", " days");
unit!(MyHour, " hour", " hours");
unit!(MyMinute, " minute", " minutes");
unit!(MySecond, " second", " seconds");
unit!(MyMillis, " millisecond", " milliseconds");
unit!(MyMicro, " microsecond", " microseconds");
unit!(MyNano, " nanosecond", " nanoseconds");

impl Formatter for TimeFormat {
    fn format(
        &self,
        f: &mut std::fmt::Formatter<'_>,
        parts: humanize_duration::types::DurationParts,
        truncate: humanize_duration::Truncate,
    ) -> core::fmt::Result {
        self.format_default(f, parts, truncate)
    }

    fn get(&self, truncate: humanize_duration::Truncate) -> Box<dyn humanize_duration::Unit> {
        match truncate {
            Truncate::Nano => Box::new(MyNano),
            Truncate::Micro => Box::new(MyMicro),
            Truncate::Millis => Box::new(MyMillis),
            Truncate::Second => Box::new(MySecond),
            Truncate::Minute => Box::new(MyMinute),
            Truncate::Hour => Box::new(MyHour),
            Truncate::Day => Box::new(MyDay),
            Truncate::Month => Box::new(MyMonth),
            Truncate::Year => Box::new(MyYear),
        }
    }
}
