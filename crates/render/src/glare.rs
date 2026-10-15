//! The viewer's eye's glare (M1c.md, step 6, part 10): the light past
//! what the output can show, scattered as an eye scatters it, by the
//! CIE's glare spread function (Vos and van den Berg 1999, CIE 135/1).
//! What the output does show, the viewer's own eye scatters.
//!
//! The function as McCann and Vonikakis 2018 give it (their eq. 2), its
//! core's half-width read 0.0046°, not their 0.046°: at 0.046° its
//! integral is 37, not ~1, and its tail 900/θ³, against the CIE's general
//! glare equation's 10/θ³ (CIE 146:2002, as Bará and Bao-Varela 2023
//! give it).

/// A viewer's eye, by what its glare depends on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Eye {
    /// Years.
    pub age: f64,
    /// The iris's pigmentation: 0 very dark, 0.5 brown, 1 blue-green,
    /// 1.2 blue.
    pub pigmentation: f64,
}

impl Eye {
    /// A young brown-eyed viewer's (McCann and Vonikakis's).
    pub const STANDARD: Eye = Eye {
        age: 25.0,
        pigmentation: 0.5,
    };

    /// The glare spread function at `theta` degrees off a point source,
    /// sr⁻¹, as the CIE gives it: its integral over the sphere is ~1.05.
    fn spread(&self, theta: f64) -> f64 {
        let a = (self.age / 70.0).powi(4);
        let p = self.pigmentation;
        let q = 1.0 + (theta / 0.1).powi(2);
        (1.0 - 0.08 * a)
            * (9.2e6 / (1.0 + (theta / 0.0046).powi(2)).powf(1.5)
                + 1.5e5 / (1.0 + (theta / 0.045).powi(2)).powf(1.5))
            + (1.0 + 1.6 * a)
                * (400.0 / q + 3e-8 * theta * theta + p * (1300.0 / q.powf(1.5) + 0.8 / q.sqrt()))
            + 2.5e-3 * p
    }

    /// The unnormalized light landing between `from` and `to` degrees off
    /// a point source.
    fn between(&self, from: f64, to: f64) -> f64 {
        // Midpoints in log θ, 200 a decade: within 1e-5 of 100× finer.
        let from = from.max(1e-5);
        if to <= from {
            return 0.0;
        }
        let (l0, l1) = (from.ln(), to.ln());
        let n = ((l1 - l0) * 200.0 / std::f64::consts::LN_10)
            .ceil()
            .max(1.0) as usize;
        (0..n)
            .map(|i| {
                let u0 = (l0 + (l1 - l0) * i as f64 / n as f64).exp();
                let u1 = (l0 + (l1 - l0) * (i + 1) as f64 / n as f64).exp();
                let u = (u0 * u1).sqrt();
                self.spread(u)
                    * std::f64::consts::TAU
                    * u.to_radians().sin()
                    * (u1 - u0).to_radians()
            })
            .sum()
    }

    /// The share of a point source's light that lands between `from` and
    /// `to` degrees off it.
    pub fn share(&self, from: f64, to: f64) -> f64 {
        self.between(from.min(180.0), to.min(180.0)) / self.between(0.0, 180.0)
    }
}

/// How a level spreads a point's light, measured (each level alone at
/// 1920×1080; levels 2–6's mean): the share landing in each octave of
/// distance from 4 octaves in from the one it peaks in, 2^(k+1)–2^(k+2) px
/// for level k, to 2 out.
const SPREAD: [f64; 7] = [0.0035, 0.0152, 0.0594, 0.2034, 0.4612, 0.2575, 0.0021];

/// The first level's: under 1 px, 1–2, 2–4 and 4–8.
const FIRST: [f64; 4] = [0.199, 0.304, 0.447, 0.053];

/// The second's, as [`SPREAD`]'s (its first is under the image's least
/// octave, its second under 1 px).
const SECOND: [f64; 7] = [0.0, 0.0311, 0.0546, 0.2354, 0.4738, 0.2053, 0.0006];

/// The last level's, a few texels across, as [`SPREAD`]'s: the image's
/// edges cut it off its outer octave.
const LAST: [f64; 7] = [0.0048, 0.0193, 0.0764, 0.2958, 0.5148, 0.0118, 0.0];

/// Each of `levels` glare levels' weight, for a target whose pixels
/// subtend `pixel` radians at the eye: so that the light they spread
/// lands in each octave of distance as the eye's glare does, from half a
/// pixel (nearer, the source's own pixel) to where the last level's
/// peaks. Beyond 4 px within ~12% an octave; nearer, the first level's
/// one shape gives up to ~42% over at 2–4 px (at a display scale of 2;
/// 30% at 1), and within ~8% at 1–2.
pub(crate) fn weights(eye: &Eye, pixel: f64, levels: usize) -> Vec<f64> {
    let degrees = pixel.to_degrees();
    // Octave b ≥ 1 is 2^(b-1)–2^b px; 0 is under 1.
    let n = levels + 2;
    let target: Vec<f64> = (0..n)
        .map(|b| {
            let from = if b == 0 { 0.5 } else { 2f64.powi(b as i32 - 1) };
            eye.share(from * degrees, 2f64.powi(b as i32) * degrees)
        })
        .collect();
    let spread: Vec<Vec<f64>> = (0..levels)
        .map(|k| {
            let mut r = vec![0.0; n];
            if k == 0 {
                for (b, s) in FIRST.iter().enumerate() {
                    r[b.min(n - 1)] += s;
                }
            } else {
                let shape = match k {
                    _ if k + 1 == levels => &LAST,
                    1 => &SECOND,
                    _ => &SPREAD,
                };
                for (i, s) in shape.iter().enumerate() {
                    let b = (k + i).saturating_sub(2);
                    if b < n {
                        r[b] += s;
                    }
                }
            }
            r
        })
        .collect();
    // Richardson–Lucy, from each level at its peak octave's share: 10
    // rounds (more make the weights alternate, level to level).
    let mut w: Vec<f64> = (0..levels).map(|k| target[(k + 2).min(n - 1)]).collect();
    w[0] = target[..3.min(n)].iter().sum();
    for _ in 0..10 {
        let got: Vec<f64> = (0..n)
            .map(|b| (0..levels).map(|k| w[k] * spread[k][b]).sum())
            .collect();
        for (w, r) in w.iter_mut().zip(&spread) {
            let gain: f64 = (0..n)
                .filter(|&b| got[b] > 0.0)
                .map(|b| r[b] * target[b] / got[b])
                .sum();
            *w *= gain / r.iter().sum::<f64>();
        }
    }
    w
}
