//! L'inspiration qu'on entend juste avant qu'Eve parle, après un silence : un souffle très doux, au moment même où son
//! portrait inspire (le chef dit « parole » quand il donne la première phrase à la voix), et la voix part juste après
//! lui. Rien d'un enregistrement : un bruit fait ici, coloré par deux résonances de la bouche ouverte (vers 700 et
//! 1 700 Hz) et un peu d'air plus aigu, qui monte puis retombe. Éteinte d'office (le réglage `souffle`) : à écouter
//! avant de l'allumer.

use std::time::Duration;

use super::wav_mono;

/// La durée de l'inspiration, et le temps qu'elle met à monter (elle retombe ensuite, plus vite).
const DUREE: f64 = 0.36;
const MONTEE: f64 = 0.24;
/// Son niveau au plus fort (RMS, en dBFS sur l'échelle de Piper, avant `adoucir`) : 23 dB sous la voix (son niveau
/// actif, −15 dBFS) — on la devine plus qu'on ne l'entend.
const NIVEAU: f32 = -38.0;
/// Entre la fin du souffle et la première syllabe.
pub(super) const APRES: Duration = Duration::from_millis(60);

/// Un filtre passe-bande du second ordre (une résonance, de gain 1 à son sommet).
struct Resonance {
    b0: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Resonance {
    fn new(frequence: f32, q: f32, taux: u32) -> Resonance {
        let w = 2.0 * std::f32::consts::PI * frequence / taux as f32;
        let alpha = w.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        Resonance {
            b0: alpha / a0,
            a1: -2.0 * w.cos() / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn filtrer(&mut self, x: f32) -> f32 {
        let y = self.b0 * (x - self.x2) - self.a1 * self.y1 - self.a2 * self.y2;
        (self.x2, self.x1, self.y2, self.y1) = (self.x1, x, self.y1, y);
        y
    }
}

/// L'inspiration, en WAV mono 16 bits à la cadence `taux` (celle de Piper : 22 050 Hz).
pub(super) fn inspiration(taux: u32) -> Vec<u8> {
    let n = (DUREE * f64::from(taux)) as usize;
    let mut graine: u32 = 0x5eed_1234;
    let (mut basse, mut haute) = (Resonance::new(700.0, 1.2, taux), Resonance::new(1_700.0, 1.6, taux));
    let mut x: Vec<f32> = (0..n)
        .map(|_| {
            graine = graine.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let b = (graine >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0;
            basse.filtrer(b) + 0.8 * haute.filtrer(b) + 0.15 * b
        })
        .collect();
    let rms = (x.iter().map(|v| v * v).sum::<f32>() / n.max(1) as f32).sqrt().max(1e-9);
    let gain = 10f32.powf(NIVEAU / 20.0) / rms;
    let (montee, descente) = (MONTEE * f64::from(taux), (DUREE - MONTEE) * f64::from(taux));
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f64;
        let e = if t < montee {
            (std::f64::consts::FRAC_PI_2 * t / montee).sin().powi(2)
        } else {
            (std::f64::consts::FRAC_PI_2 * (t - montee) / descente).cos().powi(2)
        };
        *v *= gain * e as f32;
    }
    let echantillons: Vec<i16> = x.iter().map(|v| (v * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16).collect();
    wav_mono(&echantillons, taux)
}

#[cfg(test)]
mod essais {
    use super::*;
    use crate::voix::{duree_wav, entete};

    #[test]
    fn l_inspiration_est_breve_douce_et_sans_clic() {
        let w = inspiration(22_050);
        assert!((duree_wav(&w).unwrap().as_secs_f64() - DUREE).abs() < 0.001);
        let e = entete(&w).unwrap();
        let (ech, _) = w[e.debut..e.debut + e.taille].as_chunks::<2>();
        let x: Vec<f32> = ech.iter().map(|&b| f32::from(i16::from_le_bytes(b)) / 32_768.0).collect();
        // Elle part de zéro et y revient ; au plus fort (autour de la fin de sa montée), une vingtaine de décibels sous
        // la voix.
        assert!(x[0].abs() < 1e-3 && x[x.len() - 1].abs() < 1e-3);
        let m = (MONTEE * 22_050.0) as usize;
        let fort = &x[m - 1_100..m + 1_100];
        let db = 10.0 * (fort.iter().map(|v| v * v).sum::<f32>() / fort.len() as f32).log10();
        assert!((db - NIVEAU).abs() < 2.5, "{db}");
        assert!(x.iter().all(|v| v.abs() < 0.1));
    }
}
