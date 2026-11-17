use super::*;

#[test]
fn identifiers_roundtrip_and_reject_bad_input() -> Result<(), Box<dyn std::error::Error>> {
    let account = AccountId32::from_bytes([42; 32]);
    assert_eq!(
        account,
        serde_json::from_str(&serde_json::to_string(&account)?)?
    );
    assert!("0xff".parse::<AccountId32>().is_err());
    assert!("g".repeat(64).parse::<AccountId32>().is_err());
    assert!(serde_json::from_str::<Sr25519Signature>("\"0xff\"").is_err());
    Ok(())
}

#[test]
fn quantities_validate_json_scale_and_overflow() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        serde_json::to_string(&Money::new(u64::MAX))?,
        format!("\"{}\"", u64::MAX)
    );
    for bad in [
        "1",
        "\"-1\"",
        "\"1.5\"",
        "\"01\"",
        "\"18446744073709551616\"",
    ] {
        assert!(serde_json::from_str::<Money>(bad).is_err());
    }
    assert!(Money::new(u64::MAX).checked_add(Money::new(1)).is_err());
    assert!(Money::ZERO.checked_sub(Money::new(1)).is_err());
    assert!(serde_json::from_str::<Score>("6").is_err());
    assert!(Score::decode(&mut &[0_u8][..]).is_err());
    assert!(Percentage::decode(&mut &[101_u8][..]).is_err());
    assert!(serde_json::from_str::<Minutes>("-1").is_err());
    Ok(())
}

#[test]
fn iso_weeks_and_windows_cannot_be_forged() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(Week::new(2020, 53)?.next()?, Week::new(2021, 1)?);
    assert!(Week::new(2021, 53).is_err());
    assert!(serde_json::from_str::<Week>(r#"{"isoYear":2021,"week":53}"#).is_err());
    assert!(Week::decode(&mut &(2021_i32, 53_u8).encode()[..]).is_err());
    let start = Week::new(2026, 20)?;
    let end = Week::new(2026, 19)?;
    assert!(WeekWindow::new(start, end).is_err());
    assert!(WeekWindow::decode(&mut &(start, end).encode()[..]).is_err());
    assert!(Week::new(-262_143, 1).is_err());
    assert!(Week::new(-262_143, 2).is_ok());
    let last = Week::new(262_143, 1)?;
    assert_eq!(Week::new(262_142, 52)?.next()?, last);
    assert!(last.next().is_err());
    assert_eq!(Week::decode(&mut &last.encode()[..])?, last);
    Ok(())
}

#[test]
fn calendar_dates_keep_the_canonical_wire_format() {
    for valid in ["0001-01-01", "2000-02-29", "2028-02-29", "9999-12-31"] {
        assert!(validate_calendar_date(valid).is_ok(), "{valid}");
    }
    for invalid in [
        "0000-01-01",
        "1900-02-29",
        "2026-04-31",
        "2026-1-01",
        "20260101",
        "2026-W01-1",
        "2026-001",
        "2026-01-01T00:00:00Z",
        " 2026-01-01",
        "2026-01-01 ",
        "+2026-01-01",
        "２０２６-01-01",
    ] {
        assert!(validate_calendar_date(invalid).is_err(), "{invalid}");
    }
}
