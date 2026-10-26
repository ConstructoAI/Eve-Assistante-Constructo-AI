//! Le buste d'Eve sous sa tête : son souffle ([`super::souffle`]), ses épaules et son torse. Ses épaules suivent ses
//! émotions : un soupir quand la peine ou l'empathie arrive (une inspiration profonde, puis les épaules qui retombent),
//! un peu basses dans la tristesse, un peu hautes dans la surprise — le souffle coupé —, qui sautent légèrement quand
//! elle rit au bout d'une phrase amusée, et qui se haussent au bout d'une phrase dite en réfléchissant tout haut (« le
//! prix, ou le délai »). Son torse suit un peu sa tête — une part de son roulis descend dans le haut du corps, avec un
//! temps de retard —, se penche vers la personne quand elle l'écoute, et se redresse quand elle réfléchit.
//! LivePortrait, qui fait les poses du visage, ne bouge ni les épaules ni le torse : le rendu les déplace sur son
//! treillis ([`super::rendu`]), d'après le [`Corps`] de la pose.

use std::sync::Arc;
use std::time::Duration;

use eve::parole::Emotion;

use crate::voix::{Marque, Repere};

use super::emotions::Emotions;
use super::mimique::{Moment, Ressort};
use super::souffle::Souffle;
use super::{Activite, Corps, Pose, Tete};

/// La pulsation du ressort des épaules : le haut du corps est lourd, elles changent de posture en une seconde et
/// demie ; une surprise les lève d'un coup (en un tiers de seconde).
const W_EPAULES: f32 = 3.0;
const W_SURSAUT: f32 = 10.0;
/// Le moins de temps entre deux mêmes gestes (deux soupirs, deux surprises, deux rires, deux haussements), en
/// secondes : on ne rit pas au bout de chaque phrase d'une longue réponse amusée ; l'air d'un soupir de peine, et
/// d'empathie.
const ENTRE_DEUX_GESTES: f64 = 10.0;
const SOUPIR_DE_PEINE: f32 = 1.75;
const SOUPIR_D_EMPATHIE: f32 = 1.5;
/// Le rire au bout d'une phrase amusée : un instant après sa dernière syllabe, trois rebonds des épaules de plus en
/// plus petits (en part du rayon), un tous les 0,29 s — assez lents pour ne pas trembler —, chacun qui monte en 0,11 s
/// et redescend en 0,17 s.
const RIRE_APRES: f64 = 0.15;
const RIRE: [f32; 3] = [0.0045, 0.0034, 0.0022];
const ENTRE_DEUX_REBONDS: f64 = 0.29;
/// Le haussement d'épaules au bout d'une phrase dite en réfléchissant : sa hauteur (en part du rayon), et son avance
/// sur la dernière syllabe (il culmine avec elle).
const HAUSSEMENT: f32 = 0.0065;
const AVANT_LA_DERNIERE: f32 = 0.2;
/// Le haut du corps suit la tête : la part de son roulis qui y descend, et la pulsation de son ressort — plus lent que
/// celui de la tête, il la suit avec un temps de retard.
const SUIT_LA_TETE: f32 = 0.2;
const W_TORSE: f32 = 3.5;
/// La pulsation du ressort de la posture du torse (penché vers la personne, redressé) : on ne se penche pas d'un coup,
/// mais en une seconde et demie.
const W_POSTURE: f32 = 2.5;

/// La posture du torse, selon ce que fait Eve et l'émotion qui domine : ce dont il se penche vers la personne (la part
/// dont elle grandit autour de ses yeux ; négatif : en arrière) et ce dont il se redresse (en part du rayon ; négatif :
/// tassé). Vers la personne quand elle l'écoute, un peu quand elle lui parle, et dans l'empathie ; redressée et un peu
/// en arrière quand elle réfléchit, en arrière dans la surprise ; tassée dans la tristesse, droite quand elle est
/// sérieuse.
fn posture_du_torse(a: Activite, e: Option<Emotion>) -> (f32, f32) {
    let (avance, monte): (f32, f32) = match a {
        Activite::Ecoute => (0.007, 0.0),
        Activite::Reflechit => (-0.003, 0.003),
        Activite::Parle => (0.002, 0.0),
        Activite::Repos => (0.0, 0.0),
    };
    let (vers_elle, droite) = match e {
        Some(Emotion::Empathie) => (0.004, 0.0),
        Some(Emotion::Surprise) => (-0.003, 0.0),
        Some(Emotion::Triste) => (0.0, -0.002),
        Some(Emotion::Serieuse) => (0.0, 0.0015),
        _ => (0.0, 0.0),
    };
    ((avance + vers_elle).clamp(-0.006, 0.01), monte + droite)
}

/// Les épaules d'une émotion, en part du rayon (positif : levées) : un peu basses dans la tristesse, à peine dans
/// l'empathie, un peu hautes dans la surprise.
fn epaules_de(e: Option<Emotion>) -> f32 {
    match e {
        Some(Emotion::Triste) => -0.0035,
        Some(Emotion::Empathie) => -0.0012,
        Some(Emotion::Surprise) => 0.003,
        _ => 0.0,
    }
}

/// Un mouvement des épaules : depuis quand, sa montée, sa tenue et sa descente (en secondes, chacune en
/// demi-cosinus), sa hauteur (en part du rayon).
#[derive(Clone, Copy)]
struct Rebond {
    depuis: f64,
    montee: f64,
    tenue: f64,
    descente: f64,
    hauteur: f32,
}

impl Rebond {
    /// Où il en est à l'instant `t` : de 0 à sa hauteur.
    fn en(&self, t: f64) -> f32 {
        let demi = |x: f64| (0.5 - 0.5 * (std::f64::consts::PI * x.clamp(0.0, 1.0)).cos()) as f32;
        let s = t - self.depuis;
        let part = match s {
            s if s <= 0.0 => 0.0,
            s if s < self.montee => demi(s / self.montee),
            s if s < self.montee + self.tenue => 1.0,
            s => demi(1.0 - (s - self.montee - self.tenue) / self.descente),
        };
        self.hauteur * part
    }

    fn fini(&self, t: f64) -> bool {
        t - self.depuis >= self.montee + self.tenue + self.descente
    }
}

/// Le buste, d'une image à l'autre : son souffle ; ses épaules — leur posture, sur son ressort, et leurs rebonds ; son
/// torse — penché du côté de la tête, vers la personne, redressé, chacun sur son ressort ; l'émotion qui dominait à
/// l'image d'avant ; le dernier soupir, la dernière surprise, le dernier rire, le dernier haussement ; la phrase
/// qu'elle dit — ses repères, jusqu'où ils sont lus —, si elle la dit amusée, en réfléchissant.
#[derive(Default)]
pub(super) struct Buste {
    souffle: Souffle,
    epaules: Ressort,
    rebonds: Vec<Rebond>,
    roulis: Ressort,
    avance: Ressort,
    monte: Ressort,
    emotion: Option<Emotion>,
    dernier_soupir: Option<f64>,
    derniere_surprise: Option<f64>,
    dernier_rire: Option<f64>,
    dernier_haussement: Option<f64>,
    phrase: Option<Arc<[Repere]>>,
    lu: f32,
    amusee: bool,
    pensive: bool,
}

/// Le geste fait pour la dernière fois à l'instant `dernier` peut revenir à l'instant `temps` : pas deux fois en
/// [`ENTRE_DEUX_GESTES`].
fn libre(dernier: Option<f64>, temps: f64) -> bool {
    dernier.is_none_or(|d| temps - d >= ENTRE_DEUX_GESTES)
}

impl Buste {
    /// Le buste à cette image, les `emotions` du visage à la leur, sous la tête de la pose (que ses gestes ont posée) :
    /// son souffle, qui suit ce que fait Eve et, quand elle parle, sa phrase (ce qui s'en entend :
    /// [`crate::voix::reperes_a_venir`]) ; ses épaules ; son torse. Sans les animations de Windows, rien ne bouge.
    pub(super) fn avancer(&mut self, m: &Moment, emotions: &Emotions, pose: &mut Pose) {
        if !m.vivant {
            return;
        }
        let voix = if m.parle() { crate::voix::reperes_a_venir(Duration::ZERO) } else { None };
        pose.corps = self.bouger(m, emotions.dominante(), pose.tete, voix.as_ref());
    }

    /// Le buste à cette image, l'émotion `dominante` du visage, sous la `tete` ; `voix` : la phrase qui s'entend — où
    /// elle en est (en secondes depuis son début) et ses repères.
    fn bouger(
        &mut self,
        m: &Moment,
        dominante: Option<Emotion>,
        tete: Tete,
        voix: Option<&(f32, Arc<[Repere]>)>,
    ) -> Corps {
        // Le souffle d'abord (il sait alors si elle va parler), puis ce que l'émotion qui arrive lui demande.
        let souffle = self.souffle.avancer(m, voix);
        if dominante != self.emotion {
            self.emotion = dominante;
            self.emouvoir(m.temps);
        }
        self.suivre_la_phrase(m, voix);
        let voulu = epaules_de(dominante);
        let sursaut = dominante == Some(Emotion::Surprise) && voulu > self.epaules.x;
        let posture = self.epaules.vers(voulu, if sursaut { W_SURSAUT } else { W_EPAULES }, m.dt);
        self.rebonds.retain(|r| !r.fini(m.temps));
        let rebonds: f32 = self.rebonds.iter().map(|r| r.en(m.temps)).sum();
        let (avance, monte) = posture_du_torse(m.activite, dominante);
        Corps {
            souffle,
            epaules: posture + rebonds,
            monte: self.monte.vers(monte, W_POSTURE, m.dt),
            roulis: self.roulis.vers(SUIT_LA_TETE * tete.roulis, W_TORSE, m.dt),
            avance: self.avance.vers(avance, W_POSTURE, m.dt),
        }
    }

    /// Une émotion arrive (celle qui domine change) : la peine ou l'empathie, un soupir ; la surprise, le souffle
    /// coupé — ni l'un ni l'autre deux fois en dix secondes, ni pendant qu'elle dit une phrase.
    fn emouvoir(&mut self, temps: f64) {
        match self.emotion {
            Some(e @ (Emotion::Triste | Emotion::Empathie)) if libre(self.dernier_soupir, temps) => {
                let air = if e == Emotion::Triste { SOUPIR_DE_PEINE } else { SOUPIR_D_EMPATHIE };
                if self.souffle.soupirer(temps, air) {
                    self.dernier_soupir = Some(temps);
                }
            }
            Some(Emotion::Surprise) if libre(self.derniere_surprise, temps) => {
                if self.souffle.couper(temps) {
                    self.derniere_surprise = Some(temps);
                }
            }
            _ => {}
        }
    }

    /// La phrase qu'elle dit : amusée ou en réfléchissant tout haut, si cette émotion a dominé à un moment de la
    /// phrase ; au bout de sa dernière syllabe — son dernier repère de fin —, amusée, elle rit ; en réfléchissant,
    /// elle hausse les épaules, qui culminent avec elle ; ni l'un ni l'autre deux fois en dix secondes.
    fn suivre_la_phrase(&mut self, m: &Moment, voix: Option<&(f32, Arc<[Repere]>)>) {
        let Some((t, reperes)) = voix else {
            self.phrase = None;
            return;
        };
        if !self.phrase.as_ref().is_some_and(|p| Arc::ptr_eq(p, reperes)) {
            (self.phrase, self.lu, self.amusee, self.pensive) = (Some(reperes.clone()), t - m.dt, false, false);
        }
        self.amusee |= self.emotion == Some(Emotion::Amusee);
        self.pensive |= self.emotion == Some(Emotion::Reflexion);
        let avant = std::mem::replace(&mut self.lu, *t);
        let fins = reperes.iter().filter(|r| matches!(r.marque, Marque::Fin | Marque::Question | Marque::Exclamation));
        let Some(derniere) = fins.last() else { return };
        let atteint = |a: f32| avant < a && a <= *t;
        if self.amusee && atteint(derniere.a) && libre(self.dernier_rire, m.temps) {
            self.dernier_rire = Some(m.temps);
            self.rire(m.temps + RIRE_APRES);
        }
        if self.pensive && atteint(derniere.a - AVANT_LA_DERNIERE) && libre(self.dernier_haussement, m.temps) {
            self.dernier_haussement = Some(m.temps);
            let hausse = Rebond { depuis: m.temps, montee: 0.22, tenue: 0.3, descente: 0.5, hauteur: HAUSSEMENT };
            self.rebonds.push(hausse);
        }
    }

    /// Le rire, à partir de l'instant `depuis` : les épaules sautent trois fois, de moins en moins, et l'air sort tout
    /// ce temps.
    fn rire(&mut self, depuis: f64) {
        let mut fin = depuis;
        for (k, &hauteur) in RIRE.iter().enumerate() {
            let rebond = Rebond {
                depuis: depuis + k as f64 * ENTRE_DEUX_REBONDS,
                montee: 0.11,
                tenue: 0.0,
                descente: 0.17,
                hauteur,
            };
            fin = rebond.depuis + rebond.montee + rebond.descente;
            self.rebonds.push(rebond);
        }
        self.souffle.rire(fin);
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    fn repere(a: f32, marque: Marque) -> Repere {
        Repere { a, marque }
    }

    /// Le buste à chaque image, à 60 images par seconde, de `debut` pendant `duree` secondes, Eve à son `activite`,
    /// l'émotion `dominante` ; `voix` : la phrase qui s'entend depuis l'instant qu'elle dit (et ses repères), tant que
    /// sa durée n'est pas passée.
    fn filmer(
        b: &mut Buste,
        (debut, duree): (f64, f64),
        activite: Activite,
        dominante: Option<Emotion>,
        voix: Option<(f64, f64, &Arc<[Repere]>)>,
    ) -> Vec<(f64, Corps)> {
        filmer_sous(b, (debut, duree), activite, dominante, voix, Tete::default())
    }

    /// Comme [`filmer`], sous la tête `tete`.
    fn filmer_sous(
        b: &mut Buste,
        (debut, duree): (f64, f64),
        activite: Activite,
        dominante: Option<Emotion>,
        voix: Option<(f64, f64, &Arc<[Repere]>)>,
        tete: Tete,
    ) -> Vec<(f64, Corps)> {
        let dt = 1.0 / 60.0;
        (0..(duree / dt).round() as usize)
            .map(|k| {
                let temps = debut + k as f64 * dt;
                let m = Moment { temps, dt: dt as f32, activite, vivant: true, ..Default::default() };
                let entendue = voix
                    .filter(|&(depuis, longueur, _)| temps >= depuis && temps < depuis + longueur)
                    .map(|(depuis, _, r)| ((temps - depuis) as f32, r.clone()));
                (temps, b.bouger(&m, dominante, tete, entendue.as_ref()))
            })
            .collect()
    }

    /// Rien ne saute d'une image à l'autre : les épaules vont en douceur (au plus un quart de pixel par image au film,
    /// au plus vite d'un rire).
    fn sans_saut(images: &[(f64, Corps)]) {
        for w in images.windows(2) {
            let saut = (w[1].1.epaules - w[0].1.epaules).abs();
            assert!(saut < 0.0012, "à {:.2} s : {saut}", w[1].0);
        }
    }

    #[test]
    fn la_peine_fait_soupirer_puis_les_epaules_restent_basses() {
        // Au repos, puis la peine arrive : un soupir (l'air bien au-dessus d'une inspiration au repos), puis l'air
        // sort plus bas qu'au repos et les épaules se posent plus bas ; la peine qui revient tout de suite après (une
        // autre émotion entre deux) ne fait pas soupirer de nouveau.
        let mut b = Buste::default();
        filmer(&mut b, (0.0, 6.0), Activite::Repos, None, None);
        let images = filmer(&mut b, (6.0, 6.0), Activite::Repos, Some(Emotion::Triste), None);
        sans_saut(&images);
        assert!(images.iter().any(|x| x.0 < 7.2 && x.1.souffle > 1.6), "le soupir");
        assert!(images.iter().any(|x| x.0 < 10.5 && x.1.souffle < -0.1), "l'air sort plus bas");
        assert!(images.last().unwrap().1.epaules < -0.003, "{}", images.last().unwrap().1.epaules);
        filmer(&mut b, (12.0, 1.0), Activite::Repos, Some(Emotion::Joie), None);
        let encore = filmer(&mut b, (13.0, 2.0), Activite::Repos, Some(Emotion::Triste), None);
        assert!(encore.iter().all(|x| x.1.souffle < 1.55), "pas un second soupir si tôt");
    }

    #[test]
    fn la_surprise_coupe_le_souffle_et_leve_les_epaules() {
        let mut b = Buste::default();
        filmer(&mut b, (0.0, 6.0), Activite::Repos, None, None);
        let images = filmer(&mut b, (6.0, 1.0), Activite::Repos, Some(Emotion::Surprise), None);
        sans_saut(&images);
        let a = |t: f64| images.iter().find(|x| x.0 >= t).unwrap().1;
        assert!(a(6.4).souffle >= 1.0 && a(6.4).epaules > 0.0025, "{} {}", a(6.4).souffle, a(6.4).epaules);
    }

    #[test]
    fn amusee_elle_rit_au_bout_de_sa_phrase_sans_trembler() {
        // « Sinon, ils tombent dans le bateau ! », dite amusée : sa dernière syllabe à 2,0 s ; les épaules sautent
        // trois fois, de moins en moins, à 0,29 s l'une de l'autre, après elle — rien avant —, et l'air sort tout le
        // temps qu'elle rit.
        let reperes: Arc<[Repere]> = [repere(0.5, Marque::Groupe), repere(2.0, Marque::Exclamation)].into();
        let mut b = Buste::default();
        filmer(&mut b, (0.0, 3.0), Activite::Repos, None, None);
        let images = filmer(&mut b, (3.0, 3.0), Activite::Parle, Some(Emotion::Amusee), Some((3.0, 3.0, &reperes)));
        sans_saut(&images);
        let rire: Vec<f32> = images.iter().filter(|x| (5.17..5.99).contains(&x.0)).map(|x| x.1.souffle).collect();
        assert!(rire.windows(2).all(|w| w[1] <= w[0] + 1e-4), "elle inspire en riant : {rire:?}");
        let hauts: Vec<(f64, f32)> = (1..images.len() - 1)
            .filter(|&i| {
                let e = |k: usize| images[k].1.epaules;
                e(i) > 0.0015 && e(i) > e(i - 1) && e(i) >= e(i + 1)
            })
            .map(|i| (images[i].0, images[i].1.epaules))
            .collect();
        assert_eq!(hauts.len(), 3, "{hauts:?}");
        assert!(hauts[0].0 > 5.0 && hauts.windows(2).all(|w| w[1].1 < w[0].1), "{hauts:?}");
        assert!(hauts.windows(2).all(|w| (w[1].0 - w[0].0 - 0.29).abs() < 0.03), "{hauts:?}");
        // La phrase amusée qui suit, tout de suite après : elle ne rit pas encore.
        let suite: Arc<[Repere]> = [repere(1.5, Marque::Exclamation)].into();
        let images = filmer(&mut b, (6.0, 3.0), Activite::Parle, Some(Emotion::Amusee), Some((6.0, 3.0, &suite)));
        assert!(images.iter().all(|x| x.1.epaules.abs() < 1e-4), "un second rire si tôt");
        // Dite sans être amusée : pas de rire.
        let mut c = Buste::default();
        let sans = filmer(&mut c, (3.0, 3.0), Activite::Parle, None, Some((3.0, 3.0, &reperes)));
        assert!(sans.iter().all(|x| x.1.epaules.abs() < 1e-4));
    }

    #[test]
    fn en_reflechissant_tout_haut_elle_hausse_les_epaules_au_bout() {
        // « Il y a deux façons de voir ça : le prix, ou le délai. » : sa dernière syllabe à 2,4 s ; les épaules se
        // haussent avec elle, puis retombent.
        let reperes: Arc<[Repere]> =
            [repere(0.9, Marque::Groupe), repere(1.6, Marque::Groupe), repere(2.4, Marque::Fin)].into();
        let mut b = Buste::default();
        let pensive = Some(Emotion::Reflexion);
        let images = filmer(&mut b, (0.0, 4.0), Activite::Parle, pensive, Some((0.0, 3.0, &reperes)));
        sans_saut(&images);
        let a = |t: f64| images.iter().find(|x| x.0 >= t).unwrap().1.epaules;
        assert!(a(2.0) < 0.0005 && a(2.45) > 0.005 && a(3.6) < 0.001, "{} {} {}", a(2.0), a(2.45), a(3.6));
    }

    #[test]
    fn le_torse_suit_la_tete_avec_un_temps_de_retard() {
        // La tête se penche de trois degrés : le haut du corps la suit, d'un cinquième, plus lentement qu'elle.
        let mut b = Buste::default();
        let penchee = Tete { roulis: 0.05, ..Tete::default() };
        let images = filmer_sous(&mut b, (0.0, 3.0), Activite::Repos, None, None, penchee);
        let a = |t: f64| images.iter().find(|x| x.0 >= t).unwrap().1.roulis;
        assert!(a(0.3) < 0.5 * SUIT_LA_TETE * 0.05, "{}", a(0.3));
        assert!((a(2.9) - SUIT_LA_TETE * 0.05).abs() < 0.0005, "{}", a(2.9));
        assert!(images.windows(2).all(|w| (w[1].1.roulis - w[0].1.roulis).abs() < 0.0005));
    }

    #[test]
    fn elle_se_penche_pour_ecouter_et_se_redresse_pour_reflechir() {
        // Elle écoute : penchée vers la personne, en une seconde et demie, sans à-coup ; elle réfléchit : redressée, un
        // peu en arrière ; la tristesse la tasse.
        let mut b = Buste::default();
        filmer(&mut b, (0.0, 2.0), Activite::Repos, None, None);
        let ecoute = filmer(&mut b, (2.0, 3.0), Activite::Ecoute, None, None);
        let reflechit = filmer(&mut b, (5.0, 3.0), Activite::Reflechit, None, None);
        let triste = filmer(&mut b, (8.0, 3.0), Activite::Repos, Some(Emotion::Triste), None);
        let toutes: Vec<_> = ecoute.iter().chain(&reflechit).chain(&triste).collect();
        assert!(toutes.windows(2).all(|w| (w[1].1.avance - w[0].1.avance).abs() < 0.0003));
        let (e, r, t) = (ecoute.last().unwrap().1, reflechit.last().unwrap().1, triste.last().unwrap().1);
        assert!(e.avance > 0.0065 && ecoute.iter().find(|x| x.0 >= 2.3).unwrap().1.avance < 0.004, "{}", e.avance);
        assert!(r.avance < -0.0025 && r.monte > 0.0025, "{} {}", r.avance, r.monte);
        assert!(t.monte < -0.0015 && t.epaules < -0.003, "{} {}", t.monte, t.epaules);
    }
}
