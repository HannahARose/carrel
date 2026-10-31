//! 16-bit floats (IEEE 754 binary16), as the index keeps its vectors: half the size of 32-bit
//! floats, and precise enough to compare unit vectors.

use std::sync::OnceLock;

/// `value` as a 16-bit float's bits, rounded to the nearest, ties to even. Values too large
/// become infinity and values too small become zero, keeping their sign.
#[must_use]
pub fn to_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = u16::try_from((bits >> 16) & 0x8000).unwrap_or(0);
    let exponent = i32::try_from((bits >> 23) & 0xff).unwrap_or(0);
    let mantissa = bits & 0x007f_ffff;
    if exponent == 0xff {
        let nan = if mantissa == 0 { 0 } else { 0x0200 };
        return sign | 0x7c00 | nan;
    }
    let half_exponent = exponent - 127 + 15;
    if half_exponent >= 0x1f {
        return sign | 0x7c00;
    }
    let (kept, shift) = if half_exponent <= 0 {
        if half_exponent < -10 {
            return sign;
        }
        // A subnormal: the implicit leading one becomes explicit.
        (
            mantissa | 0x0080_0000,
            u32::try_from(14 - half_exponent).unwrap_or(24),
        )
    } else {
        (
            (u32::try_from(half_exponent).unwrap_or(0) << 23) | mantissa,
            13,
        )
    };
    let truncated = kept >> shift;
    let rest = kept & ((1 << shift) - 1);
    let middle = 1 << (shift - 1);
    let rounded = if rest > middle || (rest == middle && truncated & 1 == 1) {
        truncated + 1
    } else {
        truncated
    };
    // A carry out of the mantissa moves into the exponent, as it should, up to infinity.
    sign | u16::try_from(rounded).unwrap_or(0x7c00)
}

/// The 32-bit float a 16-bit float's `bits` stand for, exactly.
#[must_use]
pub fn from_bits(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x03ff);
    match exponent {
        0 => {
            // Zero or a subnormal: the mantissa times 2^-24.
            let value = f32::from(bits & 0x03ff) / 16_777_216.0;
            if sign == 0 { value } else { -value }
        }
        0x1f => f32::from_bits(sign | 0x7f80_0000 | (mantissa << 13)),
        _ => f32::from_bits(sign | ((exponent + 112) << 23) | (mantissa << 13)),
    }
}

/// Every 16-bit float's value, so comparing vectors looks numbers up instead of converting
/// each one.
pub(crate) fn table() -> &'static [f32] {
    static TABLE: OnceLock<Vec<f32>> = OnceLock::new();
    TABLE.get_or_init(|| (0..=u16::MAX).map(from_bits).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_to_the_nearest_half() {
        for value in [0.0, 1.0, -1.0, 0.5, 0.333_333, -0.6, 65504.0, 6.1e-5] {
            let back = from_bits(to_bits(value));
            assert!(
                (back - value).abs() <= value.abs() / 1024.0,
                "{value} came back as {back}"
            );
        }
        assert_eq!(to_bits(1.0), 0x3c00);
        assert_eq!(to_bits(-2.0), 0xc000);
        assert_eq!(to_bits(65504.0), 0x7bff);
        assert_eq!(to_bits(1.0e6), 0x7c00, "too large is infinity");
        assert_eq!(to_bits(-1.0e-9), 0x8000, "too small is zero, with its sign");
        assert_eq!(to_bits(5.960_464_5e-8), 0x0001, "the smallest subnormal");
        assert!(from_bits(to_bits(f32::NAN)).is_nan());
        // Ties go to even: 1 + 2^-11 is halfway between 1 and the next half.
        assert_eq!(to_bits(1.0 + 1.0 / 2048.0), 0x3c00);
        assert_eq!(to_bits(1.0 + 3.0 / 2048.0), 0x3c02);
    }

    #[test]
    fn every_half_converts_back_to_itself() {
        for bits in 0..=u16::MAX {
            let value = from_bits(bits);
            if !value.is_nan() {
                assert_eq!(to_bits(value), bits, "{bits:#06x} is {value}");
            }
        }
        assert_eq!(table().len(), 65536);
    }
}
