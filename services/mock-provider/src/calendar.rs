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
mod tests {
    use super::*;
    use generated_contracts::{AccountId32, EntityId, WeekOverride};

    #[test]
    fn weekly_defaults_overrides_and_reservations_cross_years() -> Result<()> {
        let start = Week::new(2026, 53)?;
        let middle = start.next()?;
        let end = middle.next()?;
        assert_eq!(middle, Week::new(2027, 1)?);
        let mut calendar = CalendarView {
            calendar_id: EntityId::from_bytes([1; 16]),
            owner: AccountId32::from_bytes([2; 32]),
            definition: CalendarDefinition {
                default_weekly_minutes: Minutes::new(60),
                overrides: vec![
                    WeekOverride {
                        week: start,
                        capacity: Minutes::ZERO,
                    },
                    WeekOverride {
                        week: middle,
                        capacity: Minutes::new(20),
                    },
                ],
            },
            reservations: Vec::new(),
        };
        let window = WeekWindow::new(start, end)?;
        assert_eq!(
            allocation(&calendar, window, Minutes::new(70))?,
            vec![(middle, Minutes::new(20)), (end, Minutes::new(50))]
        );
        reserve(
            &mut calendar,
            window,
            Minutes::new(70),
            &ReservationView {
                project_id: EntityId::from_bytes([3; 16]),
                milestone_id: None,
                requirement_key: None,
                week: start,
                minutes: Minutes::ZERO,
            },
        )?;
        assert_eq!(available(&calendar, end)?, Minutes::new(10));
        assert_eq!(available(&calendar, end.next()?)?, Minutes::new(60));
        assert!(allocation(&calendar, window, Minutes::new(11)).is_err());
        assert_eq!(calendar.reservations.len(), 2);
        calendar.definition.default_weekly_minutes = Minutes::new(49);
        assert!(validate(&calendar).is_err());
        Ok(())
    }
}
