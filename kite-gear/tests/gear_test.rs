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
fn heavier_rider_gets_larger_kite_at_same_wind() {
    let heavy = kite_size(18.0, 100.0);
    let ref_size = kite_size(18.0, 80.0);
    assert!(
        heavy >= ref_size,
        "heavier rider should get same or larger kite (m²) at the same wind"
    );
}

#[test]
fn lighter_rider_gets_smaller_kite_at_same_wind() {
    let light = kite_size(18.0, 60.0);
    let ref_size = kite_size(18.0, 80.0);
    assert!(
        light <= ref_size,
        "lighter rider should get same or smaller kite (m²) at the same wind"
    );
}

#[test]
fn heavier_rider_needs_more_wind_to_downsize_kite() {
    // f = rider/80: at 12 kn, 80 kg is in the 12 m² band; 100 kg still in 14 m² (more power needed before dropping a size).
    assert_eq!(kite_size(12.0, 100.0), 14.0);
    assert_eq!(kite_size(12.0, 80.0), 12.0);
}

#[test]
fn lighter_rider_downsizes_kite_earlier() {
    // Same wind: lighter rider is already in a smaller-kite band than 80 kg reference.
    assert_eq!(kite_size(14.0, 60.0), 9.0);
    assert_eq!(kite_size(14.0, 80.0), 12.0);
}

/// At 15 kn, heavier riders sit in larger-kite bands (`weight_factor = rider_kg / 80`).
#[test]
fn kite_size_100kg_vs_50kg_same_wind() {
    const W: f64 = 15.0;
    for r in [50.0, 55.0, 60.0] {
        assert_eq!(kite_size(W, r), 7.0, "{r} kg @ {W} kn");
    }
    for r in [65.0, 70.0, 75.0, 80.0] {
        assert_eq!(kite_size(W, r), 9.0, "{r} kg @ {W} kn");
    }
    for r in [85.0, 90.0, 95.0, 100.0] {
        assert_eq!(kite_size(W, r), 12.0, "{r} kg @ {W} kn");
    }
    assert!(kite_size(W, 100.0) > kite_size(W, 50.0));
}

#[test]
fn below_min_wind_returns_zero() {
    assert_eq!(kite_size(4.0, 80.0), 0.0);
    assert_eq!(wing_size(4.0, 80.0), 0.0);
}
