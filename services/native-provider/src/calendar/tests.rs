use super::*;
use protocol::{AccountId32, EntityId, WeekOverride};

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

/// The same committed cases are executed against production v5 WASM in the
/// isolated sandbox workspace. Neither suite calls the other implementation.
#[test]
fn canonical_calendar_parity_cases() -> std::result::Result<(), Box<dyn std::error::Error>> {
    use serde_json::{Value, from_value, json};

    let fixture: Value = serde_json::from_str(include_str!("fixtures/parity.json"))?;
    assert_eq!(fixture["schemaVersion"], 1);
    for case in fixture["cases"].as_array().ok_or("missing cases")? {
        let mut calendar: CalendarView = from_value(case["initial"].clone())?;
        validate(&calendar)?;
        for action in case["actions"].as_array().ok_or("missing actions")? {
            let before = calendar.clone();
            let outcome = match action["kind"].as_str().ok_or("missing action kind")? {
                "available" => available(&calendar, from_value(action["week"].clone())?)
                    .map(|minutes| json!(minutes)),
                "allocation" => allocation(
                    &calendar,
                    from_value(action["window"].clone())?,
                    from_value(action["minutes"].clone())?,
                )
                .map(|slots| json!(slots)),
                "reserve" => reserve(
                    &mut calendar,
                    from_value(action["window"].clone())?,
                    from_value(action["minutes"].clone())?,
                    &ReservationView {
                        project_id: EntityId::from_bytes([10; 16]),
                        milestone_id: Some(EntityId::from_bytes([11; 16])),
                        requirement_key: Some(7),
                        week: Week::new(2026, 1)?,
                        minutes: Minutes::ZERO,
                    },
                )
                .map(|()| json!(calendar.reservations)),
                "setCalendar" => {
                    let mut candidate = calendar.clone();
                    candidate.definition = from_value(action["definition"].clone())?;
                    validate(&candidate).map(|()| {
                        calendar = candidate;
                        json!(calendar.reservations)
                    })
                }
                _ => return Err("unknown calendar parity action".into()),
            };
            if let Some(error) = action["error"].as_str() {
                assert_eq!(outcome.map_err(|error| error.code()), Err(error), "{case}");
                assert_eq!(calendar, before, "failed operation changed calendar");
            } else {
                assert_eq!(outcome?, action["expected"], "{}: {action}", case["name"]);
            }
        }
    }
    Ok(())
}
