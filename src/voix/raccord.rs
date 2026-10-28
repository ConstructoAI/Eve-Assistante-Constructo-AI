//! Le raccord des phrases : chaque WAV de Piper façonné avant de se dire, pour que la parole s'enchaîne comme celle
//! d'une personne. Piper rend des phrases qui se suivent mal (mesuré le 2026-10-26 dans une Eve d'essai, le son du
//! poste enregistré) :
//!
//! - un souffle autour de la voix (un bruit blanc vers −50 dBFS), parfois long — 0,65 s avant « Lundi », 1,7 s après
//!   une phrase : les pauses entendues allaient de 0,3 à 2 s, au hasard de ce souffle ;
//! - le même silence après chaque phrase, qu'elle finisse par un point, une question ou un paragraphe ;
//! - chaque phrase ramenée à la même crête (0 dBFS) : une phrase courte sonne fort, une longue faible — « Oui. » à
//!   −10,5 dBFS et une longue phrase à −20 dBFS, presque 10 dB d'écart d'une phrase à l'autre ;
//! - une voix qui commence d'un coup, loin de zéro (jusqu'à −18 dBFS au premier échantillon) : un clic.
//!
//! Le raccord ([`faconner`]) retrouve les phrases de Piper dans son WAV (il pose après chacune un silence exact, des
//! zéros), ôte le souffle de leurs bords, les fait finir au studio ([`studio::finir`] : la chaîne de la voix, puis
//! chacune à la même sonie, ses crêtes gardées), les fond à leurs bords, et les sépare par la pause que demande leur
//! ponctuation ([`pause_apres`]). La phrase suivante part de la fin de sa voix, après sa propre pause ([`Saut`] : un
//! paragraphe se marque) : c'est le fil de la voix qui la fait partir à l'heure.

use std::ops::Range;
use std::time::Duration;

use eve::parole::Saut;

use super::studio::{self, Chaine};
use super::{entete, wav_mono};

/// Une trame d'analyse : le niveau se mesure par 10 ms.
const TRAME: f64 = 0.010;
/// Le niveau (RMS d'une trame) au-dessus duquel c'est sûrement la voix — ses voyelles, la plupart de ses consonnes : le
/// souffle de Piper reste dessous, même ses bouffées (de −56 à −46 dBFS en moyenne au bord des phrases, des pointes à
/// −36 dBFS jusqu'à deux secondes après la voix, mesuré).
const SEUIL_DE_VOIX: f32 = -32.0;
/// Ce qui prolonge la voix sûre à ses bords — une consonne douce qui l'ouvre, la chute qui la finit : au-dessus de ce
/// niveau, d'un seul tenant, au plus ce temps avant et après elle.
const SEUIL_DES_BORDS: f32 = -45.0;
const BORD_AVANT: f64 = 0.120;
const BORD_APRES: f64 = 0.200;
/// Une pause dans la phrase (là où se cherche la fin du contexte d'une phrase coupée) : des trames sous ce niveau.
const SEUIL_DE_PAUSE: f32 = -42.0;
/// Ce qui se garde autour de la voix : avant elle (l'attaque d'une consonne), après elle (sa chute) ; un fondu les
/// couvre, sans clic.
const MARGE_AVANT: f64 = 0.030;
const MARGE_APRES: f64 = 0.060;
/// Le silence au bout du WAV, après la voix : le lecteur de Windows finit ainsi sur des zéros.
const QUEUE: f64 = 0.030;
/// Une pause dans la phrase, là où la frontière d'une phrase coupée pour la synthèse se cherche : 60 ms au moins.
const PAUSE_DANS_LA_PHRASE: f64 = 0.060;

/// Le son façonné : le WAV, où sa voix commence et où elle se tait (depuis son début, hors des marges que couvrent ses
/// fondus) — la pause se compte de la fin d'une voix au début de la suivante.
#[derive(Debug)]
pub(super) struct Faconne {
    pub wav: Vec<u8>,
    pub tete: Duration,
    pub voix: Duration,
}

/// Comment façonner un WAV de Piper.
pub(super) struct Consigne<'a> {
    /// Le silence que Piper pose après chaque phrase (`--sentence_silence`) : il sépare ses phrases.
    pub silence: Duration,
    /// Le texte dit : ses fins de phrases donnent les pauses entre les phrases d'un même WAV.
    pub texte: &'a str,
    /// La pause après un point ([`pause_de_phrase`]).
    pub pause: Duration,
    /// Le texte synthétisé commence par quelques mots de contexte, déjà dits : où ils finissent (leur part du texte
    /// synthétisé, de 0 à 1). La voix ne commence qu'après la pause qui les suit.
    pub contexte: Option<f32>,
    /// La chaîne de studio de la voix ; aucune : la mise à niveau seule ([`studio::finir`]).
    pub chaine: Option<Chaine>,
}

/// La pause après un point, d'après le silence de phrase de la douceur : celui que Piper posait seul, plus ce que son
/// souffle et le lecteur y ajoutaient (0,45 s entendues en médiane, mesurées, pour un silence de 0,35 s).
pub(super) fn pause_de_phrase(silence: f32) -> Duration {
    Duration::from_millis((f64::from(silence.clamp(0.05, 1.4)) * 1000.0).round() as u64 + 100)
}

/// Une part d'une pause, à la milliseconde.
fn part(pause: Duration, facteur: f64) -> Duration {
    Duration::from_millis((pause.as_millis() as f64 * facteur).round() as u64)
}

/// La pause qui suit une phrase, selon ce qui la finit (`pause` : celle d'un point) : plus longue après une question —
/// on laisse la question se poser —, une exclamation ou des points de suspension ; plus courte après un deux-points,
/// un point-virgule ou une virgule (une phrase coupée pour la synthèse, une ligne de liste) : la phrase continue.
pub(super) fn pause_apres(phrase: &str, pause: Duration) -> Duration {
    let fin = phrase.trim_end().trim_end_matches(['»', '"', ')', '\'', '’', ' ', '\u{a0}', '\u{202f}']);
    let facteur = match fin.chars().last() {
        Some('?') => 1.25,
        Some('!') => 1.1,
        Some('…') => 1.35,
        Some(':') => 0.8,
        Some(';') => 0.7,
        Some(',' | '—' | '–') => 0.6,
        _ => 1.0,
    };
    part(pause, facteur)
}

/// Ce qu'un saut du texte ajoute à la pause : une autre ligne (une liste), un autre paragraphe.
pub(super) fn pause_de_saut(saut: Saut, pause: Duration) -> Duration {
    match saut {
        Saut::Phrase => Duration::ZERO,
        Saut::Ligne => part(pause, 0.15),
        Saut::Paragraphe => part(pause, 0.6),
    }
}

/// Façonne un WAV de Piper (PCM 16 bits, mono) : ses phrases retrouvées, sans le souffle de leurs bords, finies au
/// studio, fondues, séparées par leurs pauses, une courte queue de silence au bout. `None` : ce n'est pas un WAV de
/// Piper, ou il n'a pas de voix — il se dit tel quel.
pub(super) fn faconner(wav: &[u8], c: &Consigne) -> Option<Faconne> {
    let e = entete(wav)?;
    if e.taux == 0 || e.octets_par_seconde != e.taux * 2 {
        return None;
    }
    let fin = (e.debut + e.taille).min(wav.len());
    let (echantillons, _) = wav[e.debut..fin].as_chunks::<2>();
    let x: Vec<f32> = echantillons.iter().map(|&b| f32::from(i16::from_le_bytes(b)) / 32_768.0).collect();
    let taux = e.taux as usize;
    let n = |s: f64| (s * taux as f64).round() as usize;
    let mut phrases = phrases_de_piper(&x, n(c.silence.as_secs_f64() * 0.9).max(n(0.05)));
    if let (Some(part), Some(premiere)) = (c.contexte, phrases.first_mut()) {
        premiere.start += apres_le_contexte(&x[premiere.clone()], taux, part);
    }
    let textes = phrases_du_texte(c.texte);
    let mut sortie: Vec<f32> = Vec::with_capacity(x.len());
    let (mut tete, mut voix) = (None, 0);
    // La phrase précédente, et la marge gardée après sa voix.
    let mut precedente: Option<(usize, usize)> = None;
    for (k, p) in phrases.iter().enumerate() {
        let Some(v) = voix_de(&x[p.clone()], taux) else { continue };
        let mut morceau = x[p.start + v.plage.start..p.start + v.plage.end].to_vec();
        studio::finir(&mut morceau, e.taux, c.chaine.as_ref());
        fondre(&mut morceau, v.avant.max(taux / 200), v.apres.max(taux / 100));
        if let Some((j, apres)) = precedente {
            // La pause d'après la phrase précédente, d'après son texte si le texte a autant de phrases que Piper ; de
            // voix à voix : les marges des fondus en font partie.
            let pause = match (textes.len() == phrases.len()).then(|| textes[j]) {
                Some(t) => pause_apres(t, c.pause),
                None => c.pause,
            };
            sortie.resize(sortie.len() + n(pause.as_secs_f64()).saturating_sub(apres + v.avant), 0.0);
        }
        tete.get_or_insert(v.avant);
        sortie.extend_from_slice(&morceau);
        voix = sortie.len() - v.apres;
        precedente = Some((k, v.apres));
    }
    let tete = tete?;
    sortie.resize(voix + precedente.map_or(0, |p| p.1) + n(QUEUE), 0.0);
    let echantillons: Vec<i16> =
        sortie.iter().map(|v| (v * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16).collect();
    let duree = |e: usize| Duration::from_secs_f64(e as f64 / taux as f64);
    Some(Faconne { wav: wav_mono(&echantillons, e.taux), tete: duree(tete), voix: duree(voix) })
}

/// Les phrases de Piper dans son WAV : ce qui est entre ses silences de phrase, des zéros exacts (`minimum`
/// échantillons au moins) — sa voix et son souffle n'en font jamais autant de suite.
fn phrases_de_piper(x: &[f32], minimum: usize) -> Vec<Range<usize>> {
    let mut phrases = Vec::new();
    let (mut debut, mut zeros) = (0, 0);
    for (i, &v) in x.iter().enumerate() {
        if v == 0.0 {
            zeros += 1;
            continue;
        }
        if zeros >= minimum {
            if i - zeros > debut {
                phrases.push(debut..i - zeros);
            }
            debut = i;
        }
        zeros = 0;
    }
    let fin = x.len() - if zeros >= minimum { zeros } else { 0 };
    if fin > debut {
        phrases.push(debut..fin);
    }
    phrases
}

/// Les phrases d'un texte, comme Piper les coupe : après un point, une question, une exclamation ou des points de
/// suspension suivis d'une espace.
fn phrases_du_texte(texte: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut debut = 0;
    let car: Vec<(usize, char)> = texte.char_indices().collect();
    for (k, &(i, c)) in car.iter().enumerate() {
        if matches!(c, '.' | '!' | '?' | '…') && car.get(k + 1).is_some_and(|&(_, s)| s.is_whitespace()) {
            let fin = i + c.len_utf8();
            if texte[debut..fin].trim().chars().any(char::is_alphanumeric) {
                out.push(texte[debut..fin].trim());
                debut = fin;
            }
        }
    }
    if texte[debut..].trim().chars().any(char::is_alphanumeric) {
        out.push(texte[debut..].trim());
    }
    out
}

/// Le niveau (dBFS) de chaque trame de `duree` secondes.
fn niveaux(x: &[f32], taux: usize, duree: f64) -> Vec<f32> {
    let t = ((taux as f64 * duree).round() as usize).max(1);
    x.chunks(t)
        .map(|m| {
            let e = m.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / m.len() as f64;
            (10.0 * e.max(1e-12).log10()) as f32
        })
        .collect()
}

/// La voix d'une phrase dans son WAV, et ce qui se garde autour d'elle, en échantillons.
struct Voix {
    plage: Range<usize>,
    /// La marge gardée avant la voix, puis après elle : leurs fondus les couvrent.
    avant: usize,
    apres: usize,
}

/// La voix d'une phrase, sans le souffle de ses bords : de sa première trame sûre ([`SEUIL_DE_VOIX`]) à sa dernière,
/// prolongées par leurs bords ([`SEUIL_DES_BORDS`]), puis ses marges ([`MARGE_AVANT`], [`MARGE_APRES`]), réduites au
/// bord du WAV — une voix qui le touche se fond alors sur 5 ms à l'entrée (son attaque reste) et 10 ms à la sortie.
/// `None` : rien que du souffle.
fn voix_de(x: &[f32], taux: usize) -> Option<Voix> {
    let db = niveaux(x, taux, TRAME);
    let t = (taux as f64 * TRAME).round() as usize;
    let (mut a, mut b) = (db.iter().position(|&v| v > SEUIL_DE_VOIX)?, db.iter().rposition(|&v| v > SEUIL_DE_VOIX)?);
    let (a0, b0) = (a, b);
    while a > 0 && ((a0 - a) as f64) < BORD_AVANT / TRAME && db[a - 1] > SEUIL_DES_BORDS {
        a -= 1;
    }
    while b + 1 < db.len() && ((b - b0) as f64) < BORD_APRES / TRAME && db[b + 1] > SEUIL_DES_BORDS {
        b += 1;
    }
    let premiere = (a * t).min(x.len());
    let derniere = ((b + 1) * t).min(x.len());
    let debut = premiere.saturating_sub((taux as f64 * MARGE_AVANT) as usize);
    let fin = (derniere + (taux as f64 * MARGE_APRES) as usize).min(x.len());
    Some(Voix { plage: debut..fin, avant: premiere - debut, apres: fin - derniere })
}

/// Où commence ce qui suit le contexte, dans une phrase synthétisée avec ses premiers mots de contexte : la pause qui
/// tombe le plus près de la part `part` de sa voix (le contexte finit par sa ponctuation : Piper y fait une pause) ;
/// sans pause, la trame la plus calme à 0,2 s de là.
fn apres_le_contexte(x: &[f32], taux: usize, part: f32) -> usize {
    let db = niveaux(x, taux, TRAME);
    let t = (taux as f64 * TRAME).round() as usize;
    let (Some(a), Some(b)) = (db.iter().position(|&v| v > SEUIL_DE_VOIX), db.iter().rposition(|&v| v > SEUIL_DE_VOIX))
    else {
        return 0;
    };
    let vise = a as f32 + part.clamp(0.0, 1.0) * (b - a) as f32;
    let minimum = (PAUSE_DANS_LA_PHRASE / TRAME).round() as usize;
    let mut pauses: Vec<Range<usize>> = Vec::new();
    let mut debut = None;
    for (i, &v) in db.iter().enumerate().take(b).skip(a) {
        match (v <= SEUIL_DE_PAUSE, debut) {
            (true, None) => debut = Some(i),
            (false, Some(d)) => {
                if i - d >= minimum {
                    pauses.push(d..i);
                }
                debut = None;
            }
            _ => {}
        }
    }
    let ecart = |r: &Range<usize>| ((r.start + r.end) as f32 / 2.0 - vise).abs();
    match pauses.iter().min_by(|p, q| ecart(p).total_cmp(&ecart(q))) {
        Some(p) => p.start * t,
        None => {
            let pres = 20;
            let de = (vise as usize).saturating_sub(pres).max(a);
            let a_ = (vise as usize + pres).min(b);
            let calme = (de..=a_).min_by(|&i, &j| db[i].total_cmp(&db[j])).unwrap_or(vise as usize);
            calme * t
        }
    }
}

/// Un fondu en cosinus à l'entrée (`entree` échantillons) et à la sortie (`sortie`) : la voix part de zéro et y
/// revient, sans clic.
fn fondre(x: &mut [f32], entree: usize, sortie: usize) {
    let (entree, sortie) = (entree.min(x.len() / 4), sortie.min(x.len() / 4));
    for i in 0..entree {
        x[i] *= 0.5 - 0.5 * (std::f32::consts::PI * i as f32 / entree as f32).cos();
    }
    let n = x.len();
    for i in 0..sortie {
        x[n - 1 - i] *= 0.5 - 0.5 * (std::f32::consts::PI * i as f32 / sortie as f32).cos();
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    const TAUX: usize = 22_050;

    /// Un son d'essai : du souffle (un bruit blanc au niveau `db`), ou une « voix » (un son de 200 Hz au niveau `db`).
    fn souffle(secondes: f64, db: f32, graine: &mut u32) -> Vec<f32> {
        let a = 10f32.powf(db / 20.0) * 3f32.sqrt();
        (0..(secondes * TAUX as f64) as usize)
            .map(|_| {
                *graine = graine.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                a * ((*graine >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0)
            })
            .collect()
    }

    fn voix(secondes: f64, db: f32) -> Vec<f32> {
        let a = 10f32.powf(db / 20.0) * 2f32.sqrt();
        (0..(secondes * TAUX as f64) as usize)
            .map(|i| a * (2.0 * std::f32::consts::PI * 200.0 * i as f32 / TAUX as f32).sin())
            .collect()
    }

    fn en_wav(x: &[f32]) -> Vec<u8> {
        let e: Vec<i16> = x.iter().map(|v| (v * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16).collect();
        wav_mono(&e, TAUX as u32)
    }

    fn lire(wav: &[u8]) -> Vec<f32> {
        let e = entete(wav).unwrap();
        let (ech, _) = wav[e.debut..e.debut + e.taille].as_chunks::<2>();
        ech.iter().map(|&b| f32::from(i16::from_le_bytes(b)) / 32_768.0).collect()
    }

    fn consigne(texte: &str) -> Consigne<'_> {
        let (silence, pause) = (Duration::from_millis(350), Duration::from_millis(450));
        Consigne { silence, texte, pause, contexte: None, chaine: None }
    }

    /// Les plages où le son dépasse −42 dBFS par trames de 10 ms, en ms.
    fn voix_entendue(x: &[f32]) -> Vec<(usize, usize)> {
        let db = niveaux(x, TAUX, TRAME);
        let mut out: Vec<(usize, usize)> = Vec::new();
        for (i, &v) in db.iter().enumerate() {
            if v > SEUIL_DE_VOIX {
                match out.last_mut() {
                    Some(d) if d.1 == i * 10 => d.1 = (i + 1) * 10,
                    _ => out.push((i * 10, (i + 1) * 10)),
                }
            }
        }
        out
    }

    #[test]
    fn le_souffle_des_bords_s_en_va_et_la_voix_reste() {
        // Comme Piper : 0,65 s de souffle, la voix (0,8 s), 1,7 s de souffle, puis son silence de phrase.
        let mut g = 7;
        let mut x = souffle(0.65, -52.0, &mut g);
        x.extend(voix(0.8, -12.0));
        x.extend(souffle(1.7, -52.0, &mut g));
        x.extend(vec![0.0; (0.35 * TAUX as f64) as usize]);
        let f = faconner(&en_wav(&x), &consigne("Lundi et mardi sont calmes.")).unwrap();
        let y = lire(&f.wav);
        // La voix entière (à la trame près), sa marge avant (30 ms) et après (60 ms), puis 30 ms de queue.
        let duree = y.len() as f64 / TAUX as f64;
        assert!((duree - (0.03 + 0.8 + 0.06 + 0.03)).abs() < 0.015, "{duree}");
        // La voix commence après sa marge, et se tait avant la sienne.
        assert!((f.tete.as_secs_f64() - 0.03).abs() < 0.002, "{:?}", f.tete);
        assert!((f.voix.as_secs_f64() - (0.03 + 0.8)).abs() < 0.015, "{:?}", f.voix);
        let v = voix_entendue(&y);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].0 <= 40 && v[0].1 >= 820, "{v:?}");
        // Sans clic : la voix part de zéro et y revient.
        assert!(y[0].abs() < 1e-3 && y[y.len() - 1] == 0.0);
        let saut_max = y.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(saut_max < 0.04, "un saut de {saut_max}");
        // Que du souffle : rien à dire.
        let mut g = 3;
        assert!(faconner(&en_wav(&souffle(1.0, -52.0, &mut g)), &consigne("…")).is_none());
        assert!(faconner(b"pas un son", &consigne("rien")).is_none());
    }

    #[test]
    fn les_bouffees_du_souffle_ne_sont_pas_la_voix() {
        // La voix (0,8 s), puis 1,2 s de souffle, une bouffée à −38 dBFS, encore du souffle : comme la phrase n° 9 de la
        // séance du 2026-10-26, dont la suite est partie trois secondes trop tard. Et devant la voix, une consonne douce
        // (−40 dBFS, 80 ms), qui reste.
        let mut g = 11;
        let mut x = souffle(0.3, -52.0, &mut g);
        x.extend(souffle(0.08, -40.0, &mut g));
        x.extend(voix(0.8, -12.0));
        x.extend(souffle(1.2, -52.0, &mut g));
        x.extend(souffle(0.03, -38.0, &mut g));
        x.extend(souffle(1.0, -52.0, &mut g));
        x.extend(vec![0.0; (0.35 * TAUX as f64) as usize]);
        let f = faconner(&en_wav(&x), &consigne("Celui qui avait été reporté.")).unwrap();
        // La consonne et sa marge avant la voix ; la voix se tait avec elle, pas avec la bouffée.
        assert!((f.tete.as_secs_f64() - 0.03).abs() < 0.012, "{:?}", f.tete);
        assert!((f.voix.as_secs_f64() - (0.03 + 0.08 + 0.8)).abs() < 0.025, "{:?}", f.voix);
        let duree = lire(&f.wav).len() as f64 / TAUX as f64;
        assert!(duree < 1.1, "{duree}");
    }

    #[test]
    fn chaque_phrase_a_la_meme_sonie() {
        // Une phrase forte (−10 dBFS) et une faible (−20 dBFS) dans le même WAV : les deux à la sonie du studio.
        let silence = vec![0.0; (0.35 * TAUX as f64) as usize];
        let mut x = voix(0.6, -10.0);
        x.extend(&silence);
        x.extend(voix(1.2, -20.0));
        x.extend(&silence);
        let f = faconner(&en_wav(&x), &consigne("Oui. Le rendez-vous est jeudi.")).unwrap();
        let y = lire(&f.wav);
        let v = voix_entendue(&y);
        assert_eq!(v.len(), 2, "{v:?}");
        let sonie = |(a, b): (usize, usize)| studio::sonie(&y[a * TAUX / 1000..b * TAUX / 1000], TAUX as u32).unwrap();
        let (a, b) = (sonie(v[0]), sonie(v[1]));
        assert!((a - studio::SONIE).abs() < 0.6 && (b - studio::SONIE).abs() < 0.6, "{a} {b}");
    }

    #[test]
    fn la_pause_suit_la_ponctuation() {
        let (p, ms) = (Duration::from_millis(450), Duration::from_millis);
        assert_eq!(pause_de_phrase(0.35), p);
        assert_eq!(pause_apres("Tu veux que je te le rappelle ?", p), ms(563));
        assert_eq!(pause_apres("C'est noté.", p), p);
        assert_eq!(pause_apres("« Bonne nouvelle ! »", p), ms(495));
        assert_eq!(pause_apres("Il reste trois choses à faire :", p), ms(360));
        assert_eq!(pause_apres("payer la facture ;", p), ms(315));
        assert_eq!(pause_de_saut(Saut::Paragraphe, p), ms(270));
        assert_eq!(pause_de_saut(Saut::Phrase, p), Duration::ZERO);
        // Deux phrases dans un WAV, une question puis une autre : la pause entre elles est celle d'une question.
        let silence = vec![0.0; (0.35 * TAUX as f64) as usize];
        let mut x = voix(0.5, -14.0);
        x.extend(&silence);
        x.extend(voix(0.5, -14.0));
        x.extend(&silence);
        let y = lire(&faconner(&en_wav(&x), &consigne("Tu veux ? Je le note.")).unwrap().wav);
        let v = voix_entendue(&y);
        assert_eq!(v.len(), 2, "{v:?}");
        let pause = v[1].0 - v[0].1;
        // La pause, plus les fondus sous le seuil de part et d'autre, à la trame près.
        assert!((563 - 15..=563 + 40).contains(&pause), "{pause} ms");
        assert_eq!(
            phrases_du_texte("Tu veux ? Je le note. Il a payé 937.05 $."),
            ["Tu veux ?", "Je le note.", "Il a payé 937.05 $."]
        );
    }

    #[test]
    fn le_contexte_d_une_phrase_coupee_s_en_va() {
        // « …de l'école, avec des heures… » : trois mots de contexte (0,6 s), la pause de la virgule (0,3 s), la suite.
        let mut g = 5;
        let mut x = voix(0.6, -14.0);
        x.extend(souffle(0.3, -55.0, &mut g));
        x.extend(voix(1.5, -14.0));
        x.extend(vec![0.0; (0.35 * TAUX as f64) as usize]);
        let c = Consigne { contexte: Some(0.28), ..consigne("avec des heures de coucher.") };
        let f = faconner(&en_wav(&x), &c).unwrap();
        let y = lire(&f.wav);
        let v = voix_entendue(&y);
        assert_eq!(v.len(), 1, "le contexte s'en est allé : {v:?}");
        assert!((v[0].1 - v[0].0) as f64 >= 1450.0, "{v:?}");
    }
}
