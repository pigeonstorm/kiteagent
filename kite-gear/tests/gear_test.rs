use kite_gear::{kite_size, wing_size};

#[test]
fn kite_80kg_matches_original_tables() {
    assert_eq!(kite_size(30.0, 80.0), 5.0);
    assert_eq!(kite_size(28.0, 80.0), 5.0);
    assert_eq!(kite_size(20.0, 80.0), 7.0);
    assert_eq!(kite_size(19.0, 80.0), 7.0);
    assert_eq!(kite_size(16.0, 80.0), 9.0);
    assert_eq!(kite_size(15.0, 80.0), 9.0);
    assert_eq!(kite_size(13.0, 80.0), 12.0);
    assert_eq!(kite_size(12.0, 80.0), 12.0);
    assert_eq!(kite_size(10.0, 80.0), 14.0);
    assert_eq!(kite_size(7.0, 80.0), 0.0);
}

#[test]
fn wing_80kg_step_table() {
    assert_eq!(wing_size(30.0, 80.0), 3.2);
    assert_eq!(wing_size(28.0, 80.0), 3.2);
    assert_eq!(wing_size(25.0, 80.0), 4.2);
    assert_eq!(wing_size(22.0, 80.0), 4.2);
    assert_eq!(wing_size(18.0, 80.0), 5.0);
    assert_eq!(wing_size(16.0, 80.0), 5.0);
    assert_eq!(wing_size(12.0, 80.0), 7.0);
    assert_eq!(wing_size(10.0, 80.0), 7.0);
    assert_eq!(wing_size(9.0, 80.0), 0.0);
}

#[test]
fn heavier_rider_gets_smaller_kite_number_at_same_wind() {
    let heavy = kite_size(18.0, 100.0);
    let ref_size = kite_size(18.0, 80.0);
    assert!(heavy <= ref_size, "100 kg @ 18 kn should step down to same or smaller kite size number");
}

#[test]
fn lighter_rider_gets_larger_kite_number_at_same_wind() {
    let light = kite_size(18.0, 60.0);
    let ref_size = kite_size(18.0, 80.0);
    assert!(light >= ref_size, "60 kg @ 18 kn should remain at same or larger kite size number");
}

#[test]
fn heavier_rider_shifts_breakpoints_down() {
    // 100 kg rider: breakpoints scale by 80/100 = 0.8
    // The 12 kn breakpoint becomes 9.6 kn, so 10 kn should yield 12m (not 14m)
    assert_eq!(kite_size(10.0, 100.0), 12.0);
    assert_eq!(kite_size(10.0, 80.0), 14.0);
}

#[test]
fn lighter_rider_shifts_breakpoints_up() {
    // 60 kg rider: breakpoints scale by 80/60 = 1.333
    // The 12 kn breakpoint becomes 16 kn, so 14 kn should yield 14m
    assert_eq!(kite_size(14.0, 60.0), 14.0);
    assert_eq!(kite_size(14.0, 80.0), 12.0);
}

#[test]
fn below_min_wind_returns_zero() {
    assert_eq!(kite_size(5.0, 80.0), 0.0);
    assert_eq!(wing_size(5.0, 80.0), 0.0);
}
