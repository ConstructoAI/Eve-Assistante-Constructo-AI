//! Le studio : chaque phrase de la voix finie comme une narration enregistrée, avant de se dire. Le raccord
//! ([`super::raccord`]) retrouve les phrases dans le WAV du moteur et ôte le souffle de leurs bords ; le studio traite
//! alors chacune comme un ingénieur du son traite une voix — sa chaîne ([`Chaine`], celle de la voix qui parle), puis
//! sa mise à niveau ([`niveler`]) : sa sonie à la cible, ses crêtes vraies sous le plafond.
//!
//! Ce qu'il fallait à Jessica (Piper, `fr_FR-upmc-medium`), mesuré le 2026-10-27 sur trente réponses de la forme de
//! celles de l'Assistante, rendues comme Eve les dit (`eve --essai-son`), son spectre moyen par tiers d'octave comparé
//! à celui de la parole (ANSI S3.5, ramené au même total) : telle que Piper la rend, il lui manque 3 à 6 dB de 1,25 à
//! 2,5 kHz (là où s'entendent les voyelles et les consonnes), et elle a 2 à 3 dB de trop de 6 à 8 kHz (les
//! sifflantes, et le grain du modèle : des harmoniques métalliques jusqu'à 10 kHz). Le passe-bas de la douceur d'avant
//! (5 kHz, deux pôles) adoucissait ces aigus, mais creusait aussi tout ce qui est au-dessus de 1 kHz : de 1,25 à
//! 5 kHz, la voix était 4,6 dB sous la parole en moyenne (de 3,1 à 6,5 dB selon le tiers d'octave) — étouffée.
//!
//! Tout se fait sur la phrase entière, en mémoire : chaque étape peut regarder un peu en avant (une baisse se prépare
//! avant ce qui la demande), sans retard sur le son.

use std::f64::consts::FRAC_1_SQRT_2;

use crate::filtres::Biquad;

/// La sonie de chaque phrase (LUFS : la pondération K et les portes de BS.1770), la même pour toutes : une phrase
/// courte ne sonne pas plus fort qu'une longue, un « s » ne pèse pas plus qu'une voyelle. Celle de la voix d'avant, en
/// moyenne (-18,1 LUFS mesurés) : le volume que la personne a réglé reste le bon.
pub(super) const SONIE: f64 = -18.0;
/// Le plafond des crêtes vraies (dBTP : entre les échantillons aussi, là où les recrée le lecteur) : -1 dBTP, moins un
/// dixième — un autre mesureur interpole un peu autrement (mesuré : jusqu'à 0,01 dB de plus) ; et ce qu'une phrase
/// peut perdre ou gagner pour atteindre sa sonie.
const PLAFOND_DBTP: f64 = -1.1;
const BAISSE_MAX: f64 = 12.0;
const HAUSSE_MAX: f64 = 9.0;
/// Le limiteur : il prépare sa baisse avant la crête et la relâche après, sur ce temps.
const AVANCE_DU_LIMITEUR: f64 = 0.005;
const RELACHE: f64 = 0.040;

/// La chaîne d'une voix, avant sa mise à niveau : un passe-haut, deux corrections du timbre, le dé-esseur, le grain
/// des voyelles, le souffle des pauses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Chaine {
    /// Le passe-haut (Hz) : sous la voix (la fondamentale de Jessica descend vers 160 Hz), rien qu'un grondement.
    passe_haut: f64,
    /// La présence, rendue : une cloche (sa fréquence, son Q, son gain en dB).
    presence: (f64, f64, f64),
    /// Les aigus, tenus : un plateau haut (sa fréquence, son gain en dB).
    aigus: (f64, f64),
    sifflantes: Sifflantes,
    grain: Grain,
    souffle: Souffle,
}

/// Le souffle des pauses ([`calmer_le_souffle`]) : un expanseur vers le bas.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Souffle {
    /// Son seuil : tant de dB sous le niveau actif de la phrase.
    sous_la_voix: f32,
    /// Sous le seuil, chaque décibel de moins en ôte `rapport` (3 : l'écart triple).
    rapport: f32,
    /// La baisse, au plus (dB).
    baisse_max: f32,
}

/// Le grain des voyelles ([`adoucir_le_grain`]) : les aigus d'une voix voisée, baissés.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grain {
    /// La bande du grain : au-dessus de cette fréquence (Hz).
    frequence: f64,
    /// Sa baisse dans une voyelle (dB).
    baisse: f32,
}

/// Le dé-esseur ([`adoucir_les_sifflantes`]) : la bande des sifflantes baissée là seulement où elle dépasse.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Sifflantes {
    /// La bande des sifflantes : au-dessus de cette fréquence (Hz).
    frequence: f64,
    /// Son seuil, en dB par rapport au niveau actif de la phrase.
    seuil: f32,
    /// Au-dessus du seuil, un décibel de trop n'en garde que 1/rapport.
    rapport: f32,
    /// La baisse, au plus (dB).
    baisse_max: f32,
}

impl Chaine {
    /// La douceur 1, celle d'office : douce, mais claire. La présence rendue (+2,5 dB autour de 1,8 kHz : de 1,25 à
    /// 5 kHz, la voix n'est plus qu'à 1,8 dB sous la parole de référence en moyenne, contre 4,6 dB avec le passe-bas) ;
    /// les aigus tenus (-3 dB au-dessus de 6 kHz : le grain du modèle) ; les sifflantes fortes adoucies
    /// (au-dessus de 5 kHz, à plus de 18 dB sous le niveau actif de la phrase, 60 % de ce qui dépasse, 6 dB au plus) :
    /// les plus fortes reviennent au niveau qu'elles avaient sous le passe-bas, les autres gardent leur netteté ; le
    /// grain des voyelles, 4 dB plus bas : leurs aigus reviennent où le passe-bas les mettait (de 6 à 11 kHz, 42,9 dB
    /// sous la trame en médiane ; 39,3 sans lui, 43,5 sous le passe-bas), mais pas leur présence ; le souffle des
    /// pauses calmé (sous 28 dB du niveau actif, l'écart triplé, 15 dB au plus).
    pub(super) const DOUCE: Chaine = Chaine {
        passe_haut: 70.0,
        presence: (1800.0, 1.0, 2.5),
        aigus: (6000.0, -3.0),
        sifflantes: Sifflantes { frequence: 5000.0, seuil: -18.0, rapport: 2.5, baisse_max: 6.0 },
        grain: Grain { frequence: 6000.0, baisse: 4.0 },
        souffle: Souffle { sous_la_voix: 28.0, rapport: 3.0, baisse_max: 15.0 },
    };

    /// La douceur 2, plus douce encore : moins de présence, des aigus, des sifflantes et un grain plus bas.
    pub(super) const PLUS_DOUCE: Chaine = Chaine {
        passe_haut: 70.0,
        presence: (1800.0, 1.0, 1.5),
        aigus: (5000.0, -5.0),
        sifflantes: Sifflantes { frequence: 5000.0, seuil: -20.0, rapport: 3.0, baisse_max: 8.0 },
        grain: Grain { frequence: 6000.0, baisse: 6.0 },
        souffle: Souffle { sous_la_voix: 28.0, rapport: 3.0, baisse_max: 15.0 },
    };

    fn traiter(&self, x: &mut [f32], taux: f64) {
        filtrer(x, Biquad::passe_haut(self.passe_haut, FRAC_1_SQRT_2, taux));
        let (f, q, g) = self.presence;
        filtrer(x, Biquad::cloche(f, q, g, taux));
        let (f, g) = self.aigus;
        filtrer(x, Biquad::plateau_haut(f, FRAC_1_SQRT_2, g, taux));
        adoucir_les_sifflantes(x, taux, &self.sifflantes);
        adoucir_le_grain(x, taux, &self.grain);
        calmer_le_souffle(x, taux, &self.souffle);
    }
}

/// Ce que le studio fait d'une phrase de la voix (les échantillons de -1 à 1) : la chaîne de la voix, s'il y en a une,
/// puis sa mise à niveau.
pub(super) fn finir(x: &mut [f32], taux: u32, chaine: Option<&Chaine>) {
    if let Some(c) = chaine {
        c.traiter(x, f64::from(taux));
    }
    niveler(x, taux);
}

/// Le dé-esseur : la bande des sifflantes (sans déphasage, [`bande_haute`]) baissée là seulement où son niveau, sur
/// 4 ms, dépasse le seuil ; la baisse se prépare 2 ms avant (le « s » ne claque pas) et s'en va en 30 ms. Les voyelles
/// n'y montent jamais (leurs aigus restent 20 à 40 dB sous la phrase), ni la plupart des plosives.
fn adoucir_les_sifflantes(x: &mut [f32], taux: f64, s: &Sifflantes) {
    let Some(actif) = niveau_actif(x, taux) else { return };
    let haut = bande_haute(x, s.frequence, taux);
    let (seuil, pente) = (actif + s.seuil, 1.0 - 1.0 / s.rapport);
    let cible: Vec<f32> = niveaux_glissants(&haut, (0.004 * taux).round() as usize)
        .into_iter()
        .map(|l| (-(l - seuil).max(0.0) * pente).max(-s.baisse_max))
        .collect();
    let mut gain = a_venir(&cible, (0.002 * taux).round() as usize, false);
    lisser(&mut gain, taux, 0.0005, 0.030);
    for ((v, h), g) in x.iter_mut().zip(haut).zip(gain) {
        *v += h * (10f32.powf(g / 20.0) - 1.0);
    }
}

/// Le grain des voyelles adouci : la bande au-dessus de 6 kHz (sans déphasage, [`bande_haute`]) baissée là où la voix
/// est voisée — dans une voyelle, ces aigus ne sont que les harmoniques métalliques du modèle (sur 10 ms, la bande
/// haute à 25 dB ou plus sous le tout : toute la baisse) —, rien là où ils font la consonne (à 15 dB ou moins sous le
/// tout : un s, un ch, un f), entre les deux à proportion ; le gain lissé dans les deux sens (5 ms : sans retard). Un
/// juge automatique (UTMOS22) le préfère, sur chacune des trente réponses mesurées (+0,021 ± 0,002).
fn adoucir_le_grain(x: &mut [f32], taux: f64, grain: &Grain) {
    let haut = bande_haute(x, grain.frequence, taux);
    let n = (0.010 * taux).round() as usize;
    let mut gain: Vec<f32> = niveaux_glissants(&haut, n)
        .into_iter()
        .zip(niveaux_glissants(x, n))
        .map(|(h, t)| -grain.baisse * ((-15.0 - (h - t)) / 10.0).clamp(0.0, 1.0))
        .collect();
    for _ in 0..2 {
        lisser(&mut gain, taux, 0.005, 0.005);
        gain.reverse();
    }
    for ((v, h), g) in x.iter_mut().zip(haut).zip(gain) {
        *v += h * (10f32.powf(g / 20.0) - 1.0);
    }
}

/// Le souffle des pauses calmé : ce que Piper laisse aux virgules et entre les mots (un bruit 35 à 40 dB sous la
/// voix) baisse, la voix jamais — sous le seuil, chaque décibel de moins en ôte (rapport - 1) de plus, jusqu'à la
/// baisse au plus ; il s'ouvre 10 ms avant que la voix revienne (sa consonne garde son attaque) et se referme
/// lentement (80 ms : une fin de mot s'éteint d'elle-même). Le juge automatique le préfère (+0,030 ± 0,005, sur 26
/// des trente réponses).
fn calmer_le_souffle(x: &mut [f32], taux: f64, s: &Souffle) {
    let Some(actif) = niveau_actif(x, taux) else { return };
    let (seuil, n) = (actif - s.sous_la_voix, (0.010 * taux).round() as usize);
    let cible: Vec<f32> = niveaux_glissants(x, n)
        .into_iter()
        .map(|l| ((l - seuil) * (s.rapport - 1.0)).clamp(-s.baisse_max, 0.0))
        .collect();
    let mut gain = a_venir(&cible, n, true);
    lisser(&mut gain, taux, 0.080, 0.002);
    for (v, g) in x.iter_mut().zip(gain) {
        *v *= 10f32.powf(g / 20.0);
    }
}

/// La mise à niveau d'une phrase : un seul gain, qui l'amène à la [`SONIE`] (sa vie, ses appuis restent) ; puis un
/// limiteur, qui garde ses crêtes vraies sous [`PLAFOND_DBTP`] — il prépare sa baisse avant la crête et la relâche
/// après, en douceur. Sans voix (un silence), rien.
pub(super) fn niveler(x: &mut [f32], taux: u32) {
    let Some(sonie) = sonie(x, taux) else { return };
    let gain = 10f64.powf((SONIE - sonie).clamp(-BAISSE_MAX, HAUSSE_MAX) / 20.0) as f32;
    for v in x.iter_mut() {
        *v *= gain;
    }
    limiter(x, taux, 10f64.powf(PLAFOND_DBTP / 20.0) as f32);
}

/// La sonie d'un son (LUFS, BS.1770) : pondéré K (un plateau de +4 dB au-dessus de 1,5 kHz, comme l'oreille, et un
/// passe-haut à 38 Hz), mesuré par blocs de 400 ms tous les 100 ms ; les blocs sous -70 LUFS, puis ceux à plus de
/// 10 LU sous la moyenne des autres, ne comptent pas. Plus court qu'un bloc : le son entier en est un. `None` : rien
/// qui s'entende.
pub(super) fn sonie(x: &[f32], taux: u32) -> Option<f64> {
    let t = f64::from(taux);
    let mut plateau = Biquad::plateau_haut(1500.0, FRAC_1_SQRT_2, 4.0, t);
    let mut passe_haut = Biquad::passe_haut(38.0, 0.5, t);
    let carres: Vec<f64> = x
        .iter()
        .map(|&v| {
            let y = passe_haut.filtrer(plateau.filtrer(f64::from(v)));
            y * y
        })
        .collect();
    let cumul = cumuler(&carres);
    let (bloc, pas) = ((0.4 * t) as usize, (0.1 * t) as usize);
    let blocs: Vec<f64> = if x.len() < bloc {
        vec![cumul[x.len()] / x.len().max(1) as f64]
    } else {
        (0..=(x.len() - bloc) / pas).map(|k| (cumul[k * pas + bloc] - cumul[k * pas]) / bloc as f64).collect()
    };
    let lufs = |z: f64| -0.691 + 10.0 * z.max(1e-15).log10();
    let moyenne = |z: &[f64]| z.iter().sum::<f64>() / z.len() as f64;
    let audibles: Vec<f64> = blocs.into_iter().filter(|&z| lufs(z) > -70.0).collect();
    if audibles.is_empty() {
        return None;
    }
    let seuil = lufs(moyenne(&audibles)) - 10.0;
    let gardes: Vec<f64> = audibles.into_iter().filter(|&z| lufs(z) > seuil).collect();
    Some(lufs(moyenne(&gardes)))
}

/// Un filtre passé sur un son, en place.
fn filtrer(x: &mut [f32], mut b: Biquad) {
    for v in x.iter_mut() {
        *v = b.filtrer(f64::from(*v)) as f32;
    }
}

/// La bande haute d'un son (au-dessus de `frequence`), sans déphasage : un passe-haut de Butterworth du quatrième
/// ordre passé à l'endroit, puis à l'envers (la phrase est entière en mémoire). Le son moins elle est exactement sa
/// bande basse : baisser l'une ne touche pas l'autre — avec le déphasage d'un seul passage, la baisser ne baissait
/// rien (mesuré le 2026-10-26 : +0,2 dB d'aigus).
fn bande_haute(x: &[f32], frequence: f64, taux: f64) -> Vec<f32> {
    let mut y = x.to_vec();
    for _ in 0..2 {
        for q in [0.541_196_1, 1.306_563] {
            filtrer(&mut y, Biquad::passe_haut(frequence, q, taux));
        }
        y.reverse();
    }
    y
}

/// Le niveau (dB) de chaque échantillon : l'énergie moyenne d'une fenêtre de `n` échantillons centrée sur lui.
fn niveaux_glissants(x: &[f32], n: usize) -> Vec<f32> {
    let carres: Vec<f64> = x.iter().map(|&v| f64::from(v) * f64::from(v)).collect();
    let cumul = cumuler(&carres);
    (0..x.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(n / 2), (i + n - n / 2).min(x.len()));
            (10.0 * ((cumul[b] - cumul[a]) / (b - a).max(1) as f64).max(1e-15).log10()) as f32
        })
        .collect()
}

/// Ce qui vient : pour chaque échantillon, le plus grand (ou le plus petit) des `n` qui commencent à lui — une baisse
/// ou une ouverture se voit ainsi d'avance. (Une file des candidats : chaque échantillon y entre et en sort une fois.)
fn a_venir(x: &[f32], n: usize, plus_grand: bool) -> Vec<f32> {
    let mieux = |a: f32, b: f32| if plus_grand { a >= b } else { a <= b };
    let mut file: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    let mut out = vec![0.0f32; x.len()];
    for j in (0..x.len()).rev() {
        while file.back().is_some_and(|&k| mieux(x[j], x[k])) {
            file.pop_back();
        }
        file.push_back(j);
        while file.front().is_some_and(|&k| k >= j + n.max(1)) {
            file.pop_front();
        }
        out[j] = x[file.front().copied().unwrap_or(j)];
    }
    out
}

/// Un gain (dB) lissé : il descend en `baisse` secondes et remonte en `hausse` (des constantes de temps).
fn lisser(g: &mut [f32], taux: f64, baisse: f64, hausse: f64) {
    let (kb, kh) = ((-1.0 / (baisse * taux)).exp() as f32, (-1.0 / (hausse * taux)).exp() as f32);
    let mut courant = 0.0f32;
    for v in g.iter_mut() {
        let k = if *v < courant { kb } else { kh };
        courant = *v + k * (courant - *v);
        *v = courant;
    }
}

/// Le niveau actif d'un son (dBFS) : l'énergie moyenne de ses trames de 20 ms qui parlent (à 30 dB au plus de la plus
/// forte) — ses silences et son souffle n'y comptent pas.
fn niveau_actif(x: &[f32], taux: f64) -> Option<f32> {
    let t = ((taux * 0.020) as usize).max(1);
    let db: Vec<f64> = x
        .chunks(t)
        .map(|m| {
            10.0 * (m.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / m.len() as f64).max(1e-15).log10()
        })
        .collect();
    let fort = db.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let actives: Vec<f64> = db.into_iter().filter(|&v| v > (fort - 30.0).max(-50.0)).collect();
    if actives.is_empty() {
        return None;
    }
    let e = actives.iter().map(|&v| 10f64.powf(v / 10.0)).sum::<f64>() / actives.len() as f64;
    Some((10.0 * e.log10()) as f32)
}

/// Les sommes cumulées (la première, nulle) : la somme de toute fenêtre en une soustraction.
fn cumuler(x: &[f64]) -> Vec<f64> {
    let mut cumul = Vec::with_capacity(x.len() + 1);
    cumul.push(0.0);
    let mut s = 0.0;
    for v in x {
        s += v;
        cumul.push(s);
    }
    cumul
}

/// Les crêtes vraies : pour chaque échantillon, la plus grande valeur absolue du son entre lui et le suivant, tel que
/// le lecteur le recrée — l'échantillon, et trois points entre les deux, interpolés (un sinus cardinal fenêtré de
/// Lanczos sur douze échantillons, six de part et d'autre : le suréchantillonnage par quatre de BS.1770). Une
/// sifflante peut dépasser d'un ou deux décibels ses échantillons.
fn cretes_vraies(x: &[f32]) -> Vec<f32> {
    const DEMI: i64 = 6;
    let lanczos = |t: f64| {
        if t.abs() < 1e-9 {
            1.0
        } else if t.abs() >= DEMI as f64 {
            0.0
        } else {
            let pt = std::f64::consts::PI * t;
            (pt.sin() / pt) * ((pt / DEMI as f64).sin() / (pt / DEMI as f64))
        }
    };
    // Les coefficients des trois points (un quart, une moitié, trois quarts), chacun ramené à un gain de 1.
    let phases: Vec<Vec<f64>> = [0.25, 0.5, 0.75]
        .iter()
        .map(|&f| {
            let c: Vec<f64> = (1 - DEMI..=DEMI).map(|k| lanczos(f - k as f64)).collect();
            let somme: f64 = c.iter().sum();
            c.into_iter().map(|v| v / somme).collect()
        })
        .collect();
    let n = x.len() as i64;
    let a = |i: i64| if (0..n).contains(&i) { f64::from(x[i as usize]) } else { 0.0 };
    (0..n)
        .map(|i| {
            let mut crete = a(i).abs();
            for c in &phases {
                let v: f64 = c.iter().zip(1 - DEMI..=DEMI).map(|(c, k)| c * a(i + k)).sum();
                crete = crete.max(v.abs());
            }
            crete as f32
        })
        .collect()
}

/// Le limiteur des crêtes vraies : là où une crête dépasserait `plafond`, le son baisse juste ce qu'il faut — la
/// baisse vient peu à peu avant la crête ([`AVANCE_DU_LIMITEUR`]) et s'en va peu à peu après ([`RELACHE`]), jamais
/// au-dessus de ce qu'il faut. Loin des crêtes, rien ne change.
pub(super) fn limiter(x: &mut [f32], taux: u32, plafond: f32) {
    let cretes = cretes_vraies(x);
    if cretes.iter().all(|&c| c <= plafond) {
        return;
    }
    let avance = (f64::from(taux) * AVANCE_DU_LIMITEUR).round() as usize;
    let mut limite = vec![1.0f32; x.len()];
    for (i, &c) in cretes.iter().enumerate() {
        if c > plafond {
            let besoin = plafond / c;
            for l in &mut limite[i.saturating_sub(avance)..(i + avance + 1).min(x.len())] {
                *l = l.min(besoin);
            }
        }
    }
    // Lissée des deux côtés : la baisse vient avant la crête et s'en va après, sans jamais la dépasser.
    let r = 1.0 - (-1.0 / (f64::from(taux) * RELACHE)).exp() as f32;
    let mut avant = limite.clone();
    for i in 1..avant.len() {
        avant[i] = avant[i].min(avant[i - 1] + (1.0 - avant[i - 1]) * r);
    }
    for i in (0..limite.len().saturating_sub(1)).rev() {
        limite[i] = limite[i].min(limite[i + 1] + (1.0 - limite[i + 1]) * r);
    }
    for ((v, l), a) in x.iter_mut().zip(limite).zip(avant) {
        *v *= l.min(a);
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    const TAUX: u32 = 22_050;

    fn sinus(f: f64, db: f64, secondes: f64) -> Vec<f32> {
        let a = 10f64.powf(db / 20.0) * 2f64.sqrt();
        (0..(secondes * f64::from(TAUX)) as usize)
            .map(|k| (a * (std::f64::consts::TAU * f * k as f64 / f64::from(TAUX)).sin()) as f32)
            .collect()
    }

    /// Un sinus qui entre et sort en 5 ms : ses bords ne claquent pas (un claquement a des aigus).
    fn note(f: f64, db: f64, secondes: f64) -> Vec<f32> {
        let mut x = sinus(f, db, secondes);
        let (n, l) = ((0.005 * f64::from(TAUX)) as usize, x.len());
        for i in 0..n {
            let g = (0.5 - 0.5 * (std::f64::consts::PI * i as f64 / n as f64).cos()) as f32;
            x[i] *= g;
            x[l - 1 - i] *= g;
        }
        x
    }

    /// L'écart (dB) d'énergie entre deux sons.
    fn ecart(apres: &[f32], avant: &[f32]) -> f64 {
        let e = |v: &[f32]| v.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>();
        10.0 * (e(apres) / e(avant)).log10()
    }

    #[test]
    fn la_chaine_rend_la_presence_et_tient_les_aigus() {
        // Le timbre de la douceur 1 : rien sous la voix, le milieu intact, la présence plus haute, les aigus plus bas.
        let (c, t) = (Chaine::DOUCE, f64::from(TAUX));
        let filtres = [
            Biquad::passe_haut(c.passe_haut, FRAC_1_SQRT_2, t),
            Biquad::cloche(c.presence.0, c.presence.1, c.presence.2, t),
            Biquad::plateau_haut(c.aigus.0, FRAC_1_SQRT_2, c.aigus.1, t),
        ];
        let gain = |f: f64| filtres.iter().map(|b| 20.0 * b.gain(f, t).log10()).sum::<f64>();
        assert!(gain(30.0) < -10.0, "{}", gain(30.0));
        assert!(gain(500.0).abs() < 0.5, "{}", gain(500.0));
        assert!((gain(1800.0) - 2.5).abs() < 0.3, "{}", gain(1800.0));
        assert!(gain(9000.0) < -2.5, "{}", gain(9000.0));
        // Puis la mise à niveau : la phrase finit à la sonie du studio.
        let mut x = note(440.0, -25.0, 1.5);
        finir(&mut x, TAUX, Some(&Chaine::DOUCE));
        let s = sonie(&x, TAUX).unwrap();
        assert!((s - SONIE).abs() < 0.3, "{s}");
    }

    #[test]
    fn le_deesseur_n_adoucit_que_les_sifflantes_fortes() {
        // Une « voyelle » (300 Hz), un « s » fort (9 kHz, 4 dB sous elle), la voyelle, un « s » doux (24 dB sous
        // elle) : le fort baisse de sa baisse au plus (6 dB), ni le doux ni la voyelle ne bougent.
        let n = (0.15 * f64::from(TAUX)) as usize;
        let mut x = note(300.0, -16.0, 0.6);
        let fort = x.len();
        x.extend(note(9000.0, -20.0, 0.15));
        x.extend(note(300.0, -16.0, 0.6));
        let doux = x.len();
        x.extend(note(9000.0, -40.0, 0.15));
        x.extend(note(300.0, -16.0, 0.3));
        let avant = x.clone();
        adoucir_les_sifflantes(&mut x, f64::from(TAUX), &Chaine::DOUCE.sifflantes);
        let milieu = |a: usize| a + n / 4..a + 3 * n / 4;
        let (f, d) = (ecart(&x[milieu(fort)], &avant[milieu(fort)]), ecart(&x[milieu(doux)], &avant[milieu(doux)]));
        assert!((f + 6.0).abs() < 0.3, "le s fort : {f} dB");
        assert!(d.abs() < 0.2, "le s doux : {d} dB");
        let v = ecart(&x[2_000..10_000], &avant[2_000..10_000]);
        assert!(v.abs() < 0.05, "la voyelle : {v} dB");
    }

    #[test]
    fn le_grain_des_voyelles_baisse_et_les_sifflantes_restent() {
        // Une « voyelle » (300 Hz) et son grain (10 kHz, 35 dB sous elle), puis un « s » (10 kHz seul) : le grain de
        // la voyelle baisse de toute la baisse (4 dB), le « s » et le corps de la voyelle ne bougent pas.
        let n = (0.3 * f64::from(TAUX)) as usize;
        let (corps, grain) = (note(300.0, -16.0, 0.3), note(10_000.0, -51.0, 0.3));
        let mut x: Vec<f32> = corps.iter().zip(&grain).map(|(a, b)| a + b).collect();
        x.extend(note(10_000.0, -25.0, 0.3));
        let avant = x.clone();
        adoucir_le_grain(&mut x, f64::from(TAUX), &Chaine::DOUCE.grain);
        let milieu = n / 4..3 * n / 4;
        let reste: Vec<f32> = x[milieu.clone()].iter().zip(&corps[milieu.clone()]).map(|(a, b)| a - b).collect();
        let g = ecart(&reste, &grain[milieu.clone()]);
        assert!((g + 4.0).abs() < 0.1, "le grain : {g} dB");
        let s = ecart(&x[n + n / 4..n + 3 * n / 4], &avant[n + n / 4..n + 3 * n / 4]);
        assert!(s.abs() < 0.1, "le s : {s} dB");
        let v = ecart(&x[milieu.clone()], &avant[milieu]);
        assert!(v.abs() < 0.05, "la voyelle : {v} dB");
    }

    #[test]
    fn le_souffle_des_pauses_baisse_et_la_voix_reste() {
        // Une « voix » (300 Hz), une pause de 200 ms de souffle (un bruit 40 dB sous elle), la voix : la seconde
        // moitié du souffle baisse de plus de 6 dB ; la voix, même ses 5 premières ms après la pause, ne bouge pas.
        let (a, m) = ((0.4 * f64::from(TAUX)) as usize, (0.2 * f64::from(TAUX)) as usize);
        let mut graine = 3u32;
        let mut x = note(300.0, -16.0, 0.4);
        x.extend((0..m).map(|_| {
            graine = graine.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            0.0027 * ((graine >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0)
        }));
        x.extend(note(300.0, -16.0, 0.4));
        let avant = x.clone();
        calmer_le_souffle(&mut x, f64::from(TAUX), &Chaine::DOUCE.souffle);
        let s = ecart(&x[a + m / 2..a + m], &avant[a + m / 2..a + m]);
        assert!(s < -6.0, "le souffle : {s} dB");
        for r in [2_000..8_000, a + m..a + m + 110, a + m + 2_000..a + m + 8_000] {
            let v = ecart(&x[r.clone()], &avant[r]);
            assert!(v.abs() < 0.05, "la voix : {v} dB");
        }
    }

    #[test]
    fn ce_qui_vient_se_voit_d_avance() {
        let x = [0.0, -3.0, 0.0, 0.0, -6.0, 0.0, 0.0, 0.0];
        assert_eq!(a_venir(&x, 3, false), [-3.0, -3.0, -6.0, -6.0, -6.0, 0.0, 0.0, 0.0]);
        assert_eq!(a_venir(&x, 2, true), [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        // Un gain qui baisse vite et remonte lentement.
        let mut g = vec![0.0f32; 10];
        g.extend(vec![-6.0f32; 10]);
        g.extend(vec![0.0f32; 400]);
        lisser(&mut g, 1000.0, 0.001, 0.100);
        assert!(g[19] < -5.9 && g[20 + 100] > -2.3 && g[20 + 100] < -2.1, "{} {}", g[19], g[120]);
    }

    #[test]
    fn la_sonie_est_celle_de_bs_1770() {
        // Le repère de BS.1770 : un sinus de 1 kHz à -23 dBFS efficaces (sa crête à -20 dBFS) sonne à -23 LUFS, à un
        // dixième près (la pondération K y est presque nulle) ; plus aigu, il pèse plus (+4 dB tout en haut, +3,4 dB de
        // plus qu'à 1 kHz).
        let s = sonie(&sinus(1000.0, -23.0, 3.0), TAUX).unwrap();
        assert!((s + 23.0).abs() < 0.2, "{s}");
        let aigu = sonie(&sinus(8000.0, -23.0, 3.0), TAUX).unwrap();
        assert!((aigu - s - 3.36).abs() < 0.2, "{aigu}");
        // Un silence ne s'entend pas ; une phrase plus courte qu'un bloc se mesure entière.
        assert_eq!(sonie(&vec![0.0; 22_050], TAUX), None);
        let court = sonie(&sinus(1000.0, -23.0, 0.25), TAUX).unwrap();
        assert!((court + 23.0).abs() < 0.3, "{court}");
        // Une seconde de silence au milieu ne compte presque pas : les portes écartent les blocs muets, seuls ceux qui
        // la chevauchent pèsent un peu (-23,76 LUFS, au lieu de -24,8 pour une moyenne de tout).
        let mut troue = sinus(1000.0, -23.0, 1.0);
        troue.extend(vec![0.0f32; 22_050]);
        troue.extend(sinus(1000.0, -23.0, 1.0));
        let t = sonie(&troue, TAUX).unwrap();
        assert!((t + 23.76).abs() < 0.2, "{t}");
    }

    #[test]
    fn une_phrase_se_met_a_niveau_et_ses_cretes_vraies_restent_sous_le_plafond() {
        // Une phrase faible (-25 dBFS) monte, une forte (-12) baisse : toutes deux à la sonie visée.
        for db in [-25.0, -12.0] {
            let mut x = sinus(440.0, db, 1.5);
            niveler(&mut x, TAUX);
            let s = sonie(&x, TAUX).unwrap();
            assert!((s - SONIE).abs() < 0.3, "{db} : {s}");
        }
        // Un sinus au quart de la cadence, déphasé : ses échantillons à 0,707 de sa crête. Elle se voit entre eux.
        let x: Vec<f32> = (0..1_000)
            .map(|k| (0.9 * (std::f64::consts::FRAC_PI_2 * k as f64 + std::f64::consts::FRAC_PI_4).sin()) as f32)
            .collect();
        let echantillons = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let vraies = cretes_vraies(&x)[50..950].iter().fold(0.0f32, |m, &v| m.max(v));
        assert!((echantillons - 0.636).abs() < 0.01 && (vraies - 0.9).abs() < 0.02, "{echantillons} {vraies}");
        // Des crêtes au-dessus du plafond : le limiteur les ramène dessous, et ne touche pas au reste.
        let mut y = sinus(300.0, -20.0, 1.0);
        for i in [5_000, 12_000, 18_000] {
            y[i] = 0.99;
        }
        let avant = y.clone();
        let plafond = 10f64.powf(PLAFOND_DBTP / 20.0) as f32;
        limiter(&mut y, TAUX, plafond);
        let crete = cretes_vraies(&y).into_iter().fold(0.0f32, f32::max);
        assert!(crete <= plafond * 1.001, "{crete}");
        // Loin des crêtes (à plus de 200 ms, cinq fois la relâche), presque rien ne change.
        assert!(y[..500].iter().zip(&avant).all(|(a, b)| (a - b).abs() <= 1e-3 * b.abs() + 1e-6));
    }
}
