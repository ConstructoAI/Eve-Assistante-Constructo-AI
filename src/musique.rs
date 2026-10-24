//! La musique du centre de commande (le lot 6) : la musique générative et les notes des événements du tableau que
//! l'Assistante famille s'est fait (`centre-de-commande.html`, aux archives, ses lignes 255 à 320), qui la
//! jouait par WebAudio dans Edge. Ici, un petit synthétiseur dans le rappel d'une sortie audio (cpal, WASAPI) : Windows
//! la mêle à la voix d'Eve (Piper), comme il mêlait la page.
//!
//! Le graphe de la page, ses nombres repris tels quels :
//! - **la nappe** : cinq rapports de fréquence (1, 1,5, 2, 3, 2,25) à la moitié de la note de base de la gamme, deux
//!   oscillateurs désaccordés de ±6 cents chacun (des triangles ; pour le dernier rapport, des sinus), 0,018 chacun,
//!   dans un passe-bas à 900 Hz qu'un LFO balaie de ±250 Hz en 20 s ; elle glisse vers une nouvelle gamme en 2,5 s ;
//! - **les notes** (l'arpège et les événements) : une cloche douce — un sinus, et un triangle à l'octave qui s'éteint
//!   deux fois plus vite —, 12 ms d'attaque puis une décroissance exponentielle ; elles passent par un bus à 0,6, de là
//!   à sec et dans une réverbération de 3,5 s (la convolution de la page : un bruit qui s'éteint, ses deux canaux
//!   indépendants, son niveau normalisé comme le fait WebAudio), qui ressort à 0,85 ;
//! - **l'arpège** : une note toutes les 2,2 à 4 s, trois fois sur quatre ; quand Eve travaille, toutes les 0,65 à
//!   1,15 s, chaque fois ; un pas de -2 à +2 sur deux octaves de la gamme ;
//! - **les événements** : deux notes qui montent quand un outil part, une qui se pose quand il revient, un accord quand
//!   une impulsion arrive au bout du réseau ;
//! - **le maître** : 0,855 le jour, 0,45 la nuit, qui glisse en 0,6 s.
//!
//! Ce que la page n'avait pas à savoir : Eve écoute. La musique se tait (un fondu de 50 ms) pendant qu'elle écoute,
//! transcrit ou parle — le micro ne doit pas l'entendre, la voix d'Eve doit rester claire —, et ne revient qu'après
//! 1,2 s de calme. Le reste du temps, quand l'éveil écoute, seule la pièce la sépare du micro : [`VOLUME`] dit ce qu'on
//! en a mesuré sur le poste (`eve --essai-musique`), et quand il faut la taire.
//!
//! Le rappel du son ne prend aucun verrou et n'alloue rien : la fenêtre lui parle par des atomiques, et tout ce qu'il
//! calcule est alloué à l'ouverture.

use std::f64::consts::{PI, TAU};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample};

use crate::journal;

// ---- Les gammes ---------------------------------------------------------------------------------------------------

/// Une gamme de cinq notes : la note de base (Hz) et ses intervalles (en demi-tons). Pentatoniques : quel que soit le
/// hasard de l'arpège, deux notes sonnent toujours juste ensemble.
pub struct Gamme {
    pub nom: &'static str,
    pub base: f64,
    pub intervalles: [u8; 5],
}

/// Les gammes du tableau (`GAMMES`) : celle d'office (ré majeur), et celle de chaque sujet que le réseau dessine.
pub const GAMMES: [Gamme; 8] = [
    Gamme { nom: "defaut", base: 293.66, intervalles: [0, 2, 4, 7, 9] },
    Gamme { nom: "ecole", base: 392.0, intervalles: [0, 2, 4, 7, 9] },
    Gamme { nom: "piece", base: 220.0, intervalles: [0, 3, 5, 7, 10] },
    Gamme { nom: "croix", base: 349.23, intervalles: [0, 2, 4, 7, 9] },
    Gamme { nom: "agenda", base: 293.66, intervalles: [0, 2, 3, 7, 9] },
    Gamme { nom: "lettre", base: 329.63, intervalles: [0, 2, 4, 7, 9] },
    Gamme { nom: "globe", base: 261.63, intervalles: [0, 2, 4, 6, 9] },
    Gamme { nom: "repas", base: 349.23, intervalles: [0, 2, 4, 7, 9] },
];

/// La gamme d'un sujet ; un sujet qui n'en a pas (« maison », « aucun », rien) prend celle d'office, comme la page.
pub fn gamme_du_sujet(sujet: &str) -> usize {
    GAMMES.iter().position(|g| g.nom == sujet).unwrap_or(0)
}

/// La fréquence d'un degré : cinq degrés par octave, le degré 0 sur la note de base (`freqDeg` de la page).
pub fn frequence(gamme: &Gamme, degre: i32) -> f64 {
    let note = gamme.intervalles[degre.rem_euclid(5) as usize];
    gamme.base * 2f64.powf(f64::from(note) / 12.0 + f64::from(degre.div_euclid(5)))
}

/// Les cinq fréquences de la nappe dans une gamme (avant le désaccord de ±6 cents de ses paires d'oscillateurs).
pub fn nappe_de(gamme: usize) -> [f64; 5] {
    let base = GAMMES[gamme.min(GAMMES.len() - 1)].base / 2.0;
    RAPPORTS.map(|r| base * r)
}

/// Ce que l'état d'Eve (le nom que publie le chef) fait à la musique : `(travail, silence)`. Elle travaille : l'arpège
/// s'anime. Elle écoute, transcrit ou parle : la musique se tait.
pub fn sorte_d_etat(etat: &str) -> (bool, bool) {
    match etat {
        "reflexion" => (true, false),
        "ecoute" | "transcription" | "parole" => (false, true),
        _ => (false, false),
    }
}

// ---- Les nombres de la page ---------------------------------------------------------------------------------------

/// Le volume de toute la musique, par rapport à celui de la page : 1, le même niveau numérique que dans Edge (−30,5
/// dBFS au repos, −27,7 quand Eve travaille, −35 la nuit ; des crêtes à −15 dBFS).
///
/// Mesuré le 23 octobre 2026 sur le poste de l'Assistante famille (`eve --essai-musique --marge`), avec le détecteur de
/// parole d'Eve (l'écoute du mot d'éveil) sur le micro du poste : la sortie de Windows, le moniteur SAMSUNG (HDMI), à
/// 99 % ; le micro, la LifeCam VX-5500, à 49 %. À ce volume, puis plus fort de 3 dB en 3 dB jusqu'à +15 dB (−12,8
/// dBFS, aussi fort que la voix d'Eve), le micro n'a rien entendu de la musique : ses blocs de 20 ms sont restés ceux
/// du bureau (0,0007 en médiane, 0,0017 au plus), la nappe introuvable à ses propres fréquences même à 0,2 Hz près (un
/// couplage sous −58 dB), et le détecteur n'a commencé aucune phrase en quatre minutes de musique, dont trois au niveau
/// de la page ou plus fort ; son seuil (0,012, tenu 60 ms) est resté à plus de 20 dB, comme dans le calme du bureau.
/// Ce poste n'a pas de chemin du son de sa sortie à son micro (un casque, ou les haut-parleurs du moniteur coupés) :
/// ici, la musique de la page ne réveille pas Eve, avec plus de 15 dB de marge.
///
/// Avec des haut-parleurs que le micro entend, c'est autre chose : au plus fort (60 ms tenus, ce que juge le
/// détecteur), la musique est à 15,6 dB sous la voix d'Eve (Piper, la douceur 1 : −9,1 dBFS, contre −24,7 au travail
/// et −26,9 au repos). Un micro qui entend la voix d'Eve plus fort que 0,023 (−32,8 dBFS) entend donc la musique à
/// moins de 10 dB du seuil ; celui d'un portable qui l'entendrait vers −10 dBFS (des haut-parleurs à quelques
/// centimètres) l'entendrait au-dessus — pour la garder 10 dB dessous, il faudrait 23 dB de moins, et on ne
/// l'entendrait plus. Là, elle doit se taire tant que l'éveil écoute. `eve --essai-musique --marge` le dit en trois
/// minutes, sur le poste même ; sa garde fait taire la musique avant qu'une autre Eve du poste ne la prenne pour une
/// voix.
pub const VOLUME: f64 = 1.0;

/// La nappe : les rapports à la moitié de la note de base (la fondamentale, la quinte, l'octave, la douzième, la
/// neuvième) ; des triangles, sauf le dernier rapport (des sinus).
const RAPPORTS: [f64; 5] = [1.0, 1.5, 2.0, 3.0, 2.25];
/// Les deux oscillateurs de chaque rapport, désaccordés de ±6 cents : ils battent doucement (1 à 3 Hz).
const DESACCORD_CENTS: f64 = 6.0;
const GAIN_NAPPE: f64 = 0.018;
/// Le passe-bas de la nappe : 900 Hz, balayé de ±250 Hz par un LFO à 0,05 Hz (un aller-retour en 20 s).
const COUPURE: f64 = 900.0;
const BALAYAGE: f64 = 250.0;
const LFO: f64 = 0.05;
/// Son Q : 0,6, que WebAudio lit en décibels pour un passe-bas (un Q de 1,07, une bosse légère à la coupure).
const Q_DB: f64 = 0.6;
/// La nappe glisse vers sa nouvelle gamme : la constante de temps de `setTargetAtTime`.
const GLISSE_TON: f64 = 2.5;
/// Le bus des notes, et la sortie de la réverbération.
const BUS: f64 = 0.6;
const HUMIDE: f64 = 0.85;
const DUREE_REVERB: f64 = 3.5;
/// Le maître : le jour, la nuit, et le temps qu'il met à glisser (celui de la page quand on coupe ou rallume).
const MAITRE_JOUR: f64 = 0.855;
const MAITRE_NUIT: f64 = 0.45;
const GLISSE: f64 = 0.6;
/// Le silence quand Eve écoute, transcrit ou parle : un fondu de 50 ms (−40 dB en 0,23 s), plus court à la fermeture
/// du programme. La musique ne revient qu'après 1,2 s sans silence : entre deux phrases d'Eve, ou dans les 600 ms où
/// le micro attend encore l'écho de sa voix, elle ne doit pas remonter.
const FONDU_SILENCE: f64 = 0.05;
const FONDU_FERMETURE: f64 = 0.03;
const REPRISE: f64 = 1.2;
/// L'arpège : l'intervalle entre deux tirages (le minimum, et ce que le hasard y ajoute), au repos et au travail ; au
/// repos, une note trois fois sur quatre. Le degré part de 4 et reste sur deux octaves (0 à 9).
const ARPEGE_REPOS: (f64, f64) = (2.2, 1.8);
const ARPEGE_TRAVAIL: (f64, f64) = (0.65, 0.5);
const ARPEGE_CHANCE: f64 = 0.75;
const ARPEGE_VOLUME: f64 = 0.035;
const ARPEGE_VOLUME_TRAVAIL: f64 = 0.045;
const ARPEGE_DUREE: f64 = 2.6;
const DEGRE_DEPART: i32 = 4;
const DEGRE_MAX: i32 = 9;
/// Une note (`pluck`) : 12 ms d'attaque, puis l'exponentielle jusqu'à 0,0001 au bout de sa durée ; l'oscillateur
/// s'arrête 50 ms plus tard. Son harmonique (un triangle à l'octave) part de 0,28 et tombe à 0,001 en 45 % de la durée.
const ATTAQUE: f64 = 0.012;
const PLANCHER: f64 = 0.0001;
const ARRET: f64 = 0.05;
const HARMONIQUE: f64 = 0.28;
const HARMONIQUE_FIN: f64 = 0.001;
const HARMONIQUE_RAMPE: f64 = 0.45;
const HARMONIQUE_ARRET: f64 = 0.5;

/// Une note d'un événement : son degré (ajouté à celui de l'arpège), son volume, sa durée (s), son retard (s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteDEvenement {
    pub degre: i32,
    pub volume: f64,
    pub duree: f64,
    pub retard: f64,
}

const fn note(degre: i32, volume: f64, duree: f64, retard: f64) -> NoteDEvenement {
    NoteDEvenement { degre, volume, duree, retard }
}

/// Un outil part : deux notes qui montent.
const ALLER: [NoteDEvenement; 2] = [note(2, 0.07, 1.8, 0.0), note(4, 0.06, 2.0, 0.14)];
/// Un outil revient : la note se pose (celle de l'arpège, puis deux degrés plus haut, qui s'éteignent lentement).
const RETOUR: [NoteDEvenement; 2] = [note(0, 0.08, 3.0, 0.0), note(2, 0.05, 3.0, 0.09)];
/// Un outil revient en erreur : la même figure, une octave plus bas (la page ne distinguait pas l'erreur).
const ERREUR: [NoteDEvenement; 2] = [note(-5, 0.08, 3.0, 0.0), note(-3, 0.05, 3.0, 0.09)];
/// Une impulsion arrive : un accord, une octave sous l'arpège, égrené en 70 ms.
const ARRIVEE: [NoteDEvenement; 3] = [note(-5, 0.05, 3.6, 0.0), note(-3, 0.05, 3.6, 0.07), note(-1, 0.05, 3.6, 0.14)];

/// Ce qui fait sonner une note hors de l'arpège.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evenement {
    Aller,
    Retour,
    Erreur,
    Arrivee,
}

const SORTES: [Evenement; 4] = [Evenement::Aller, Evenement::Retour, Evenement::Erreur, Evenement::Arrivee];

/// Les notes d'un événement.
pub fn notes_de(e: Evenement) -> &'static [NoteDEvenement] {
    match e {
        Evenement::Aller => &ALLER,
        Evenement::Retour => &RETOUR,
        Evenement::Erreur => &ERREUR,
        Evenement::Arrivee => &ARRIVEE,
    }
}

// ---- Les tailles --------------------------------------------------------------------------------------------------

/// L'arpège, les notes en attente et la coupure du filtre se suivent par sous-blocs de 32 échantillons (0,67 ms) :
/// un départ de note décalé d'autant ne s'entend pas.
const SOUS_BLOC: usize = 32;
/// Les notes à la fois : l'arpège en tient quatre au plus ; un outil en ajoute dix (l'aller, son accord, le retour, le
/// sien) en trois secondes ; au-delà, la plus ancienne cède sa place.
const MAX_NOTES: usize = 64;
const MAX_ATTENTE: usize = 32;
/// La réverbération se calcule à 12 kHz environ (un échantillon sur quatre à 48 kHz) : les notes n'ont presque rien
/// au-dessus de 5 kHz, et la convolution coûte quatre fois moins. Par blocs de 256 échantillons (21 ms de retard, un
/// pré-délai naturel pour une réverbération).
const CADENCE_REVERB: f64 = 12_000.0;
const BLOC_REVERB: usize = 256;
/// Le rappel calcule par tranches de cette taille au plus (des tampons alloués à l'ouverture).
const TRANCHE: usize = 2048;

// ---- Le hasard ----------------------------------------------------------------------------------------------------

/// Le hasard de l'arpège et du bruit de la réverbération (SplitMix64) : sans allocation ni appel au système.
struct Hasard(u64);

impl Hasard {
    fn suivant(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniforme dans [0, 1), comme `Math.random()`.
    fn uniforme(&mut self) -> f64 {
        (self.suivant() >> 11) as f64 / (1u64 << 53) as f64
    }
}

// ---- Les filtres --------------------------------------------------------------------------------------------------

/// Un biquad (la forme transposée directe II), en f64 : une coupure basse devant la cadence reste juste.
#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn passe_bas(frequence: f64, q: f64, taux: f64) -> Biquad {
        let mut b = Biquad::default();
        b.regler(frequence, q, taux);
        b
    }

    /// Un passe-bas (le livre de recettes de Robert Bristow-Johnson, que suit WebAudio) ; `q` linéaire. L'état reste :
    /// la coupure peut glisser sans clic.
    fn regler(&mut self, frequence: f64, q: f64, taux: f64) {
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
    fn filtrer(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn vider(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    /// Le gain à une fréquence (pour les essais).
    #[cfg(test)]
    fn gain(&self, frequence: f64, taux: f64) -> f64 {
        let w = TAU * frequence / taux;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let (nr, ni) = (self.b0 + self.b1 * c1 + self.b2 * c2, -(self.b1 * s1 + self.b2 * s2));
        let (dr, di) = (1.0 + self.a1 * c1 + self.a2 * c2, -(self.a1 * s1 + self.a2 * s2));
        ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt()
    }
}

/// Le Q linéaire d'un passe-bas de WebAudio, qui le donne en décibels (la spécification de `BiquadFilterNode`).
fn q_de_webaudio(q_db: f64) -> f64 {
    10f64.powf(q_db / 20.0)
}

/// Un passe-bas de Butterworth d'ordre 6 (trois biquads) : avant et après la réverbération, qui se calcule à une
/// cadence réduite — contre le repliement à l'entrée, contre les images à la sortie (−33 dB et plus bas).
fn butterworth(frequence: f64, taux: f64) -> [Biquad; 3] {
    // Les Q des trois sections : 1 / (2 sin((2k − 1) π / 12)).
    [1.0, 3.0, 5.0].map(|k: f64| Biquad::passe_bas(frequence, 1.0 / (2.0 * (k * PI / 12.0).sin()), taux))
}

// ---- La transformée de Fourier ------------------------------------------------------------------------------------

/// Une transformée de Fourier rapide complexe (radix 2, en place), de taille fixe : ses tables se font une fois.
pub struct Fft {
    n: usize,
    cos: Vec<f32>,
    sin: Vec<f32>,
    ordre: Vec<u32>,
}

impl Fft {
    /// `n` : une puissance de deux, 2 au moins.
    pub fn nouvelle(n: usize) -> Fft {
        assert!(n.is_power_of_two() && n >= 2, "une FFT de {n} points");
        let bits = n.trailing_zeros();
        let ordre = (0..n as u32).map(|i| i.reverse_bits() >> (32 - bits)).collect();
        let angle = |k: usize| TAU * k as f64 / n as f64;
        let cos = (0..n / 2).map(|k| angle(k).cos() as f32).collect();
        let sin = (0..n / 2).map(|k| angle(k).sin() as f32).collect();
        Fft { n, cos, sin, ordre }
    }

    /// La transformée de `re` + i `im` (de longueur n), en place ; `inverse` : l'inverse, sans la division par n.
    pub fn transformer(&self, re: &mut [f32], im: &mut [f32], inverse: bool) {
        let n = self.n;
        let (re, im) = (&mut re[..n], &mut im[..n]);
        for i in 0..n {
            let j = self.ordre[i] as usize;
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let signe = if inverse { 1.0 } else { -1.0 };
        let mut taille = 2;
        while taille <= n {
            let (moitie, pas) = (taille / 2, n / taille);
            for debut in (0..n).step_by(taille) {
                for k in 0..moitie {
                    let (wr, wi) = (self.cos[k * pas], signe * self.sin[k * pas]);
                    let (a, b) = (debut + k, debut + k + moitie);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            taille *= 2;
        }
    }
}

// ---- La réverbération ---------------------------------------------------------------------------------------------

/// La réponse de la réverbération de la page, à la cadence `taux` : 3,5 s d'un bruit blanc qui s'éteint en
/// (1 − t/T)^2,5, ses deux canaux indépendants (la largeur), coupée en blocs entiers (la fin perdue est sous −170 dB).
/// Son énergie est celle que lui donnait WebAudio dans une page à `taux_page` : le `ConvolverNode` normalise sa réponse
/// à 0,00125 / sa puissance × 44 100 / la cadence, soit une énergie de (0,00125 × 44 100)² × 3,5 / la cadence par
/// canal — 0,22 à 48 kHz, un gain de 0,47 pour un son tenu.
fn reponse(taux_page: f64, taux: f64, bloc: usize, graine: u64) -> [Vec<f32>; 2] {
    let longueur = (DUREE_REVERB * taux).round().max(1.0) as usize;
    let utile = (longueur / bloc).max(1) * bloc;
    let mut hasard = Hasard(graine);
    let mut canaux = [vec![0.0f32; utile], vec![0.0f32; utile]];
    let mut somme = 0.0f64;
    for canal in &mut canaux {
        for (i, x) in canal.iter_mut().enumerate() {
            let v = (hasard.uniforme() * 2.0 - 1.0) * (1.0 - i as f64 / longueur as f64).max(0.0).powf(2.5);
            *x = v as f32;
            somme += v * v;
        }
    }
    let energie = (0.00125 * 44_100.0f64).powi(2) * DUREE_REVERB / taux_page;
    let echelle = if somme > 0.0 { (energie * 2.0 / somme).sqrt() } else { 0.0 };
    for canal in &mut canaux {
        for x in canal.iter_mut() {
            *x = (f64::from(*x) * echelle) as f32;
        }
    }
    canaux
}

/// La convolution par blocs (« overlap-save », la réponse en partitions égales), d'une entrée réelle par une réponse
/// stéréo : la gauche en partie réelle, la droite en partie imaginaire — une seule convolution complexe rend les deux
/// canaux. Une entrée muette depuis toute la longueur de la réponse ne coûte plus rien.
struct Convolueur {
    b: usize,
    n: usize,
    p: usize,
    /// Les spectres des partitions de la réponse, puis ceux des p dernières fenêtres d'entrée (`tete` : la dernière).
    h_re: Vec<f32>,
    h_im: Vec<f32>,
    x_re: Vec<f32>,
    x_im: Vec<f32>,
    tete: usize,
    /// Le bloc précédent, puis le bloc courant.
    fenetre: Vec<f32>,
    y_re: Vec<f32>,
    y_im: Vec<f32>,
    fft: Fft,
    /// Les blocs muets de suite : au-delà de p + 1, toutes les fenêtres retenues sont nulles, la sortie aussi.
    muets: usize,
}

impl Convolueur {
    fn nouveau(gauche: &[f32], droite: &[f32], b: usize) -> Convolueur {
        let n = 2 * b;
        let p = gauche.len().max(droite.len()).div_ceil(b).max(1);
        let fft = Fft::nouvelle(n);
        let (mut h_re, mut h_im) = (vec![0.0f32; p * n], vec![0.0f32; p * n]);
        for q in 0..p {
            let (re, im) = (&mut h_re[q * n..(q + 1) * n], &mut h_im[q * n..(q + 1) * n]);
            for j in 0..b {
                re[j] = gauche.get(q * b + j).copied().unwrap_or(0.0);
                im[j] = droite.get(q * b + j).copied().unwrap_or(0.0);
            }
            fft.transformer(re, im, false);
        }
        Convolueur {
            b,
            n,
            p,
            h_re,
            h_im,
            x_re: vec![0.0; p * n],
            x_im: vec![0.0; p * n],
            tete: 0,
            fenetre: vec![0.0; n],
            y_re: vec![0.0; n],
            y_im: vec![0.0; n],
            fft,
            muets: p + 2,
        }
    }

    fn muet(&self) -> bool {
        self.muets > self.p + 1
    }

    /// Un bloc de `b` échantillons d'entrée ; rend les `b` de sortie de chaque canal (avec un bloc de retard).
    fn bloc(&mut self, entree: &[f32], gauche: &mut [f32], droite: &mut [f32]) {
        let (b, n, p) = (self.b, self.n, self.p);
        let silencieux = entree[..b].iter().all(|x| x.abs() < 1e-9);
        self.muets = if silencieux { self.muets.saturating_add(1) } else { 0 };
        self.fenetre.copy_within(b..n, 0);
        if silencieux {
            self.fenetre[b..].fill(0.0);
        } else {
            self.fenetre[b..].copy_from_slice(&entree[..b]);
        }
        if self.muet() {
            gauche[..b].fill(0.0);
            droite[..b].fill(0.0);
            return;
        }
        self.tete = (self.tete + 1) % p;
        let case = self.tete * n;
        let (xr, xi) = (&mut self.x_re[case..case + n], &mut self.x_im[case..case + n]);
        xr.copy_from_slice(&self.fenetre);
        xi.fill(0.0);
        self.fft.transformer(xr, xi, false);
        let (yr, yi) = (&mut self.y_re[..n], &mut self.y_im[..n]);
        yr.fill(0.0);
        yi.fill(0.0);
        for q in 0..p {
            let (c, h) = (((self.tete + p - q) % p) * n, q * n);
            let (xr, xi) = (&self.x_re[c..c + n], &self.x_im[c..c + n]);
            let (hr, hi) = (&self.h_re[h..h + n], &self.h_im[h..h + n]);
            for k in 0..n {
                yr[k] += xr[k] * hr[k] - xi[k] * hi[k];
                yi[k] += xr[k] * hi[k] + xi[k] * hr[k];
            }
        }
        self.fft.transformer(yr, yi, true);
        let k = 1.0 / n as f32;
        for j in 0..b {
            gauche[j] = yr[b + j] * k;
            droite[j] = yi[b + j] * k;
        }
    }

    fn vider(&mut self) {
        self.x_re.fill(0.0);
        self.x_im.fill(0.0);
        self.fenetre.fill(0.0);
        self.muets = self.p + 2;
    }
}

/// La réverbération des notes : le bus passe à la cadence réduite (un passe-bas, puis un échantillon sur d), la
/// convolution, puis la cadence du périphérique (des zéros intercalés, le même passe-bas, le gain d).
struct Reverb {
    d: usize,
    compte: usize,
    avant: [Biquad; 3],
    apres: [[Biquad; 3]; 2],
    entree: Vec<f32>,
    sortie: [Vec<f32>; 2],
    k: usize,
    conv: Convolueur,
    /// Au repos : la convolution est muette, les filtres vidés ; un bus nul ne coûte rien.
    repos: bool,
}

impl Reverb {
    fn nouvelle(taux: f64, graine: u64) -> Reverb {
        let d = ((taux / CADENCE_REVERB).round() as usize).max(1);
        let reduit = taux / d as f64;
        let [g, dr] = reponse(taux, reduit, BLOC_REVERB, graine);
        let filtre = butterworth(0.42 * reduit, taux);
        Reverb {
            d,
            compte: 0,
            avant: filtre,
            apres: [filtre, filtre],
            entree: vec![0.0; BLOC_REVERB],
            sortie: [vec![0.0; BLOC_REVERB], vec![0.0; BLOC_REVERB]],
            k: 0,
            conv: Convolueur::nouveau(&g, &dr, BLOC_REVERB),
            repos: true,
        }
    }

    /// Un échantillon du bus ; rend la réverbération, à gauche et à droite.
    #[inline]
    fn pousser(&mut self, x: f64) -> (f64, f64) {
        if x == 0.0 && self.conv.muet() {
            if !self.repos {
                self.repos = true;
                for f in self.avant.iter_mut().chain(self.apres.iter_mut().flatten()) {
                    f.vider();
                }
            }
            return (0.0, 0.0);
        }
        self.repos = false;
        let d = self.d;
        let v = if d > 1 { self.avant.iter_mut().fold(x, |v, f| f.filtrer(v)) } else { x };
        let (mut g, mut dr) = (0.0, 0.0);
        if self.compte == 0 {
            // Un échantillon à la cadence réduite : la sortie du bloc précédent, et l'entrée du bloc courant.
            g = f64::from(self.sortie[0][self.k]) * d as f64;
            dr = f64::from(self.sortie[1][self.k]) * d as f64;
            self.entree[self.k] = v as f32;
            self.k += 1;
            if self.k == BLOC_REVERB {
                let [sg, sd] = &mut self.sortie;
                self.conv.bloc(&self.entree, sg, sd);
                self.k = 0;
            }
        }
        self.compte = (self.compte + 1) % d;
        if d > 1 {
            let [fg, fd] = &mut self.apres;
            g = fg.iter_mut().fold(g, |v, f| f.filtrer(v));
            dr = fd.iter_mut().fold(dr, |v, f| f.filtrer(v));
        }
        (g, dr)
    }

    fn vider(&mut self) {
        self.conv.vider();
        self.entree.fill(0.0);
        self.sortie.iter_mut().for_each(|s| s.fill(0.0));
        for f in self.avant.iter_mut().chain(self.apres.iter_mut().flatten()) {
            f.vider();
        }
        (self.k, self.compte, self.repos) = (0, 0, true);
    }
}

// ---- Les notes ----------------------------------------------------------------------------------------------------

/// Une note qui sonne (`pluck` de la page) : un sinus, et son harmonique, un triangle à l'octave.
#[derive(Clone, Copy, Default)]
struct Note {
    arpege: bool,
    t: u32,
    attaque: u32,
    fin_rampe: u32,
    fin: u32,
    fin_rampe_h: u32,
    fin_h: u32,
    volume: f64,
    env: f64,
    k_env: f64,
    /// Le sinus, par rotation (cos, sin) : sa fréquence ne change pas pendant la note.
    c: f64,
    s: f64,
    cr: f64,
    sr: f64,
    /// Le triangle (sa phase, décalée d'un quart : il part de zéro en montant, comme celui de WebAudio).
    psi: f64,
    inc_h: f64,
    env_h: f64,
    k_h: f64,
}

impl Note {
    fn nouvelle(frequence: f64, volume: f64, duree: f64, taux: f64, arpege: bool) -> Note {
        let ech = |s: f64| (s * taux).round().max(1.0) as u32;
        let attaque = ech(ATTAQUE);
        let fin_rampe = ech(duree).max(attaque + 1);
        let fin_rampe_h = ech(duree * HARMONIQUE_RAMPE);
        let (sr, cr) = (TAU * frequence / taux).sin_cos();
        Note {
            arpege,
            t: 0,
            attaque,
            fin_rampe,
            fin: ech(duree + ARRET),
            fin_rampe_h,
            fin_h: ech(duree * HARMONIQUE_ARRET),
            volume,
            env: volume,
            k_env: (PLANCHER / volume).powf(1.0 / f64::from(fin_rampe - attaque)),
            c: 1.0,
            s: 0.0,
            cr,
            sr,
            psi: 0.25,
            inc_h: 2.0 * frequence / taux,
            env_h: HARMONIQUE,
            k_h: (HARMONIQUE_FIN / HARMONIQUE).powf(1.0 / f64::from(fin_rampe_h)),
        }
    }

    /// Le gain de la note à son échantillon courant, puis le suivant : la rampe linéaire de l'attaque, l'exponentielle
    /// (`exponentialRampToValueAtTime`), le plancher jusqu'à l'arrêt.
    #[inline]
    fn gain(&mut self) -> f64 {
        let t = self.t;
        if t < self.attaque {
            self.volume * f64::from(t) / f64::from(self.attaque)
        } else if t < self.fin_rampe {
            let g = self.env;
            self.env *= self.k_env;
            g
        } else {
            PLANCHER
        }
    }

    /// L'échantillon suivant, et la note sonne-t-elle encore après lui.
    #[inline]
    fn suivant(&mut self) -> (f64, bool) {
        let g = self.gain();
        let mut x = self.s;
        if self.t < self.fin_h {
            let h = if self.t < self.fin_rampe_h {
                let h = self.env_h;
                self.env_h *= self.k_h;
                h
            } else {
                HARMONIQUE_FIN
            };
            x += h * (1.0 - 4.0 * (self.psi - 0.5).abs());
            self.psi = (self.psi + self.inc_h).fract();
        }
        let (c, s) = (self.c, self.s);
        self.c = c * self.cr - s * self.sr;
        self.s = c * self.sr + s * self.cr;
        self.t += 1;
        (g * x, self.t < self.fin)
    }
}

/// Une note d'événement qui attend son retard.
#[derive(Clone, Copy, Default)]
struct Attente {
    quand: u64,
    degre: i32,
    volume: f64,
    duree: f64,
}

// ---- Le synthétiseur ----------------------------------------------------------------------------------------------

/// Ce que la fenêtre demande à la musique ; le rappel la relit à chaque tampon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Consigne {
    /// La nappe et l'arpège (la touche A).
    pub musique: bool,
    /// Les notes des événements (la touche S).
    pub sons: bool,
    /// Eve travaille : l'arpège s'anime.
    pub travail: bool,
    /// Eve écoute, transcrit ou parle : tout se tait.
    pub silence: bool,
    pub nuit: bool,
    /// L'indice de la gamme dans [`GAMMES`].
    pub gamme: usize,
    /// Un gain de plus sur tout : pour l'essai de la marge (1 d'office).
    pub amplification: f64,
    /// Le programme se ferme : un fondu très court.
    pub fermeture: bool,
}

impl Default for Consigne {
    fn default() -> Consigne {
        Consigne {
            musique: true,
            sons: true,
            travail: false,
            silence: false,
            nuit: false,
            gamme: 0,
            amplification: 1.0,
            fermeture: false,
        }
    }
}

/// Le synthétiseur : la partie pure de la musique, sans périphérique — le rappel du son le fait avancer, les essais
/// aussi.
pub struct Synthe {
    taux: f64,
    horloge: u64,
    hasard: Hasard,
    consigne: Consigne,
    /// La première consigne fixe les gains sans fondu : une musique coupée au lancement ne se fait pas entendre en
    /// s'éteignant.
    premiere: bool,
    /// Les gains et leurs cibles, lissés comme `setTargetAtTime` (une exponentielle vers la cible).
    g_mus: f64,
    g_sons: f64,
    g_maitre: f64,
    g_etat: f64,
    cible_etat: f64,
    a_glisse: f64,
    a_silence: f64,
    a_fermeture: f64,
    a_ton: f64,
    /// Les échantillons de calme qu'il faut encore avant que la musique revienne.
    reprise: u64,
    /// La nappe : ses cinq fréquences (et leurs cibles), ses dix phases, son filtre, la phase du LFO.
    f: [f64; 5],
    cible_f: [f64; 5],
    phases: [f64; 10],
    pas: [f64; 2],
    filtre: Biquad,
    q: f64,
    lfo: f64,
    notes: [Note; MAX_NOTES],
    n_notes: usize,
    attente: [Attente; MAX_ATTENTE],
    n_attente: usize,
    degre: i32,
    prochaine: u64,
    reverb: Reverb,
    /// Les notes jouées et les tirages de l'arpège, pour les essais : (instant, degré, fréquence, volume, durée,
    /// arpège) et (instant, une note est-elle partie).
    #[cfg(test)]
    trace: Vec<(u64, i32, f64, f64, f64, bool)>,
    #[cfg(test)]
    tirages: Vec<(u64, bool)>,
}

impl Synthe {
    /// Tout ce que le rappel calculera s'alloue ici : la réverbération (1,4 Mo à 48 kHz), les notes, les tampons.
    pub fn nouveau(taux: u32, graine: u64) -> Synthe {
        let taux = f64::from(taux.max(8_000));
        let coefficient = |tau: f64| 1.0 - (-1.0 / (tau * taux)).exp();
        let mut hasard = Hasard(graine);
        let reverb = Reverb::nouvelle(taux, hasard.suivant());
        let cible_f = nappe_de(0);
        let premiere = (ARPEGE_REPOS.0 + hasard.uniforme() * ARPEGE_REPOS.1) * taux;
        let consigne = Consigne::default();
        let q = q_de_webaudio(Q_DB);
        Synthe {
            taux,
            horloge: 0,
            hasard,
            consigne,
            premiere: true,
            g_mus: 1.0,
            g_sons: 1.0,
            g_maitre: VOLUME * MAITRE_JOUR,
            // La page part du silence et monte en 0,6 s (`majAmb` à la fin d'`ambiance`).
            g_etat: 0.0,
            cible_etat: 1.0,
            a_glisse: coefficient(GLISSE),
            a_silence: coefficient(FONDU_SILENCE),
            a_fermeture: coefficient(FONDU_FERMETURE),
            a_ton: coefficient(GLISSE_TON),
            reprise: 0,
            f: cible_f,
            cible_f,
            // Les triangles partent de zéro en montant (la phase décalée d'un quart), les sinus de zéro.
            phases: [0.25, 0.25, 0.25, 0.25, 0.25, 0.25, 0.25, 0.25, 0.0, 0.0],
            pas: [-DESACCORD_CENTS, DESACCORD_CENTS].map(|c| 2f64.powf(c / 1200.0) / taux),
            filtre: Biquad::passe_bas(COUPURE, q, taux),
            q,
            lfo: 0.0,
            notes: [Note::default(); MAX_NOTES],
            n_notes: 0,
            attente: [Attente::default(); MAX_ATTENTE],
            n_attente: 0,
            degre: DEGRE_DEPART,
            prochaine: premiere.round() as u64,
            reverb,
            #[cfg(test)]
            trace: Vec::new(),
            #[cfg(test)]
            tirages: Vec::new(),
        }
    }

    fn echantillons(&self, secondes: f64) -> u64 {
        (secondes * self.taux).round().max(0.0) as u64
    }

    /// La consigne, à chaque tampon : les cibles changent, les gains y glissent.
    pub fn consigne(&mut self, c: &Consigne) {
        if c.gamme != self.consigne.gamme {
            self.cible_f = nappe_de(c.gamme);
        }
        self.consigne = Consigne { gamme: c.gamme.min(GAMMES.len() - 1), ..*c };
        if self.premiere {
            self.premiere = false;
            self.f = self.cible_f;
            self.g_mus = if c.musique { 1.0 } else { 0.0 };
            self.g_sons = if c.sons { 1.0 } else { 0.0 };
            self.g_maitre = Self::maitre(c);
        }
    }

    /// Le gain du maître que demande une consigne.
    fn maitre(c: &Consigne) -> f64 {
        VOLUME * c.amplification * if c.nuit { MAITRE_NUIT } else { MAITRE_JOUR }
    }

    /// Une note (ou un accord) peut-elle partir : ni silence, ni fermeture, ni l'attente d'après un silence.
    fn jouable(&self) -> bool {
        !self.consigne.silence && !self.consigne.fermeture && self.reprise == 0
    }

    /// Un événement : ses notes partent (celles qui ont un retard attendent leur tour, dans le sous-bloc où il échoit).
    pub fn evenement(&mut self, e: Evenement) {
        if !self.consigne.sons || !self.jouable() {
            return;
        }
        for n in notes_de(e) {
            if n.retard <= 0.0 {
                self.jouer(self.degre + n.degre, n.volume, n.duree, false);
            } else if self.n_attente < MAX_ATTENTE {
                let quand = self.horloge + self.echantillons(n.retard);
                self.attente[self.n_attente] = Attente { quand, degre: n.degre, volume: n.volume, duree: n.duree };
                self.n_attente += 1;
            }
        }
    }

    fn jouer(&mut self, degre: i32, volume: f64, duree: f64, arpege: bool) {
        let f = frequence(&GAMMES[self.consigne.gamme], degre);
        let note = Note::nouvelle(f, volume, duree, self.taux, arpege);
        if self.n_notes < MAX_NOTES {
            self.notes[self.n_notes] = note;
            self.n_notes += 1;
        } else if let Some(k) = (0..MAX_NOTES).max_by_key(|&k| self.notes[k].t) {
            self.notes[k] = note;
        }
        #[cfg(test)]
        self.trace.push((self.horloge, degre, f, volume, duree, arpege));
    }

    /// Le silence et le retour : la cible du gain d'état, au début de chaque sous-bloc.
    fn suivre_l_etat(&mut self, m: usize) {
        if self.consigne.silence || self.consigne.fermeture {
            self.reprise = self.echantillons(REPRISE);
            self.cible_etat = 0.0;
        } else if self.reprise > 0 {
            self.reprise = self.reprise.saturating_sub(m as u64);
            self.cible_etat = 0.0;
        } else {
            self.cible_etat = 1.0;
        }
    }

    /// L'arpège (ses tirages échus) et les notes d'événements dont le retard est passé.
    fn ordonnancer(&mut self) {
        let jouable = self.jouable();
        while self.horloge >= self.prochaine {
            let travail = self.consigne.travail;
            let tire = travail || self.hasard.uniforme() < ARPEGE_CHANCE;
            if tire {
                let pas = (self.hasard.uniforme() * 5.0).floor() as i32 - 2;
                self.degre = (self.degre + pas).clamp(0, DEGRE_MAX);
                if self.consigne.musique && jouable {
                    let volume = if travail { ARPEGE_VOLUME_TRAVAIL } else { ARPEGE_VOLUME };
                    self.jouer(self.degre, volume, ARPEGE_DUREE, true);
                }
            }
            #[cfg(test)]
            self.tirages.push((self.prochaine, tire));
            let (minimum, ecart) = if travail { ARPEGE_TRAVAIL } else { ARPEGE_REPOS };
            let intervalle = minimum + self.hasard.uniforme() * ecart;
            self.prochaine += self.echantillons(intervalle);
        }
        let mut i = 0;
        while i < self.n_attente {
            let a = self.attente[i];
            if a.quand > self.horloge {
                i += 1;
                continue;
            }
            self.n_attente -= 1;
            self.attente[i] = self.attente[self.n_attente];
            if self.consigne.sons && jouable {
                self.jouer(self.degre + a.degre, a.volume, a.duree, false);
            }
        }
    }

    /// Un échantillon de la nappe, filtré.
    #[inline]
    fn nappe(&mut self) -> f64 {
        let mut x = 0.0;
        for k in 0..5 {
            self.f[k] += (self.cible_f[k] - self.f[k]) * self.a_ton;
            for d in 0..2 {
                let p = &mut self.phases[2 * k + d];
                *p += self.f[k] * self.pas[d];
                if *p >= 1.0 {
                    *p -= 1.0;
                }
                x += if k == 4 { (TAU * *p).sin() } else { 1.0 - 4.0 * (*p - 0.5).abs() };
            }
        }
        self.filtre.filtrer(x * GAIN_NAPPE)
    }

    /// Rend `gauche.len()` échantillons stéréo.
    pub fn rendre(&mut self, gauche: &mut [f32], droite: &mut [f32]) {
        let n = gauche.len().min(droite.len());
        let c = self.consigne;
        let cible_mus = if c.musique { 1.0 } else { 0.0 };
        let cible_sons = if c.sons { 1.0 } else { 0.0 };
        let cible_maitre = Self::maitre(&c);
        let mut i = 0;
        while i < n {
            let m = (n - i).min(SOUS_BLOC);
            self.suivre_l_etat(m);
            self.ordonnancer();
            // Le LFO, au début du sous-bloc : la coupure glisse de 0,1 Hz au plus d'un sous-bloc à l'autre.
            let coupure = COUPURE + BALAYAGE * (TAU * self.lfo).sin();
            self.filtre.regler(coupure, self.q, self.taux);
            self.lfo = (self.lfo + LFO * m as f64 / self.taux).fract();
            // Une nappe coupée (la touche A) ne se calcule plus, une fois son fondu fini.
            let avec_nappe = c.musique || self.g_mus > 1e-7;
            if !avec_nappe {
                self.f = self.cible_f;
                self.filtre.vider();
            }
            let a_etat = if self.cible_etat >= self.g_etat {
                self.a_glisse
            } else if c.fermeture {
                self.a_fermeture
            } else {
                self.a_silence
            };
            for j in i..i + m {
                self.g_mus += (cible_mus - self.g_mus) * self.a_glisse;
                self.g_sons += (cible_sons - self.g_sons) * self.a_glisse;
                self.g_maitre += (cible_maitre - self.g_maitre) * self.a_glisse;
                self.g_etat += (self.cible_etat - self.g_etat) * a_etat;
                let nappe = if avec_nappe { self.nappe() } else { 0.0 };
                let (mut arpege, mut evenements) = (0.0, 0.0);
                let mut k = 0;
                while k < self.n_notes {
                    let (x, vivante) = self.notes[k].suivant();
                    if self.notes[k].arpege {
                        arpege += x;
                    } else {
                        evenements += x;
                    }
                    if vivante {
                        k += 1;
                    } else {
                        self.n_notes -= 1;
                        self.notes[k] = self.notes[self.n_notes];
                    }
                }
                let bus = BUS * (self.g_mus * arpege + self.g_sons * evenements);
                let (rg, rd) = self.reverb.pousser(bus);
                let sec = self.g_mus * nappe + bus;
                let maitre = self.g_maitre * self.g_etat;
                gauche[j] = (maitre * (sec + HUMIDE * rg)).clamp(-1.0, 1.0) as f32;
                droite[j] = (maitre * (sec + HUMIDE * rd)).clamp(-1.0, 1.0) as f32;
            }
            self.horloge += m as u64;
            i += m;
        }
        // Le silence est complet (−120 dB) : les notes qui restaient s'oublient, la réverbération se vide — rien ne
        // reprendra au milieu d'une queue figée.
        if self.g_etat < 1e-6 && self.cible_etat == 0.0 && (self.n_notes > 0 || !self.reverb.repos) {
            self.n_notes = 0;
            self.n_attente = 0;
            self.reverb.vider();
        }
    }
}

// ---- La sortie audio ----------------------------------------------------------------------------------------------

/// Ce que la fenêtre dit au rappel du son, et ce que le rappel mesure : des atomiques seulement (le rappel ne prend
/// jamais de verrou).
struct Partage {
    musique: AtomicBool,
    sons: AtomicBool,
    travail: AtomicBool,
    silence: AtomicBool,
    nuit: AtomicBool,
    fermeture: AtomicBool,
    gamme: AtomicUsize,
    /// Les bits d'un f32.
    amplification: AtomicU32,
    /// Les événements demandés depuis l'ouverture, par sorte (dans l'ordre de [`SORTES`]) : le rappel compare ces
    /// compteurs à ce qu'il a déjà joué.
    evenements: [AtomicU32; 4],
    /// Une erreur du flux qui demande de le rouvrir (l'appareil débranché, la sortie par défaut changée).
    panne: AtomicBool,
    accrocs: AtomicU64,
    trames: AtomicU64,
    calcul_ns: AtomicU64,
    /// La pire charge d'un tampon (le temps de calcul sur sa durée), en bits de f32.
    pire: AtomicU32,
    /// La somme des carrés de la sortie (bits de f64), sa crête et celle du dernier tampon (bits de f32).
    energie: AtomicU64,
    crete: AtomicU32,
    dernier: AtomicU32,
}

impl Partage {
    fn nouveau() -> Partage {
        Partage {
            musique: AtomicBool::new(true),
            sons: AtomicBool::new(true),
            travail: AtomicBool::new(false),
            silence: AtomicBool::new(false),
            nuit: AtomicBool::new(false),
            fermeture: AtomicBool::new(false),
            gamme: AtomicUsize::new(0),
            amplification: AtomicU32::new(1.0f32.to_bits()),
            evenements: Default::default(),
            panne: AtomicBool::new(false),
            accrocs: AtomicU64::new(0),
            trames: AtomicU64::new(0),
            calcul_ns: AtomicU64::new(0),
            pire: AtomicU32::new(0),
            energie: AtomicU64::new(0),
            crete: AtomicU32::new(0),
            dernier: AtomicU32::new(0),
        }
    }

    fn consigne(&self) -> Consigne {
        Consigne {
            musique: self.musique.load(Relaxed),
            sons: self.sons.load(Relaxed),
            travail: self.travail.load(Relaxed),
            silence: self.silence.load(Relaxed),
            nuit: self.nuit.load(Relaxed),
            gamme: self.gamme.load(Relaxed),
            amplification: f64::from(f32::from_bits(self.amplification.load(Relaxed))),
            fermeture: self.fermeture.load(Relaxed),
        }
    }

    /// Les événements arrivés depuis `vus`, par sorte (et `vus` à jour).
    fn nouveaux(&self, vus: &mut [u32; 4]) -> [u32; 4] {
        let mut n = [0; 4];
        for k in 0..4 {
            let total = self.evenements[k].load(Relaxed);
            n[k] = total.wrapping_sub(vus[k]);
            vus[k] = total;
        }
        n
    }

    fn mesurer(&self, trames: usize, calcul: Duration, energie: f64, crete: f32, taux: u32) {
        self.trames.fetch_add(trames as u64, Relaxed);
        self.calcul_ns.fetch_add(calcul.as_nanos() as u64, Relaxed);
        if trames > 0 {
            let charge = (calcul.as_secs_f64() * f64::from(taux) / trames as f64) as f32;
            self.pire.fetch_max(charge.to_bits(), Relaxed);
        }
        // Un seul écrivain (le rappel) : lire, ajouter, écrire suffit.
        let e = f64::from_bits(self.energie.load(Relaxed)) + energie;
        self.energie.store(e.to_bits(), Relaxed);
        // Des f32 positifs s'ordonnent comme leurs bits.
        self.crete.fetch_max(crete.to_bits(), Relaxed);
        self.dernier.store(crete.to_bits(), Relaxed);
    }
}

/// Ce que la musique a joué et coûté depuis l'ouverture de sa sortie : pour les essais.
#[derive(Clone, Copy, Debug, Default)]
pub struct Releve {
    pub trames: u64,
    pub calcul: Duration,
    /// La pire charge d'un tampon : son temps de calcul sur sa durée (1 : le rappel n'aurait pas tenu).
    pub pire_charge: f32,
    /// La somme des carrés (la moyenne des deux canaux) et la crête.
    pub energie: f64,
    pub crete: f32,
    pub accrocs: u64,
}

/// La musique du centre de commande : sa sortie audio, et ce que la fenêtre lui dit.
pub struct Musique {
    flux: Option<cpal::Stream>,
    partage: Arc<Partage>,
    /// La sortie audio (pour le journal et les essais) et sa cadence.
    pub nom: String,
    pub taux: u32,
    tentative: Instant,
    echecs: u32,
}

impl Musique {
    /// Ouvre la sortie audio par défaut ; None s'il n'y en a pas (et une ligne au journal). La musique et les sons sont
    /// allumés, comme dans la page.
    pub fn nouvelle() -> Option<Musique> {
        let partage = Arc::new(Partage::nouveau());
        match ouvrir(&partage) {
            Ok((flux, nom, taux)) => {
                journal::ecrire(&format!("musique : {nom}, {taux} Hz"));
                Some(Musique { flux: Some(flux), partage, nom, taux, tentative: Instant::now(), echecs: 0 })
            }
            Err(e) => {
                journal::ecrire(&format!("musique : {e} ; le centre de commande reste muet"));
                None
            }
        }
    }

    /// La musique (la nappe, l'arpège) et les sons (les notes des événements), allumés ou non (touches A et S). Dans la
    /// page, A comme S coupaient tout (les notes passaient par la musique, et les bips qui devaient les remplacer ne
    /// sonnaient plus) : `regler(a && s, a && s)` en reprend le geste ; `regler(a, s)` sépare les deux, comme la page
    /// le voulait.
    pub fn regler(&mut self, musique: bool, sons: bool) {
        self.reparer();
        self.partage.musique.store(musique, Relaxed);
        self.partage.sons.store(sons, Relaxed);
    }

    /// L'état d'Eve, aux noms de l'état publié : « reflexion » (elle travaille) rend l'arpège plus vif ; « ecoute »,
    /// « transcription » et « parole » font taire la musique (un fondu de 50 ms) — le micro ne doit pas l'entendre, et
    /// la voix d'Eve doit rester claire ; tout autre état : la nappe ordinaire. Ne coûte que deux écritures atomiques :
    /// on peut l'appeler à chaque image.
    pub fn etat(&mut self, etat: &str) {
        self.reparer();
        let (travail, silence) = sorte_d_etat(etat);
        self.partage.travail.store(travail, Relaxed);
        self.partage.silence.store(silence, Relaxed);
    }

    /// La gamme du sujet que dessine le réseau (« ecole », « piece », « lettre »… : les `GAMMES` du tableau) ; un
    /// autre sujet, ou « aucun », rend la gamme d'office.
    pub fn sujet(&mut self, sujet: &str) {
        self.reparer();
        self.partage.gamme.store(gamme_du_sujet(sujet), Relaxed);
    }

    /// La nuit (de 21 h à 6 h) : plus bas (0,45 au lieu de 0,855, comme la page).
    pub fn nuit(&mut self, oui: bool) {
        self.reparer();
        self.partage.nuit.store(oui, Relaxed);
    }

    /// Un outil part : deux notes qui montent.
    pub fn aller(&mut self) {
        self.signaler(Evenement::Aller);
    }

    /// Un outil revient : une note qui se pose ; une erreur, une octave plus bas.
    pub fn retour(&mut self, erreur: bool) {
        self.signaler(if erreur { Evenement::Erreur } else { Evenement::Retour });
    }

    /// Une impulsion arrive au bout du réseau : l'accord.
    pub fn arrivee(&mut self) {
        self.signaler(Evenement::Arrivee);
    }

    /// Un gain de plus sur toute la musique (1 d'office) : pour mesurer la marge du détecteur de parole.
    pub fn amplifier(&mut self, gain: f32) {
        self.partage.amplification.store(gain.max(0.0).to_bits(), Relaxed);
    }

    /// Ce que la musique a joué et coûté depuis l'ouverture de sa sortie.
    pub fn releve(&self) -> Releve {
        let p = &self.partage;
        Releve {
            trames: p.trames.load(Relaxed),
            calcul: Duration::from_nanos(p.calcul_ns.load(Relaxed)),
            pire_charge: f32::from_bits(p.pire.load(Relaxed)),
            energie: f64::from_bits(p.energie.load(Relaxed)),
            crete: f32::from_bits(p.crete.load(Relaxed)),
            accrocs: p.accrocs.load(Relaxed),
        }
    }

    fn signaler(&mut self, e: Evenement) {
        self.reparer();
        if let Some(k) = SORTES.iter().position(|&s| s == e) {
            self.partage.evenements[k].fetch_add(1, Relaxed);
        }
    }

    /// Une sortie tombée en panne (débranchée, ou la sortie par défaut de Windows a changé) se rouvre, au plus toutes
    /// les 5 s, au prochain geste de la fenêtre.
    fn reparer(&mut self) {
        if !self.partage.panne.load(Relaxed) || self.tentative.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.tentative = Instant::now();
        self.flux = None;
        self.partage.panne.store(false, Relaxed);
        match ouvrir(&self.partage) {
            Ok((flux, nom, taux)) => {
                journal::ecrire(&format!("musique : la sortie se rouvre sur {nom}, {taux} Hz"));
                (self.flux, self.nom, self.taux, self.echecs) = (Some(flux), nom, taux, 0);
            }
            Err(e) => {
                self.partage.panne.store(true, Relaxed);
                self.echecs += 1;
                if self.echecs == 1 {
                    journal::ecrire(&format!("musique : {e} ; elle se tait jusqu'au retour d'une sortie"));
                }
            }
        }
    }
}

impl Drop for Musique {
    /// Un flux coupé net claque : la musique qui joue encore s'éteint d'abord en 150 ms.
    fn drop(&mut self) {
        if self.flux.is_some() && f32::from_bits(self.partage.dernier.load(Relaxed)) > 1e-4 {
            self.partage.fermeture.store(true, Relaxed);
            thread::sleep(Duration::from_millis(150));
        }
    }
}

/// Une graine différente à chaque lancement : la mélodie de l'arpège ne se répète pas d'un jour à l'autre.
fn graine() -> u64 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    (nanos as u64) ^ 0x5EED_E7E0_2026_1023
}

/// La sortie par défaut de Windows, et le flux qui la nourrit.
fn ouvrir(partage: &Arc<Partage>) -> Result<(cpal::Stream, String, u32), String> {
    let appareil = cpal::default_host().default_output_device().ok_or("aucune sortie audio")?;
    let nom = appareil.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "sortie audio".into());
    let cfg = appareil.default_output_config().map_err(|e| format!("la sortie audio ne dit pas son format : {e}"))?;
    let (canaux, taux) = (usize::from(cfg.channels()).max(1), cfg.sample_rate());
    let synthe = Synthe::nouveau(taux, graine());
    let (config, p) = (cfg.config(), partage);
    let flux = match cfg.sample_format() {
        SampleFormat::F32 => construire::<f32>(&appareil, config, canaux, synthe, p, |x| x),
        SampleFormat::I16 => construire::<i16>(&appareil, config, canaux, synthe, p, vers_i16),
        SampleFormat::U16 => construire::<u16>(&appareil, config, canaux, synthe, p, vers_u16),
        SampleFormat::I32 => construire::<i32>(&appareil, config, canaux, synthe, p, vers_i32),
        autre => return Err(format!("format de sortie non pris en charge : {autre:?}")),
    }?;
    flux.play().map_err(|e| format!("la sortie audio ne démarre pas : {e}"))?;
    Ok((flux, nom, taux))
}

fn vers_i16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * 32_767.0).round() as i16
}
fn vers_u16(x: f32) -> u16 {
    ((x.clamp(-1.0, 1.0) + 1.0) * 32_767.5).round() as u16
}
fn vers_i32(x: f32) -> i32 {
    (f64::from(x.clamp(-1.0, 1.0)) * 2_147_483_647.0).round() as i32
}

/// Le fil du son : les nombres dénormaux arrondis à zéro (une queue de réverbération qui s'éteint en ferait, cent fois
/// plus lents), et une priorité au-dessus des autres fils, pour qu'un processeur occupé ne la hache pas.
fn preparer_le_fil() {
    crate::oreille::arrondir_les_denormaux();
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST};
        // SAFETY : GetCurrentThread rend le pseudo-handle du fil courant (le fil du son de cpal), toujours valable ;
        // SetThreadPriority n'en change que la priorité.
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST) };
    }
}

fn construire<T>(
    appareil: &cpal::Device,
    config: cpal::StreamConfig,
    canaux: usize,
    mut synthe: Synthe,
    partage: &Arc<Partage>,
    conv: fn(f32) -> T,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + Send + 'static,
{
    let (p, p_erreur) = (partage.clone(), partage.clone());
    let taux = config.sample_rate;
    let (mut gauche, mut droite) = (vec![0.0f32; TRANCHE], vec![0.0f32; TRANCHE]);
    // Les événements déjà demandés avant cette ouverture (une réouverture) ne se rejouent pas.
    let mut vus = [0u32; 4];
    partage.nouveaux(&mut vus);
    let mut pret = false;
    appareil
        .build_output_stream::<T, _, _>(
            config,
            move |donnees: &mut [T], _: &cpal::OutputCallbackInfo| {
                let debut = Instant::now();
                if !pret {
                    pret = true;
                    preparer_le_fil();
                }
                synthe.consigne(&p.consigne());
                for (e, n) in SORTES.into_iter().zip(p.nouveaux(&mut vus)) {
                    // Une rafale (la fenêtre figée un moment) ne se rejoue pas en entier.
                    for _ in 0..n.min(4) {
                        synthe.evenement(e);
                    }
                }
                let trames = donnees.len() / canaux;
                let (mut energie, mut crete) = (0.0f64, 0.0f32);
                let mut fait = 0;
                while fait < trames {
                    let n = (trames - fait).min(TRANCHE);
                    synthe.rendre(&mut gauche[..n], &mut droite[..n]);
                    for i in 0..n {
                        let (g, d) = (gauche[i], droite[i]);
                        energie += f64::from(g * g + d * d) / 2.0;
                        crete = crete.max(g.abs()).max(d.abs());
                        let trame = &mut donnees[(fait + i) * canaux..(fait + i + 1) * canaux];
                        if canaux == 1 {
                            trame[0] = conv((g + d) * 0.5);
                        } else {
                            trame[0] = conv(g);
                            trame[1] = conv(d);
                            for x in &mut trame[2..] {
                                *x = conv(0.0);
                            }
                        }
                    }
                    fait += n;
                }
                p.mesurer(trames, debut.elapsed(), energie, crete, taux);
            },
            move |e: cpal::Error| {
                // Un accroc (un tampon servi en retard) se compte ; toute autre erreur demande de rouvrir la sortie.
                if matches!(e.kind(), cpal::ErrorKind::Xrun) {
                    p_erreur.accrocs.fetch_add(1, Relaxed);
                } else if !p_erreur.panne.swap(true, Relaxed) {
                    journal::ecrire(&format!("musique : {e} ; la sortie se rouvrira"));
                }
            },
            None,
        )
        .map_err(|e| format!("la sortie audio ne s'ouvre pas : {e}"))
}

#[cfg(test)]
mod essais {
    use super::*;

    const TAUX: u32 = 48_000;

    fn rendre(s: &mut Synthe, secondes: f64) -> (Vec<f32>, Vec<f32>) {
        let n = (secondes * f64::from(TAUX)) as usize;
        let (mut g, mut d) = (vec![0.0f32; n], vec![0.0f32; n]);
        for (cg, cd) in g.chunks_mut(480).zip(d.chunks_mut(480)) {
            s.rendre(cg, cd);
        }
        (g, d)
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / x.len().max(1) as f64).sqrt()
    }

    fn db(x: f64) -> f64 {
        20.0 * x.max(1e-12).log10()
    }

    #[test]
    fn les_gammes_du_tableau() {
        let d = &GAMMES[0];
        assert!((frequence(d, 0) - 293.66).abs() < 1e-9);
        assert!((frequence(d, 5) - 587.32).abs() < 1e-6, "le degré 5 : l'octave");
        assert!((frequence(d, -5) - 146.83).abs() < 1e-6, "le degré −5 : l'octave en dessous");
        assert!((frequence(d, 3) - 293.66 * 2f64.powf(7.0 / 12.0)).abs() < 1e-9, "le degré 3 : la quinte");
        assert!((frequence(&GAMMES[2], 1) - 220.0 * 2f64.powf(0.25)).abs() < 1e-9, "la tierce mineure de « piece »");
        assert_eq!(gamme_du_sujet("ecole"), 1);
        assert_eq!(gamme_du_sujet("repas"), 7);
        for autre in ["defaut", "maison", "aucun", "avions", ""] {
            assert_eq!(gamme_du_sujet(autre), 0, "{autre}");
        }
        for g in &GAMMES {
            assert!(g.intervalles.windows(2).all(|w| w[0] < w[1]) && g.intervalles[0] == 0 && g.intervalles[4] < 12);
        }
        assert_eq!(sorte_d_etat("reflexion"), (true, false));
        for e in ["ecoute", "transcription", "parole"] {
            assert_eq!(sorte_d_etat(e), (false, true), "{e}");
        }
        for e in ["pret", "demarrage", "confirmation", "erreur", "inconnu"] {
            assert_eq!(sorte_d_etat(e), (false, false), "{e}");
        }
    }

    /// L'enveloppe d'une note, échantillon par échantillon : la rampe de 12 ms, l'exponentielle de WebAudio (0,0001 au
    /// bout de la durée), l'arrêt 50 ms plus tard ; l'harmonique tombe à 0,001 en 45 % de la durée et se tait à 50 %.
    #[test]
    fn une_note_a_l_enveloppe_de_la_page() {
        let taux = f64::from(TAUX);
        let (volume, duree) = (0.06, 2.0);
        let mut n = Note::nouvelle(440.0, volume, duree, taux, false);
        let mut gains = Vec::new();
        let mut vivante = true;
        let mut echantillons = 0usize;
        while vivante {
            let mut copie = n;
            gains.push(copie.gain());
            vivante = n.suivant().1;
            echantillons += 1;
        }
        assert_eq!(echantillons, ((duree + ARRET) * taux).round() as usize, "l'arrêt 50 ms après la durée");
        let a = (ATTAQUE * taux).round() as usize;
        assert!((gains[a / 2] - volume / 2.0).abs() < 1e-9, "la rampe linéaire");
        assert!((gains[a] - volume).abs() < 1e-12, "le sommet au bout de l'attaque");
        // L'exponentielle : à mi-chemin (en temps) de la rampe, la moyenne géométrique du volume et du plancher.
        let milieu = (a + (duree * taux).round() as usize) / 2;
        let attendu = (volume * PLANCHER).sqrt();
        assert!((gains[milieu] / attendu - 1.0).abs() < 1e-3, "{} contre {attendu}", gains[milieu]);
        let fin = (duree * taux).round() as usize;
        assert!((gains[fin - 1] / PLANCHER - 1.0).abs() < 1e-3 && gains[fin] == PLANCHER);
        // L'harmonique (suivie à part) : 0,28 au départ, 0,001 à 45 % de la durée.
        let mut h = Note::nouvelle(440.0, volume, duree, taux, false);
        let k45 = (duree * 0.45 * taux).round() as usize;
        for _ in 0..k45 {
            h.suivant();
        }
        assert!((h.env_h / HARMONIQUE_FIN - 1.0).abs() < 1e-3, "{}", h.env_h);
        assert_eq!(h.fin_h as usize, (duree * 0.5 * taux).round() as usize);
    }

    /// Le son d'une note : un sinus à sa fréquence, qui ne dérive pas (la rotation reste sur le cercle), et son
    /// harmonique à l'octave, un triangle qui part de zéro en montant.
    #[test]
    fn une_note_sonne_juste() {
        let taux = f64::from(TAUX);
        let mut n = Note::nouvelle(1000.0, 0.05, 3.0, taux, true);
        let mut x = Vec::new();
        for _ in 0..(3.0 * taux) as usize {
            x.push(n.suivant().0);
        }
        // Les passages à zéro montants du sinus, sur la dernière seconde (l'harmonique s'y est tue) : 1000 par seconde.
        let derniere = &x[(2.0 * taux) as usize..];
        let montants = derniere.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((999..=1001).contains(&montants), "{montants}");
        let norme = (n.c * n.c + n.s * n.s).sqrt();
        assert!((norme - 1.0).abs() < 1e-9, "la rotation dérive : {norme}");
    }

    /// La réponse de la réverbération : 3,5 s, l'énergie que lui donnait WebAudio (0,22 par canal à 48 kHz), une
    /// enveloppe en (1 − t/T)^2,5, deux canaux indépendants.
    #[test]
    fn la_reverberation_de_la_page() {
        let [g, d] = reponse(48_000.0, 12_000.0, BLOC_REVERB, 7);
        assert_eq!(g.len(), 164 * BLOC_REVERB, "3,5 s à 12 kHz, en blocs entiers");
        let energie = |x: &[f32]| x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>();
        let attendue = (0.00125 * 44_100.0f64).powi(2) * 3.5 / 48_000.0;
        assert!((attendue - 0.2216).abs() < 1e-3);
        let moyenne = (energie(&g) + energie(&d)) / 2.0;
        assert!((moyenne / attendue - 1.0).abs() < 1e-6, "{moyenne}");
        // L'enveloppe : l'énergie de chaque dixième, contre l'intégrale de (1 − t/T)^5.
        let total = energie(&g);
        let dixieme = g.len() / 10;
        for k in [0usize, 3, 6] {
            let part = energie(&g[k * dixieme..(k + 1) * dixieme]) / total;
            let (a, b) = (k as f64 / 10.0, (k + 1) as f64 / 10.0);
            let theorie = (1.0 - a).powi(6) - (1.0 - b).powi(6);
            assert!((part / theorie - 1.0).abs() < 0.1, "le dixième {k} : {part} contre {theorie}");
        }
        let croise: f64 = g.iter().zip(&d).map(|(&a, &b)| f64::from(a) * f64::from(b)).sum();
        assert!(croise.abs() / total < 0.03, "deux canaux indépendants : {}", croise / total);
    }

    /// La réverbération a le niveau de celle de la page, la chaîne entière comprise (le passage à 12 kHz, la
    /// convolution, le retour à 48 kHz) : sa part d'énergie — que donne la différence des deux canaux, le son sec étant
    /// le même des deux côtés — est celle du `ConvolverNode`, l'énergie de sa réponse (0,22) fois 0,85² : −8 dB sous le
    /// son sec, en moyenne sur bien des notes (une note seule tombe sur un creux ou une bosse de la réponse).
    #[test]
    fn la_reverberation_a_le_niveau_de_la_page() {
        let mut s = Synthe::nouveau(TAUX, 31);
        s.consigne(&Consigne { musique: false, ..Consigne::default() });
        let (mut g, mut d) = (Vec::new(), Vec::new());
        for _ in 0..60 {
            s.evenement(Evenement::Arrivee);
            let (x, y) = rendre(&mut s, 1.5);
            g.extend(x);
            d.extend(y);
        }
        let (mut somme, mut humide) = (0.0, 0.0);
        for (&a, &b) in g.iter().zip(&d) {
            let (a, b) = (f64::from(a), f64::from(b));
            somme += ((a + b) / 2.0).powi(2);
            humide += (a - b).powi(2) / 2.0;
        }
        let sec = somme - humide / 2.0;
        let (ecart, attendu) = (10.0 * (humide / sec).log10(), 10.0 * (0.2216 * HUMIDE * HUMIDE).log10());
        assert!((ecart - attendu).abs() < 2.0, "{ecart:.1} dB contre {attendu:.1}");
    }

    #[test]
    fn la_fft_rend_la_transformee() {
        let n = 16;
        let fft = Fft::nouvelle(n);
        let x: Vec<f32> = (0..n).map(|i| ((i * 7 + 3) % 11) as f32 - 5.0).collect();
        let (mut re, mut im) = (x.clone(), vec![0.0f32; n]);
        fft.transformer(&mut re, &mut im, false);
        for k in 0..n {
            let (mut a, mut b) = (0.0f64, 0.0f64);
            for (j, &v) in x.iter().enumerate() {
                let w = -TAU * (j * k) as f64 / n as f64;
                a += f64::from(v) * w.cos();
                b += f64::from(v) * w.sin();
            }
            assert!((f64::from(re[k]) - a).abs() < 1e-3 && (f64::from(im[k]) - b).abs() < 1e-3, "{k}");
        }
        fft.transformer(&mut re, &mut im, true);
        for (j, &v) in x.iter().enumerate() {
            assert!((re[j] / n as f32 - v).abs() < 1e-4 && (im[j] / n as f32).abs() < 1e-4);
        }
    }

    /// La convolution par blocs rend la convolution directe, à un bloc de retard : la gauche par la réponse gauche, la
    /// droite par la droite.
    #[test]
    fn la_convolution_par_blocs_est_exacte() {
        let b = 16;
        let mut h = Hasard(3);
        let rg: Vec<f32> = (0..5 * b).map(|_| h.uniforme() as f32 - 0.5).collect();
        let rd: Vec<f32> = (0..5 * b).map(|_| h.uniforme() as f32 - 0.5).collect();
        let x: Vec<f32> = (0..20 * b).map(|i| if i < 9 * b { h.uniforme() as f32 - 0.5 } else { 0.0 }).collect();
        let mut c = Convolueur::nouveau(&rg, &rd, b);
        let (mut sg, mut sd) = (Vec::new(), Vec::new());
        let (mut og, mut od) = (vec![0.0f32; b], vec![0.0f32; b]);
        for bloc in x.chunks(b) {
            c.bloc(bloc, &mut og, &mut od);
            sg.extend_from_slice(&og);
            sd.extend_from_slice(&od);
        }
        let directe = |r: &[f32], i: usize| -> f32 { (0..r.len()).filter(|&j| j <= i).map(|j| r[j] * x[i - j]).sum() };
        for i in 0..x.len() {
            assert!((sg[i] - directe(&rg, i)).abs() < 1e-4, "gauche, {i}");
            assert!((sd[i] - directe(&rd, i)).abs() < 1e-4, "droite, {i}");
        }
        assert!(c.muet(), "une entrée muette depuis toute la réponse ne coûte plus rien");
        assert!(sg[sg.len() - b..].iter().all(|&v| v == 0.0));
    }

    /// Le passe-bas de la nappe : à sa coupure, le gain vaut son Q (0,6 dB lus comme WebAudio : 1,07) ; il laisse
    /// passer les fondamentales de la nappe et coupe l'aigu à 12 dB par octave.
    #[test]
    fn le_passe_bas_de_la_page() {
        let taux = f64::from(TAUX);
        let f = Biquad::passe_bas(900.0, q_de_webaudio(Q_DB), taux);
        assert!((f.gain(900.0, taux) - 1.0715).abs() < 1e-3, "{}", f.gain(900.0, taux));
        assert!((f.gain(146.83, taux) - 1.0).abs() < 0.05);
        let pente = db(f.gain(7200.0, taux)) - db(f.gain(3600.0, taux));
        assert!((-13.5..-11.0).contains(&pente), "{pente}");
        // Le Butterworth de la réverbération : −3 dB à sa coupure, plat dessous, −33 dB à l'image la plus proche.
        let bw = butterworth(5_040.0, taux);
        let g = |fr: f64| bw.iter().map(|b| b.gain(fr, taux)).product::<f64>();
        assert!((db(g(5_040.0)) + 3.01).abs() < 0.05 && db(g(1_000.0)).abs() < 0.01);
        assert!(db(g(9_500.0)) < -32.0, "{}", db(g(9_500.0)));
    }

    /// Le calendrier de l'arpège : au repos, un tirage toutes les 2,2 à 4 s et une note trois fois sur quatre ; au
    /// travail, toutes les 0,65 à 1,15 s, chaque fois ; le degré fait des pas de −2 à +2, sur deux octaves.
    #[test]
    fn le_calendrier_de_l_arpege() {
        let taux = f64::from(TAUX);
        for (travail, (minimum, ecart), chance) in [(false, ARPEGE_REPOS, 0.75), (true, ARPEGE_TRAVAIL, 1.0)] {
            let mut s = Synthe::nouveau(TAUX, 11);
            s.consigne(&Consigne { travail, ..Consigne::default() });
            // Une heure de calendrier, sans calculer le son : seulement les sous-blocs, comme `rendre` les suit.
            for _ in 0..(3600.0 * taux) as usize / SOUS_BLOC {
                s.suivre_l_etat(SOUS_BLOC);
                s.ordonnancer();
                s.horloge += SOUS_BLOC as u64;
            }
            // Le premier tirage se fixe à la création, avant la consigne : on le passe.
            let tirages: Vec<(u64, bool)> = s.tirages.iter().skip(1).copied().collect();
            assert!(tirages.len() > 900, "{}", tirages.len());
            for w in tirages.windows(2) {
                let dt = (w[1].0 - w[0].0) as f64 / taux;
                assert!(dt >= minimum - 1e-3 && dt <= minimum + ecart + 1e-3, "{dt} s");
            }
            let part = tirages.iter().filter(|t| t.1).count() as f64 / tirages.len() as f64;
            assert!((part - chance).abs() < 0.04, "travail {travail} : {part}");
            let notes: Vec<_> = s.trace.iter().filter(|t| t.5).collect();
            let volume = if travail { ARPEGE_VOLUME_TRAVAIL } else { ARPEGE_VOLUME };
            assert!(notes.iter().all(|t| (0..=DEGRE_MAX).contains(&t.1) && t.3 == volume && t.4 == ARPEGE_DUREE));
            assert!(notes.windows(2).all(|w| (w[1].1 - w[0].1).abs() <= 2), "des pas de −2 à +2");
            let degres: std::collections::HashSet<i32> = notes.iter().map(|t| t.1).collect();
            assert!(degres.len() >= 8, "l'arpège parcourt les deux octaves : {degres:?}");
        }
    }

    /// Les événements : deux notes qui montent à l'aller (140 ms entre elles), une qui se pose au retour, la même
    /// une octave plus bas en erreur, un accord égrené en 70 ms à l'arrivée ; rien quand les sons sont coupés.
    #[test]
    fn les_evenements_jouent_leurs_notes() {
        let taux = f64::from(TAUX);
        let cas: [(Evenement, &[(i32, f64, f64, f64)]); 4] = [
            (Evenement::Aller, &[(6, 0.07, 1.8, 0.0), (8, 0.06, 2.0, 0.14)]),
            (Evenement::Retour, &[(4, 0.08, 3.0, 0.0), (6, 0.05, 3.0, 0.09)]),
            (Evenement::Erreur, &[(-1, 0.08, 3.0, 0.0), (1, 0.05, 3.0, 0.09)]),
            (Evenement::Arrivee, &[(-1, 0.05, 3.6, 0.0), (1, 0.05, 3.6, 0.07), (3, 0.05, 3.6, 0.14)]),
        ];
        for (e, attendu) in cas {
            let mut s = Synthe::nouveau(TAUX, 5);
            s.consigne(&Consigne::default());
            rendre(&mut s, 0.5);
            let depart = s.horloge;
            s.evenement(e);
            // Un rendu court : l'arpège (au repos, 2,2 s au plus tôt) ne s'en mêle pas.
            rendre(&mut s, 0.3);
            let notes: Vec<_> = s.trace.iter().filter(|t| !t.5).collect();
            assert_eq!(notes.len(), attendu.len(), "{e:?}");
            for (t, &(degre, volume, duree, retard)) in notes.iter().zip(attendu) {
                assert_eq!((t.1, t.3, t.4), (degre, volume, duree), "{e:?}");
                assert!((t.2 - frequence(&GAMMES[0], degre)).abs() < 1e-9);
                let ecart = (t.0 - depart) as f64 / taux - retard;
                assert!((0.0..=SOUS_BLOC as f64 / taux).contains(&ecart), "{e:?} : {ecart}");
            }
        }
        let mut s = Synthe::nouveau(TAUX, 5);
        s.consigne(&Consigne { sons: false, ..Consigne::default() });
        s.evenement(Evenement::Arrivee);
        rendre(&mut s, 0.3);
        assert!(s.trace.is_empty(), "les sons coupés (S)");
    }

    /// La gamme du sujet : les notes la prennent tout de suite, la nappe y glisse en 2,5 s.
    #[test]
    fn la_gamme_suit_le_sujet() {
        let mut s = Synthe::nouveau(TAUX, 9);
        // La première consigne fixe tout sans glisser : le sujet change à la seconde.
        s.consigne(&Consigne::default());
        s.consigne(&Consigne { gamme: 1, ..Consigne::default() });
        s.evenement(Evenement::Retour);
        assert!((s.trace[0].2 - frequence(&GAMMES[1], 4)).abs() < 1e-9);
        rendre(&mut s, 2.5);
        let (depart, cible) = (293.66 / 2.0, 392.0 / 2.0);
        let fait = (s.f[0] - depart) / (cible - depart);
        assert!((fait - (1.0 - (-1.0f64).exp())).abs() < 0.01, "une constante de temps : {fait}");
    }

    /// Le niveau de la musique, au repos (la nappe et l'arpège lent) : celui de la page multiplié par [`VOLUME`].
    #[test]
    fn le_niveau_de_la_musique() {
        let mut s = Synthe::nouveau(TAUX, 21);
        s.consigne(&Consigne::default());
        let (g, d) = rendre(&mut s, 40.0);
        let debut = 10 * TAUX as usize;
        let niveau = db((rms(&g[debut..]).powi(2) / 2.0 + rms(&d[debut..]).powi(2) / 2.0).sqrt()) - db(VOLUME);
        // La page : environ −31 dBFS (la nappe seule en fait −32).
        assert!((-34.0..-28.0).contains(&niveau), "{niveau:.1} dBFS à VOLUME = 1");
    }

    /// La nuit baisse tout de 0,45 / 0,855 (−5,6 dB), comme la page : deux rendus du même hasard.
    #[test]
    fn la_nuit_baisse_la_musique() {
        let mut niveaux = Vec::new();
        for nuit in [false, true] {
            let mut s = Synthe::nouveau(TAUX, 4);
            s.consigne(&Consigne { nuit, ..Consigne::default() });
            let (g, _) = rendre(&mut s, 20.0);
            niveaux.push(rms(&g[5 * TAUX as usize..]));
        }
        let ecart = db(niveaux[1] / niveaux[0]);
        assert!((ecart - db(MAITRE_NUIT / MAITRE_JOUR)).abs() < 0.05, "{ecart}");
    }

    /// Eve écoute, transcrit ou parle : la musique perd 40 dB en un quart de seconde et 60 dB en 0,35 s, reste muette,
    /// et ne revient qu'après 1,2 s sans silence ; les événements, pendant le silence, ne sonnent pas.
    #[test]
    fn la_musique_se_tait_quand_eve_ecoute() {
        let taux = TAUX as usize;
        let mut s = Synthe::nouveau(TAUX, 13);
        s.consigne(&Consigne::default());
        let (avant, _) = rendre(&mut s, 8.0);
        let reference = rms(&avant[6 * taux..]);
        s.consigne(&Consigne { silence: true, ..Consigne::default() });
        s.evenement(Evenement::Aller);
        let (pendant, _) = rendre(&mut s, 3.0);
        let fenetre = |x: &[f32], t: f64| rms(&x[(t * taux as f64) as usize..((t + 0.01) * taux as f64) as usize]);
        for (t, plafond) in [(0.25, -38.0), (0.35, -58.0)] {
            let niveau = db(fenetre(&pendant, t) / reference);
            assert!(niveau < plafond, "à {t} s : {niveau:.1} dB");
        }
        assert!(pendant[taux..].iter().all(|&v| v.abs() < 1e-5), "le silence tient");
        s.consigne(&Consigne::default());
        let (apres, _) = rendre(&mut s, 6.0);
        assert!(apres[..(1.15 * taux as f64) as usize].iter().all(|&v| v.abs() < 1e-5), "1,2 s de calme d'abord");
        assert!(db(rms(&apres[4 * taux..]) / reference) > -6.0, "puis elle revient");
    }

    /// La musique coupée (A), les sons gardés (S) : la nappe et l'arpège se taisent, les événements sonnent.
    #[test]
    fn la_musique_et_les_sons_se_reglent_a_part() {
        let mut s = Synthe::nouveau(TAUX, 17);
        s.consigne(&Consigne { musique: false, ..Consigne::default() });
        let (g, _) = rendre(&mut s, 10.0);
        assert!(rms(&g[4 * TAUX as usize..]) < 1e-6, "ni nappe ni arpège");
        assert!(s.trace.is_empty());
        s.evenement(Evenement::Arrivee);
        let (g, _) = rendre(&mut s, 1.0);
        assert!(rms(&g) > 1e-3, "l'accord sonne seul");
    }

    #[test]
    fn les_formats_de_sortie() {
        assert_eq!((vers_i16(1.5), vers_i16(-1.5), vers_i16(0.0)), (32_767, -32_767, 0));
        assert_eq!((vers_u16(-1.0), vers_u16(0.0), vers_u16(1.0)), (0, 32_768, 65_535));
        assert_eq!((vers_i32(1.0), vers_i32(-2.0)), (2_147_483_647, -2_147_483_647));
    }

    /// Les compteurs d'événements : ce qui arrive entre deux tampons se joue une fois, et un compteur qui fait le tour
    /// ne trompe pas.
    #[test]
    fn les_evenements_passent_par_des_compteurs() {
        let p = Partage::nouveau();
        let mut vus = [0u32; 4];
        p.evenements[0].fetch_add(2, Relaxed);
        p.evenements[3].fetch_add(1, Relaxed);
        assert_eq!(p.nouveaux(&mut vus), [2, 0, 0, 1]);
        assert_eq!(p.nouveaux(&mut vus), [0; 4]);
        p.evenements[1].store(u32::MAX, Relaxed);
        vus[1] = u32::MAX - 1;
        assert_eq!(p.nouveaux(&mut vus)[1], 1);
        p.evenements[1].fetch_add(1, Relaxed);
        assert_eq!(p.nouveaux(&mut vus)[1], 1, "le tour du compteur");
    }
}
