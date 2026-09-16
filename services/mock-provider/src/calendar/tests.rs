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
