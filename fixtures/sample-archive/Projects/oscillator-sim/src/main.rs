fn main() {
    let (w1, w2, k, dt) = (1.0_f64, 1.2_f64, 0.15_f64, 0.01_f64);
    let (mut a, mut b) = (0.0_f64, 2.0_f64);
    for step in 0..20_000 {
        let da = w1 + k * (b - a).sin();
        let db = w2 + k * (a - b).sin();
        a += da * dt;
        b += db * dt;
        if step % 5_000 == 0 {
            println!("t={:.0} phase difference={:.3}", step as f64 * dt, (b - a).rem_euclid(std::f64::consts::TAU));
        }
    }
}
