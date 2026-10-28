//! Les filtres du son, communs à la musique du centre de commande ([`crate::musique`]) et à la voix d'Eve (le
//! studio, `voix/studio.rs`) : le biquad, en f64 — une coupure basse devant la cadence reste juste —, et ses
//! recettes, celles du livre de Robert Bristow-Johnson, que suit WebAudio.

use std::f64::consts::TAU;

/// Un biquad (la forme transposée directe II). Son état reste quand on le règle : une coupure peut glisser sans clic.
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    /// Un passe-bas ; `q` linéaire.
    pub fn passe_bas(frequence: f64, q: f64, taux: f64) -> Biquad {
        let mut b = Biquad::default();
        b.regler(frequence, q, taux);
        b
    }

    /// Un passe-haut ; `q` linéaire (1/√2 : celui de Butterworth, sans bosse).
    pub fn passe_haut(frequence: f64, q: f64, taux: f64) -> Biquad {
        let (s, c) = (TAU * frequence / taux).sin_cos();
        let alpha = s / (2.0 * q);
        Biquad::de([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], [1.0 + alpha, -2.0 * c, 1.0 - alpha])
    }

    /// Une cloche : `gain_db` autour de `frequence`, sur une largeur que dit `q` (plus il est grand, plus elle est
    /// étroite).
    pub fn cloche(frequence: f64, q: f64, gain_db: f64, taux: f64) -> Biquad {
        let a = 10f64.powf(gain_db / 40.0);
        let (s, c) = (TAU * frequence / taux).sin_cos();
        let alpha = s / (2.0 * q);
        Biquad::de([1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a], [1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a])
    }

    /// Un plateau haut : `gain_db` au-dessus de `frequence` (la moitié du gain à `frequence`), rien bien en dessous.
    pub fn plateau_haut(frequence: f64, q: f64, gain_db: f64, taux: f64) -> Biquad {
        let a = 10f64.powf(gain_db / 40.0);
        let (s, c) = (TAU * frequence / taux).sin_cos();
        let r = a.sqrt() * s / q;
        Biquad::de(
            [
                a * ((a + 1.0) + (a - 1.0) * c + r),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
                a * ((a + 1.0) + (a - 1.0) * c - r),
            ],
            [(a + 1.0) - (a - 1.0) * c + r, 2.0 * ((a - 1.0) - (a + 1.0) * c), (a + 1.0) - (a - 1.0) * c - r],
        )
    }

    /// Les coefficients, ramenés à `a[0]` = 1.
    fn de(b: [f64; 3], a: [f64; 3]) -> Biquad {
        let [b0, b1, b2] = b.map(|v| v / a[0]);
        Biquad { b0, b1, b2, a1: a[1] / a[0], a2: a[2] / a[0], z1: 0.0, z2: 0.0 }
    }

    /// Règle un passe-bas, l'état gardé ; `q` linéaire.
    pub fn regler(&mut self, frequence: f64, q: f64, taux: f64) {
        let (s, c) = (TAU * frequence / taux).sin_cos();
        let alpha = s / (2.0 * q);
        let a0 = 1.0 + alpha;
        self.b0 = (1.0 - c) / 2.0 / a0;
        self.b1 = (1.0 - c) / a0;
        self.b2 = self.b0;
        self.a1 = -2.0 * c / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    #[inline]
    pub fn filtrer(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    pub fn vider(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    /// Le gain à une fréquence (pour les essais).
    #[cfg(test)]
    pub fn gain(&self, frequence: f64, taux: f64) -> f64 {
        let w = TAU * frequence / taux;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let (nr, ni) = (self.b0 + self.b1 * c1 + self.b2 * c2, -(self.b1 * s1 + self.b2 * s2));
        let (dr, di) = (1.0 + self.a1 * c1 + self.a2 * c2, -(self.a1 * s1 + self.a2 * s2));
        ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt()
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    const TAUX: f64 = 22_050.0;

    fn db(g: f64) -> f64 {
        20.0 * g.log10()
    }

    #[test]
    fn les_recettes_font_ce_qu_elles_disent() {
        // Le passe-haut de Butterworth : -3 dB à sa coupure, rien une octave et demie au-dessus, -12 dB une octave
        // au-dessous.
        let h = Biquad::passe_haut(80.0, std::f64::consts::FRAC_1_SQRT_2, TAUX);
        assert!((db(h.gain(80.0, TAUX)) + 3.01).abs() < 0.05, "{}", db(h.gain(80.0, TAUX)));
        assert!(db(h.gain(250.0, TAUX)).abs() < 0.2);
        assert!((db(h.gain(40.0, TAUX)) + 12.3).abs() < 0.5, "{}", db(h.gain(40.0, TAUX)));
        // La cloche : son gain à sa fréquence, rien loin d'elle.
        let c = Biquad::cloche(3000.0, 1.0, 2.5, TAUX);
        assert!((db(c.gain(3000.0, TAUX)) - 2.5).abs() < 1e-6);
        assert!(db(c.gain(200.0, TAUX)).abs() < 0.1);
        // Le plateau haut : tout son gain tout en haut, la moitié à sa fréquence, rien en bas.
        let p = Biquad::plateau_haut(1500.0, std::f64::consts::FRAC_1_SQRT_2, 4.0, TAUX);
        assert!((db(p.gain(10_000.0, TAUX)) - 4.0).abs() < 0.2, "{}", db(p.gain(10_000.0, TAUX)));
        assert!((db(p.gain(1500.0, TAUX)) - 2.0).abs() < 0.05);
        assert!(db(p.gain(100.0, TAUX)).abs() < 0.05);
        // Filtrer, c'est cela : un sinus de 3 kHz passe la cloche avec son gain.
        let mut c = c;
        let y: Vec<f64> = (0..22_050).map(|k| c.filtrer((TAU * 3000.0 * f64::from(k) / TAUX).sin())).collect();
        let efficace = (y[11_025..].iter().map(|v| v * v).sum::<f64>() / 11_025.0).sqrt();
        assert!((db(efficace * 2f64.sqrt()) - 2.5).abs() < 0.05, "{}", db(efficace * 2f64.sqrt()));
    }
}
