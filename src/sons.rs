//! Les petits sons d'Eve : un signal bref quand elle entend « Eve », une cloche pour une alerte. Synthétisés en
//! mémoire au premier appel (aucun fichier), joués par Windows sans attendre : ils ne retardent ni la voix ni le chef.
//!
//! Des notes, pas des bips : une attaque douce (quelques millisecondes), une décroissance exponentielle, un peu
//! d'harmoniques — et, pour la cloche, des partiels inharmoniques qui s'éteignent avant la fondamentale. Les crêtes
//! sont à -12 dBFS : présentes sans être fortes.

use std::f32::consts::PI;
use std::sync::OnceLock;
use std::time::Duration;

/// Un son d'Eve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Son {
    /// « Eve » entendu : deux notes douces qui montent (≈ 0,25 s).
    Eveil,
    /// Une alerte (un courriel suspect, un rendez-vous qui approche) : trois notes de cloche (≈ 0,6 s).
    Alerte,
}

/// La cadence des sons (celle des voix de Piper).
const TAUX: u32 = 22_050;
/// La crête : -12 dBFS (10^(-12/20)).
const CRETE: f32 = 0.251;
/// L'attaque de chaque note (une demi-cosinusoïde : ni clic ni bip).
const ATTAQUE: f32 = 0.006;
/// Le fondu au bout de chaque note, pour qu'elle s'éteigne à zéro.
const FONDU: f32 = 0.02;

/// Une note : sa fréquence (Hz), son départ et sa longueur (s), sa constante d'amortissement (s).
struct Note {
    frequence: f32,
    debut: f32,
    longueur: f32,
    amorti: f32,
}

/// Le timbre doux de l'éveil : la fondamentale, un peu d'harmoniques 2 et 3 (rapport à la fondamentale, amplitude).
const DOUX: [(f32, f32); 3] = [(1.0, 1.0), (2.0, 0.22), (3.0, 0.08)];
/// Une cloche : l'octave et deux partiels inharmoniques, plus faibles.
const CLOCHE: [(f32, f32); 4] = [(1.0, 1.0), (2.0, 0.32), (2.76, 0.14), (5.4, 0.05)];

/// Mi puis la, qui montent : « oui ? ».
const EVEIL: [Note; 2] = [
    Note { frequence: 659.25, debut: 0.0, longueur: 0.13, amorti: 0.045 },
    Note { frequence: 880.0, debut: 0.11, longueur: 0.14, amorti: 0.05 },
];
/// Do, sol, mi, qui descendent : un carillon d'annonce.
const ALERTE: [Note; 3] = [
    Note { frequence: 1046.5, debut: 0.0, longueur: 0.24, amorti: 0.08 },
    Note { frequence: 783.99, debut: 0.17, longueur: 0.24, amorti: 0.09 },
    Note { frequence: 659.25, debut: 0.34, longueur: 0.26, amorti: 0.11 },
];

/// Les notes mêlées, en PCM 16 bits, la crête à -12 dBFS.
fn synthetiser(notes: &[Note], partiels: &[(f32, f32)]) -> Vec<i16> {
    let taux = TAUX as f32;
    let fin = notes.iter().map(|n| n.debut + n.longueur).fold(0.0, f32::max);
    let total = (fin * taux).round() as usize;
    let mut x = vec![0.0f32; total];
    for n in notes {
        let debut = (n.debut * taux).round() as usize;
        let longueur = ((n.longueur * taux).round() as usize).min(total.saturating_sub(debut));
        for i in 0..longueur {
            let t = i as f32 / taux;
            let attaque = 0.5 - 0.5 * (PI * (t / ATTAQUE).min(1.0)).cos();
            let fondu = ((longueur - i) as f32 / taux / FONDU).min(1.0);
            let mut v = 0.0;
            for &(rapport, amplitude) in partiels {
                let f = n.frequence * rapport;
                if f < taux / 2.0 {
                    // Les partiels aigus s'éteignent plus vite : le son s'arrondit en mourant.
                    v += amplitude * (-t * rapport.sqrt() / n.amorti).exp() * (2.0 * PI * f * t).sin();
                }
            }
            x[debut + i] += attaque * fondu * v;
        }
    }
    let crete = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let k = if crete > 0.0 { CRETE * 32_767.0 / crete } else { 0.0 };
    x.iter().map(|v| (v * k).round().clamp(-32_768.0, 32_767.0) as i16).collect()
}

/// Un WAV mono 16 bits à 22 050 Hz.
fn wav(echantillons: &[i16]) -> Vec<u8> {
    let donnees = (echantillons.len() * 2) as u32;
    let mut o = Vec::with_capacity(44 + donnees as usize);
    o.extend_from_slice(b"RIFF");
    o.extend_from_slice(&(36 + donnees).to_le_bytes());
    o.extend_from_slice(b"WAVEfmt ");
    o.extend_from_slice(&16u32.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&TAUX.to_le_bytes());
    o.extend_from_slice(&(TAUX * 2).to_le_bytes());
    o.extend_from_slice(&2u16.to_le_bytes());
    o.extend_from_slice(&16u16.to_le_bytes());
    o.extend_from_slice(b"data");
    o.extend_from_slice(&donnees.to_le_bytes());
    for e in echantillons {
        o.extend_from_slice(&e.to_le_bytes());
    }
    o
}

/// Le WAV d'un son, synthétisé une fois : statique, il vit pendant toute lecture asynchrone.
fn tampon(son: Son) -> &'static [u8] {
    static EVEIL_WAV: OnceLock<Vec<u8>> = OnceLock::new();
    static ALERTE_WAV: OnceLock<Vec<u8>> = OnceLock::new();
    match son {
        Son::Eveil => EVEIL_WAV.get_or_init(|| wav(&synthetiser(&EVEIL, &DOUX))),
        Son::Alerte => ALERTE_WAV.get_or_init(|| wav(&synthetiser(&ALERTE, &CLOCHE))),
    }
}

/// Joue un son sans attendre (un son en cours s'arrête pour lui) ; rien sous `EVE_MUET` ([`crate::voix::muette`]).
#[cfg(windows)]
pub fn jouer(son: Son) {
    if !crate::voix::muette() {
        let _ = lancer(son);
    }
}

/// Confie le son à Windows ; faux s'il le refuse.
#[cfg(windows)]
fn lancer(son: Son) -> bool {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    use windows::core::PCWSTR;
    let o = tampon(son);
    // SAFETY : avec SND_MEMORY, le premier argument désigne le WAV en mémoire, et non un nom ; le tampon est
    // statique, il reste donc valable pendant toute la lecture asynchrone.
    unsafe { PlaySoundW(PCWSTR(o.as_ptr().cast()), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT) }.as_bool()
}

/// Joue un son sans attendre : il n'y a de son que sous Windows.
#[cfg(not(windows))]
pub fn jouer(_son: Son) {}

/// La durée d'un son : le micro reste en sourdine pendant ce temps (Eve ne s'entend pas elle-même).
pub fn duree(son: Son) -> Duration {
    Duration::from_secs_f64((tampon(son).len() - 44) as f64 / f64::from(TAUX * 2))
}

#[cfg(test)]
mod essais {
    use super::*;

    fn mot(o: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([o[i], o[i + 1], o[i + 2], o[i + 3]])
    }

    #[test]
    fn les_sons_sont_des_wav_de_la_bonne_duree() {
        for (son, attendue) in [(Son::Eveil, 0.25), (Son::Alerte, 0.60)] {
            let o = tampon(son);
            assert_eq!(&o[0..4], b"RIFF");
            assert_eq!(mot(o, 4) as usize, o.len() - 8);
            assert_eq!(&o[8..16], b"WAVEfmt ");
            assert_eq!(mot(o, 16), 16);
            assert_eq!(u16::from_le_bytes([o[20], o[21]]), 1, "PCM");
            assert_eq!(u16::from_le_bytes([o[22], o[23]]), 1, "mono");
            assert_eq!(mot(o, 24), 22_050);
            assert_eq!(mot(o, 28), 44_100);
            assert_eq!(u16::from_le_bytes([o[34], o[35]]), 16);
            assert_eq!(&o[36..40], b"data");
            assert_eq!(mot(o, 40) as usize, o.len() - 44);
            let d = duree(son).as_secs_f64();
            assert!((d - attendue).abs() < 0.01, "{son:?} : {d} s");
            // La crête vers -12 dBFS, un début et une fin au silence (pas de clic).
            let e: Vec<i16> = o[44..].as_chunks::<2>().0.iter().map(|&b| i16::from_le_bytes(b)).collect();
            let crete = e.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0);
            assert!((8_000..=8_300).contains(&crete), "{son:?} : crête {crete}");
            assert!(e[0].unsigned_abs() < 100 && e[e.len() - 1].unsigned_abs() < 100, "{son:?} : un clic");
        }
        assert!(duree(Son::Eveil) < duree(Son::Alerte));
    }

    /// Sur le poste (les deux sons se font entendre) : `cargo test --release -- --ignored --nocapture les_sons_se_jouent`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn les_sons_se_jouent() {
        for son in [Son::Eveil, Son::Alerte] {
            println!("{son:?} : {:.0} ms", duree(son).as_secs_f64() * 1000.0);
            assert!(lancer(son), "Windows refuse le son {son:?}");
            std::thread::sleep(duree(son) + Duration::from_millis(500));
        }
    }
}
