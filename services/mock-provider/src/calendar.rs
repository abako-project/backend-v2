use std::collections::BTreeMap;

use chrono::{DateTime, Datelike};
use generated_contracts::{
    CalendarDefinition, CalendarView, Minutes, ReservationView, UnixSeconds, Week, WeekWindow,
};

use crate::{Error, Result, require};

pub(crate) fn current_week(now: UnixSeconds) -> Result<Week> {
    let seconds = i64::try_from(now.get()).map_err(|_| Error::bad("invalid_time"))?;
    let iso = DateTime::from_timestamp(seconds, 0)
        .ok_or_else(|| Error::bad("invalid_time"))?
        .iso_week();
    Ok(Week::new(
        iso.year(),
        u8::try_from(iso.week()).map_err(|_| Error::internal())?,
    )?)
}

fn capacity(definition: &CalendarDefinition, week: Week) -> Minutes {
    definition
        .overrides
        .iter()
        .find(|item| item.week == week)
        .map_or(definition.default_weekly_minutes, |item| item.capacity)
}

fn reservations(calendar: &CalendarView) -> Result<BTreeMap<Week, Minutes>> {
    let mut totals: BTreeMap<Week, Minutes> = BTreeMap::new();
    for reservation in &calendar.reservations {
        let entry = totals.entry(reservation.week).or_default();
        *entry = entry.checked_add(reservation.minutes)?;
    }
    Ok(totals)
}

pub(crate) fn validate(calendar: &CalendarView) -> Result<()> {
    calendar.definition.validate()?;
    for (week, reserved) in reservations(calendar)? {
        require(
            reserved <= capacity(&calendar.definition, week),
            "calendar_overcommitted",
        )?;
    }
    Ok(())
}

pub(crate) fn available(calendar: &CalendarView, week: Week) -> Result<Minutes> {
    capacity(&calendar.definition, week)
        .checked_sub(
            reservations(calendar)?
                .get(&week)
                .copied()
                .unwrap_or_default(),
        )
        .map_err(Into::into)
}

/// Produce a complete allocation or nothing; callers append only after success.
pub(crate) fn allocation(
    calendar: &CalendarView,
    window: WeekWindow,
    minutes: Minutes,
) -> Result<Vec<(Week, Minutes)>> {
    let reserved = reservations(calendar)?;
    let mut remaining = minutes;
    let mut week = window.start();
    let mut result = Vec::new();
    while remaining != Minutes::ZERO {
        let free = capacity(&calendar.definition, week)
            .checked_sub(reserved.get(&week).copied().unwrap_or_default())?;
        let assigned = free.min(remaining);
        if assigned != Minutes::ZERO {
            result.push((week, assigned));
            remaining = remaining.checked_sub(assigned)?;
        }
        if week == window.end() {
            break;
        }
        // An unbounded default is represented once, not materialized at startup.
        week = week.next()?;
    }
    require(remaining == Minutes::ZERO, "insufficient_capacity")?;
    Ok(result)
}

pub(crate) fn reserve(
    calendar: &mut CalendarView,
    window: WeekWindow,
    minutes: Minutes,
    reference: &ReservationView,
) -> Result<()> {
    let slots = allocation(calendar, window, minutes)?;
    calendar
        .reservations
        .extend(slots.into_iter().map(|(week, minutes)| ReservationView {
            week,
            minutes,
            ..reference.clone()
        }));
    Ok(())
}

#[cfg(test)]
mod tests;
