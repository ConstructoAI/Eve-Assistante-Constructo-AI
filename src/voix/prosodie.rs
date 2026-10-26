//! La prosodie d'une phrase dite, pour les gestes du visage d'Eve : ses repères ([`Repere`]) — ses pauses, la fin de
//! chaque groupe de mots que le texte ponctue (la virgule, le deux-points, le point, la question, l'exclamation), et
//! les mots qui nient, posés sur la voix. Une personne qui parle accompagne ces moments de la tête et des sourcils : un
//! petit hochement sur la dernière syllabe d'un groupe (celle que le français appuie), la tête penchée et les sourcils
//! levés au bout d'une question, un hochement et les sourcils sur une exclamation, la tête qui se pose à la fin d'une
//! phrase, un « non » de la tête quand elle nie.
//!
//! Le texte dit où sont les fins de groupes, en syllabes ([`parole::syllabes`]) ; la voix dit où elle se tait. Les deux
//! s'accordent dans l'ordre ([`accorder`]) : la part de la phrase dite à chaque fin de groupe, d'après ses syllabes,
//! et la part dite à chaque pause, d'après le temps de voix — la voix enchaîne parfois deux groupes sans pause, et
//! respire parfois où le texte ne ponctue pas. Un mot qui nie tombe sur la syllabe même que dit la bouche
//! ([`super::syllabes_des_cretes`]).

use eve::parole;

use super::{PAS_DE_L_ENVELOPPE, cretes, syllabes_des_cretes};

/// Ce que marque un repère de la phrase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marque {
    /// La dernière syllabe d'un groupe de mots que suit une virgule, un point-virgule ou un deux-points : celle que le
    /// français appuie, où la voix monte pour dire que la phrase continue.
    Groupe,
    /// La dernière syllabe d'une phrase qui finit par un point (ou sans ponctuation) : la voix descend.
    Fin,
    /// La dernière syllabe d'une question.
    Question,
    /// La dernière syllabe d'une exclamation.
    Exclamation,
    /// La voix se tait : une pause d'au moins 160 ms dans la phrase…
    Pause,
    /// … et reprend.
    Reprise,
    /// La syllabe appuyée d'un mot qui refuse : « non », « jamais ».
    Non,
    /// La syllabe appuyée d'un mot qui nie après un « ne » : « ce n'est pas facile », « je ne sais plus ».
    Negation,
}

/// Un repère de la phrase : ce qu'il marque, et quand (en secondes depuis le début de sa piste).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Repere {
    pub a: f32,
    pub marque: Marque,
}

/// Le niveau d'une syllabe (celui des crêtes de la voix) : la voix commence et finit là.
const VOIX: f32 = 0.35;
/// Le niveau sous lequel la voix se tait.
const SILENCE: f32 = 0.2;
/// Une pause : au moins ce nombre de pas de silence (160 ms) ; un pas plus fort, seul, ne la coupe pas (un souffle, un
/// clic). En deçà, une consonne qui ferme la bouche (« t », « p »), pas une pause.
const PAUSE_MIN: usize = 4;
/// Ce que coûte, pour accorder le texte à la voix, une fin de groupe sans pause (la voix enchaîne) ou une pause sans
/// fin de groupe (la voix respire) ; une fin de groupe et une pause trop loin l'une de l'autre (en part de la phrase
/// dite) ne s'accordent jamais.
const SAUT: f32 = 0.15;
const ECART_MAX: f32 = 0.25;

/// Les repères d'une phrase, dans l'ordre : `niveaux`, sa voix pas à pas ([`super::enveloppe`]) ; `texte`, ce qu'elle
/// dit. Chaque groupe de mots finit sur la pause qui le suit (ou, sans elle, là où sa part de syllabes le place) ; son
/// repère tombe sur sa dernière crête. Rien ne s'entend : aucun.
pub(super) fn reperes_de(niveaux: &[f32], texte: &str) -> Vec<Repere> {
    let Some(debut) = niveaux.iter().position(|&v| v >= VOIX) else { return Vec::new() };
    let fin = niveaux.iter().rposition(|&v| v >= VOIX).map_or(debut, |i| i + 1);
    let pauses = pauses(&niveaux[..fin], debut);
    let groupes = groupes(texte);
    // Le temps de voix dit avant un pas (sans les pauses), et le pas où il atteint une part de la phrase.
    let en_pause = |k: usize| pauses.iter().any(|&(a, b)| (a..b).contains(&k));
    let dit = |pas: usize| (debut..pas).filter(|&k| !en_pause(k)).count();
    let total = dit(fin).max(1);
    let pas_de = |part: f32| {
        let voulu = (part * total as f32).round() as usize;
        (debut..fin).filter(|&k| !en_pause(k)).nth(voulu).unwrap_or(fin)
    };
    // Les fins de groupes (sauf la dernière, la fin de la voix), en part de syllabes ; les pauses, en part de voix.
    let syllabes: usize = groupes.iter().map(|g| g.0).sum();
    let mut cumul = 0;
    let fins: Vec<f32> = groupes[..groupes.len().saturating_sub(1)]
        .iter()
        .map(|g| {
            cumul += g.0;
            cumul as f32 / syllabes.max(1) as f32
        })
        .collect();
    let parts: Vec<f32> = pauses.iter().map(|&(a, _)| dit(a) as f32 / total as f32).collect();
    let accord = accorder(&fins, &parts);
    let cretes = cretes(niveaux);
    let seconde = |pas: f32| pas * PAS_DE_L_ENVELOPPE.as_secs_f32();
    let mut reperes = Vec::new();
    let mut depart = debut;
    for (k, &(_, ponctuation)) in groupes.iter().enumerate() {
        // Où finit le groupe : sur sa pause, là où sa part le place, ou à la fin de la voix ; et où commence le
        // suivant.
        let (bout, suivant) = match (fins.get(k), accord.get(k).copied().flatten()) {
            (_, Some(j)) => pauses[j],
            (Some(&part), None) => {
                let p = pas_de(part).max(depart);
                (p, p)
            }
            (None, _) => (fin, fin),
        };
        let marque = match ponctuation {
            Some('?') => Marque::Question,
            Some('!') => Marque::Exclamation,
            Some(',' | ';' | ':') if k + 1 < groupes.len() => Marque::Groupe,
            _ => Marque::Fin,
        };
        // Sa dernière syllabe : sa dernière crête, ou le dernier pas avant sa fin.
        let syllabe = cretes.iter().rev().find(|&&c| c >= depart && c < bout).copied().or(bout.checked_sub(1));
        if let Some(c) = syllabe.filter(|&c| c >= depart) {
            reperes.push(Repere { a: seconde(c as f32 + 0.5), marque });
        }
        depart = suivant;
    }
    for &(a, b) in &pauses {
        reperes.push(Repere { a: seconde(a as f32), marque: Marque::Pause });
        reperes.push(Repere { a: seconde(b as f32), marque: Marque::Reprise });
    }
    // Les mots qui nient : sur la crête de leur syllabe appuyée (la même que la bouche), ou la première d'après.
    let de_la_crete = syllabes_des_cretes(niveaux, &cretes, &parole::syllabes(texte));
    for (k, marque) in negations(texte) {
        let crete = cretes.iter().zip(&de_la_crete).find(|(_, s)| s.is_some_and(|s| s >= k)).map(|(&c, _)| c);
        if let Some(c) = crete {
            reperes.push(Repere { a: seconde(c as f32 + 0.5), marque });
        }
    }
    reperes.sort_by(|a, b| a.a.total_cmp(&b.a));
    reperes
}

/// Les mots qui nient, dans l'ordre : le rang de leur syllabe appuyée — leur dernière — parmi celles du texte
/// ([`parole::syllabes`]), et ce qu'ils marquent. « Non » et « jamais » refusent d'eux-mêmes ([`Marque::Non`]) — pas
/// « non seulement », ni « si jamais », ni « à jamais » ; « pas », « plus », « personne », « guère », « ni » nient
/// après un « ne » du même groupe de mots ([`Marque::Negation`]) — pas « n'est-ce pas », qui demande un oui ; « rien »,
/// « aucun », « aucune » nient même sans lui — pas « de rien ». Un seul par groupe : « je n'ai jamais rien vu » ne
/// secoue la tête qu'une fois.
fn negations(texte: &str) -> Vec<(usize, Marque)> {
    let mut sortie = Vec::new();
    let (mut ne, mut nie) = (false, false);
    let mut avant: Vec<String> = Vec::new();
    let mut i = 0;
    while let Some(debut) = texte[i..].find(char::is_alphabetic).map(|d| i + d) {
        // Une ponctuation depuis le mot d'avant : un autre groupe de mots.
        if texte[i..debut].chars().any(|c| force(c) > 0) {
            (ne, nie) = (false, false);
            avant.clear();
        }
        let fin = texte[debut..].find(|c: char| !c.is_alphabetic()).map_or(texte.len(), |f| debut + f);
        let mot = texte[debut..fin].to_lowercase();
        let suivant = texte[fin..].split(|c: char| !c.is_alphabetic()).find(|m| !m.is_empty()).unwrap_or("");
        let precedent = avant.last().map_or("", String::as_str);
        let question = avant.len() >= 3 && avant[avant.len() - 3..] == ["n", "est", "ce"];
        let marque = match mot.as_str() {
            "ne" | "n" => {
                ne = true;
                None
            }
            "non" if !suivant.eq_ignore_ascii_case("seulement") => Some(Marque::Non),
            "jamais" if precedent != "si" && precedent != "à" => Some(Marque::Non),
            "pas" if ne && !question => Some(Marque::Negation),
            "plus" | "personne" | "guère" | "ni" if ne => Some(Marque::Negation),
            "rien" if precedent != "de" => Some(Marque::Negation),
            "aucun" | "aucune" => Some(Marque::Negation),
            _ => None,
        };
        if let Some(marque) = marque.filter(|_| !nie) {
            nie = true;
            if let Some(k) = parole::syllabes(&texte[..fin]).len().checked_sub(1) {
                sortie.push((k, marque));
            }
        }
        avant.push(mot);
        i = fin;
    }
    sortie
}

/// Les pauses de la voix — au moins [`PAUSE_MIN`] pas sous [`SILENCE`] ; un pas plus fort, seul, ne les coupe pas —, de
/// `debut` à la fin de `niveaux` (la voix, sans son silence final) : leur premier pas, et celui où la voix reprend.
fn pauses(niveaux: &[f32], debut: usize) -> Vec<(usize, usize)> {
    let n = niveaux.len();
    let silence = |k: usize| k < n && niveaux[k] < SILENCE;
    let mut pauses = Vec::new();
    let mut i = debut;
    while i < n {
        if !silence(i) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        loop {
            if silence(j) {
                j += 1;
            } else if silence(j + 1) {
                j += 2;
            } else {
                break;
            }
        }
        if j - i >= PAUSE_MIN {
            pauses.push((i, j));
        }
        i = j;
    }
    pauses
}

/// La force d'une ponctuation : de deux qui se suivent (« ?! », « ... »), la plus forte finit le groupe — la question
/// d'abord.
fn force(c: char) -> u8 {
    match c {
        '?' => 5,
        '!' => 4,
        '.' | '…' => 3,
        ':' | ';' => 2,
        ',' => 1,
        _ => 0,
    }
}

/// Les groupes de mots d'un texte, dans l'ordre : leurs syllabes ([`parole::syllabes`]), et la ponctuation qui les
/// finit — suivie d'un blanc ou de la fin du texte (pas celle de « 3,5 » ni de « 9:30 »). Un groupe sans syllabe (un
/// nombre) rejoint celui d'avant.
fn groupes(texte: &str) -> Vec<(usize, Option<char>)> {
    let car: Vec<char> = texte.chars().collect();
    let mut groupes: Vec<(usize, Option<char>)> = Vec::new();
    let mut mots = String::new();
    let mut ponctuation: Option<char> = None;
    let mut fermer = |mots: &mut String, ponctuation: Option<char>| {
        let n = parole::syllabes(mots).len();
        mots.clear();
        if n > 0 {
            groupes.push((n, ponctuation));
        } else if let Some((_, p)) = groupes.last_mut()
            && ponctuation.map_or(0, force) > p.map_or(0, force)
        {
            *p = ponctuation;
        }
    };
    for (i, &c) in car.iter().enumerate() {
        if force(c) == 0 {
            mots.push(c);
            continue;
        }
        if ponctuation.is_none_or(|p| force(c) > force(p)) {
            ponctuation = Some(c);
        }
        match car.get(i + 1) {
            Some(&s) if force(s) > 0 => {}
            Some(&s) if !s.is_whitespace() => {
                // Dans un mot ou un nombre : pas une fin de groupe.
                mots.push(c);
                ponctuation = None;
            }
            _ => fermer(&mut mots, ponctuation.take()),
        }
    }
    fermer(&mut mots, None);
    groupes
}

/// L'accord des fins de groupes (leur part de la phrase, d'après ses syllabes) avec les pauses (leur part, d'après le
/// temps de voix), dans l'ordre : le moins d'écart en tout, chaque fin de groupe sans pause et chaque pause sans fin de
/// groupe comptant pour [`SAUT`]. Pour chaque fin de groupe, sa pause, si elle en a une.
fn accorder(fins: &[f32], pauses: &[f32]) -> Vec<Option<usize>> {
    let (n, m) = (fins.len(), pauses.len());
    let ecart = |i: usize, j: usize| Some((fins[i] - pauses[j]).abs()).filter(|&e| e <= ECART_MAX);
    // cout[i][j] : le meilleur accord des i premières fins de groupes avec les j premières pauses.
    let mut cout = vec![vec![f32::INFINITY; m + 1]; n + 1];
    cout[0][0] = 0.0;
    for i in 0..=n {
        for j in 0..=m {
            let mut c = cout[i][j];
            if i > 0 {
                c = c.min(cout[i - 1][j] + SAUT);
            }
            if j > 0 {
                c = c.min(cout[i][j - 1] + SAUT);
            }
            if i > 0
                && j > 0
                && let Some(e) = ecart(i - 1, j - 1)
            {
                c = c.min(cout[i - 1][j - 1] + e);
            }
            cout[i][j] = c;
        }
    }
    // Le chemin du meilleur accord, à rebours.
    let mut accord = vec![None; n];
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        let ici = cout[i][j];
        if i > 0
            && j > 0
            && let Some(e) = ecart(i - 1, j - 1)
            && (ici - (cout[i - 1][j - 1] + e)).abs() < 1e-5
        {
            accord[i - 1] = Some(j - 1);
            (i, j) = (i - 1, j - 1);
        } else if i > 0 && (j == 0 || (ici - (cout[i - 1][j] + SAUT)).abs() < 1e-5) {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    accord
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn le_texte_se_coupe_en_groupes() {
        // « Bonjour Sam. » (3 syllabes, un point), « Je t'ai préparé le point de la journée : » (11, deux-points),
        // « deux rendez-vous, » (4, virgule), « et une facture à envoyer. » (7, point).
        let texte = "Bonjour Sam. Je t'ai préparé le point de la journée : deux rendez-vous, et une facture à envoyer.";
        assert_eq!(groupes(texte), [(3, Some('.')), (11, Some(':')), (4, Some(',')), (7, Some('.'))]);
        // La question l'emporte sur l'exclamation qui la suit ; ni « 3,5 », ni « 9:30 », ni « 937.05 » ne coupent ; un
        // groupe sans syllabe rejoint celui d'avant ; la fin du texte, sans ponctuation, finit le dernier.
        assert_eq!(groupes("Vraiment ?! Oui"), [(2, Some('?')), (1, None)]);
        assert_eq!(groupes("Il reste 3,5 heures, à 9:30."), [(3, Some(',')), (1, Some('.'))]);
        assert_eq!(groupes("Le total : 937.05 !"), [(3, Some('!'))]);
        assert!(groupes("").is_empty() && groupes("42 ...").is_empty());
    }

    #[test]
    fn les_pauses_de_la_voix() {
        // La voix, un silence de cinq pas, la voix, un silence de trois pas (une consonne), la voix, un silence de six
        // pas coupé d'un souffle, la voix : deux pauses.
        let mut n = vec![1.0f32; 30];
        for k in [4, 5, 6, 7, 8, 12, 13, 14, 20, 21, 22, 24, 25, 26] {
            n[k] = 0.05;
        }
        n[23] = 0.4;
        assert_eq!(pauses(&n, 0), [(4, 9), (20, 27)]);
        // Ce qui précède le début de la voix ne compte pas.
        assert_eq!(pauses(&n, 9), [(20, 27)]);
    }

    #[test]
    fn le_texte_s_accorde_a_la_voix() {
        // Trois fins de groupes ; trois pauses, dont une que le texte ne ponctue pas (la voix respire) : chaque fin sur
        // la pause la plus proche, dans l'ordre.
        assert_eq!(accorder(&[0.25, 0.75], &[0.22, 0.39, 0.75]), [Some(0), Some(2)]);
        // Une fin de groupe sans pause (la voix enchaîne), une fin trop loin de toute pause.
        assert_eq!(accorder(&[0.12, 0.56, 0.72], &[0.165, 0.69]), [Some(0), None, Some(1)]);
        assert_eq!(accorder(&[0.5], &[0.9]), [None]);
        assert!(accorder(&[], &[0.3]).is_empty());
    }

    #[test]
    fn une_phrase_a_ses_reperes() {
        // « Tu viens, ou pas ? Bien ! » sur une voix inventée : trois élans de voix, chacun avec ses crêtes, séparés
        // par deux pauses ; le silence du début et de la fin.
        let mut n = vec![0.0f32; 40];
        for (k, v) in n.iter_mut().enumerate() {
            if (2..9).contains(&k) || (14..22).contains(&k) || (28..34).contains(&k) {
                *v = if k % 3 == 0 { 1.0 } else { 0.6 };
            }
        }
        let r = reperes_de(&n, "Tu viens, ou pas ? Bien !");
        let marques: Vec<_> = r.iter().map(|r| r.marque).collect();
        use Marque::{Exclamation, Groupe, Pause, Question, Reprise};
        assert_eq!(marques, [Groupe, Pause, Reprise, Question, Pause, Reprise, Exclamation]);
        // Chaque groupe sur sa dernière crête (au milieu de son pas), chaque pause sur la voix qui se tait et reprend.
        let a: Vec<f32> = r.iter().map(|r| (r.a / 0.04 * 10.0).round() / 10.0).collect();
        assert_eq!(a, [6.5, 9.0, 14.0, 21.5, 22.0, 28.0, 33.5]);
        // Rien ne s'entend : aucun repère ; un texte sans ponctuation : sa fin.
        assert!(reperes_de(&[0.0; 10], "Bonjour").is_empty());
        let fin = reperes_de(&n[..10], "Bonjour");
        assert_eq!(fin.iter().map(|r| r.marque).collect::<Vec<_>>(), [Marque::Fin]);
        // « Non, je ne sais pas. Bien ! » sur la même voix : « Non » sur les deux crêtes du premier élan — son repère
        // sur la première, la tête dit non dès le mot —, « pas » sur la dernière du deuxième, comme la bouche les dit.
        let r = reperes_de(&n, "Non, je ne sais pas. Bien !");
        let a = |m: Marque| -> Vec<f32> {
            r.iter().filter(|r| r.marque == m).map(|r| (r.a / 0.04 * 10.0).round() / 10.0).collect()
        };
        assert_eq!((a(Marque::Non), a(Marque::Negation)), (vec![3.5], vec![21.5]));
    }

    #[test]
    fn les_mots_qui_nient() {
        use Marque::{Negation, Non};
        // « Non » refuse ; un « ne … pas » nie, à peine — pas « ou pas ? » sans « ne », ni « n'est-ce pas ? » ;
        // « jamais » refuse, pas « si jamais » ; « non seulement » ne refuse rien ; « rien », « aucun » nient, pas « de
        // rien » ; un seul par groupe de mots ; chacun sur sa dernière syllabe (son rang parmi celles du texte).
        assert_eq!(negations("Non, pas cette semaine : le fournisseur est en retard."), [(0, Non)]);
        assert_eq!(negations("Je comprends, ce n'est pas facile."), [(5, Negation)]);
        assert!(negations("Tu viens, ou pas ? C'est bien, n'est-ce pas ?").is_empty());
        assert_eq!(negations("Je ne sais plus. Jamais !"), [(3, Negation), (5, Non)]);
        assert!(negations("Si jamais tu le vois, non seulement dis-le, mais écris-le.").is_empty());
        assert_eq!(negations("Il n'y a aucun problème. De rien !"), [(4, Negation)]);
        assert_eq!(negations("Je n'ai jamais rien vu."), [(3, Non)]);
    }
}
