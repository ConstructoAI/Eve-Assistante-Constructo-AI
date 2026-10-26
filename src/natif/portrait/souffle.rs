//! Le souffle d'Eve : l'air qui entre dans sa poitrine et qui en sort — le rendu le montre, sa poitrine et ses épaules
//! qui se soulèvent, la tête portée par elles ([`super::rendu`]). Au repos, le rythme d'une personne calme, jamais tout
//! à fait le même : une inspiration d'une seconde et demie, une expiration plus longue, un temps d'arrêt, et de loin en
//! loin une respiration plus profonde ; en écoutant, plus court et moins profond ; en réfléchissant, plus lent. En
//! parlant, comme on parle : une inspiration juste avant la première syllabe — le chef dit « parole » dès qu'il donne
//! la phrase à la voix, de trois dixièmes de seconde à plus d'une seconde avant qu'elle ne s'entende (mesuré au
//! journal de l'Assistante) —, la poitrine qui redescend doucement tant qu'elle parle, une petite inspiration aux
//! longues pauses de la phrase (entre deux phrases d'un même son, après un deux-points) et quand la voix se tait. Et
//! ce que demandent ses émotions ([`super::corps`]) : un soupir, le souffle coupé d'une surprise.
//!
//! L'air suit une courbe faite de passages qui partent et se posent doucement (un demi-cosinus), sur un ressort qui en
//! adoucit les reprises — une inspiration que la voix interrompt ne casse pas.

use std::sync::Arc;

use crate::voix::{Marque, Repere};

use super::Activite;
use super::mimique::{Hasard, Moment, Ressort};

/// La pulsation du ressort de l'air : il suit la courbe et adoucit ses reprises. Il a son temps de retard sur une
/// courbe qui monte ou descend (deux fois l'inverse de sa pulsation) : il la suit d'autant en avance, et l'air n'est
/// pas en retard sur ce qu'elle fait — l'inspiration d'avant la voix finit à temps. Sur une courbe qui ne fait que
/// monter (ou que descendre), ce ressort ne dépasse jamais.
const W_AIR: f32 = 14.0;
const RETARD_DE_L_AIR: f64 = 2.0 / W_AIR as f64;

/// Un rythme de respiration : l'inspiration, l'expiration et l'arrêt qui la suit (en secondes), et l'air d'une
/// inspiration (1 : celle du repos).
#[derive(Clone, Copy)]
struct Rythme {
    inspire: f32,
    expire: f32,
    arret: f32,
    ampleur: f32,
}

/// Au repos : un peu plus de treize respirations par minute ; en écoutant, une vingtaine, moins profondes ; en
/// réfléchissant, plus lentes.
const REPOS: Rythme = Rythme { inspire: 1.5, expire: 2.4, arret: 0.7, ampleur: 1.0 };
const ECOUTE: Rythme = Rythme { inspire: 1.1, expire: 1.6, arret: 0.25, ampleur: 0.65 };
const REFLEXION: Rythme = Rythme { inspire: 1.6, expire: 2.7, arret: 0.9, ampleur: 0.9 };
/// Une respiration plus profonde, toutes les cinq à sept : de combien, et de combien plus lente.
const PROFONDE: f32 = 1.35;
const PLUS_LENTE: f32 = 1.25;

/// L'air qu'elle prend avant de parler (on en prend plus pour parler qu'au repos), et en combien de temps depuis le
/// bout d'une expiration : vivement.
const PAROLE: f32 = 1.2;
const INSPIRATION_AVANT: f32 = 0.45;
/// Si la voix tarde : combien de temps elle retient son souffle, puis jusqu'où elle le relâche avant d'inspirer de
/// nouveau.
const TENUE: f32 = 0.9;
const ATTENTE: f32 = 0.75;
/// L'air qui sort en parlant, par seconde de voix, et ce qui en reste au plus bas : la poitrine redescend doucement.
const DEBIT: f32 = 0.3;
const FOND: f32 = 0.1;
/// Une longue pause de la phrase, où elle reprend son souffle (en secondes), et l'air qu'elle y prend au plus.
const LONGUE_PAUSE: f32 = 0.34;
const REPRISE: f32 = 0.5;
/// La voix se tait ce temps après la dernière syllabe de la phrase (son repère) ; puis elle reprend cet air.
const APRES_LA_DERNIERE: f32 = 0.15;
const ENTRE_DEUX: f32 = 0.95;
/// Un soupir : le temps de son inspiration et de sa tenue ; jusqu'où l'air sort ensuite, plus bas qu'au repos (les
/// épaules retombent), et en combien de temps.
const INSPIRATION_DU_SOUPIR: f32 = 0.9;
const TENUE_DU_SOUPIR: f64 = 0.25;
const AU_BOUT_DU_SOUPIR: f32 = -0.2;
const EXPIRATION_DU_SOUPIR: f32 = 2.0;

/// Un passage de la courbe de l'air : de `de` à `vers`, de l'instant `debut` à l'instant `fin`, en demi-cosinus — il
/// part doucement et se pose sans à-coup.
#[derive(Clone, Copy, Default)]
struct Passage {
    de: f32,
    vers: f32,
    debut: f64,
    fin: f64,
}

impl Passage {
    /// L'air à l'instant `t`.
    fn en(&self, t: f64) -> f32 {
        if t >= self.fin {
            return self.vers;
        }
        if t <= self.debut {
            return self.de;
        }
        let s = ((t - self.debut) / (self.fin - self.debut)) as f32;
        self.de + (self.vers - self.de) * (0.5 - 0.5 * (std::f32::consts::PI * s).cos())
    }
}

/// Où elle en est de ce qu'elle dit : rien ne s'entend encore (elle va parler, ou entre deux phrases) ; elle dit un
/// groupe de mots d'un même souffle (son rang dans la phrase) ; elle fait une longue pause (son rang) ; sa voix s'est
/// tue, au bout de la phrase.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Parole {
    Avant,
    Groupe(usize),
    Pause(usize),
    Apres,
}

/// Les souffles d'une phrase : ses repères, ses longues pauses (de quand à quand, en secondes depuis son début), et la
/// fin de sa voix.
struct Plan {
    reperes: Arc<[Repere]>,
    pauses: Vec<(f32, f32)>,
    fin: f32,
}

impl Plan {
    /// Le plan d'une phrase, d'après ses repères ([`Repere`]) : chaque pause et sa reprise, les longues gardées ; la
    /// voix finit un peu après sa dernière syllabe (sans repère, jamais : un seul souffle).
    fn de(reperes: &Arc<[Repere]>) -> Plan {
        let mut pauses = Vec::new();
        let mut ouverte = None;
        for r in reperes.iter() {
            match r.marque {
                Marque::Pause => ouverte = Some(r.a),
                Marque::Reprise => {
                    if let Some(a) = ouverte.take()
                        && r.a - a >= LONGUE_PAUSE
                    {
                        pauses.push((a, r.a));
                    }
                }
                _ => {}
            }
        }
        let syllabes = reperes.iter().filter(|r| !matches!(r.marque, Marque::Pause | Marque::Reprise));
        let derniere = syllabes.map(|r| r.a).fold(f32::NEG_INFINITY, f32::max);
        let fin = if derniere.is_finite() { derniere + APRES_LA_DERNIERE } else { f32::INFINITY };
        let fin = fin.max(pauses.last().map_or(0.0, |p| p.1));
        Plan { reperes: reperes.clone(), pauses, fin }
    }

    /// Où elle en est, `t` secondes après le début de la phrase.
    fn ou(&self, t: f32) -> Parole {
        if t >= self.fin {
            return Parole::Apres;
        }
        let mut groupe = 0;
        for (k, &(a, b)) in self.pauses.iter().enumerate() {
            if t >= b {
                groupe = k + 1;
            } else if t >= a {
                return Parole::Pause(k);
            }
        }
        Parole::Groupe(groupe)
    }
}

/// Le souffle, d'une image à l'autre : l'air qui se voit (sur son ressort), la courbe en cours et si elle inspire,
/// quand vient la suite ; ce que faisait Eve ; quand elle parle, le plan de sa phrase et où elle en est ; une
/// expiration demandée (celle d'un soupir : jusqu'où, en combien de temps) ; jusqu'à quand elle rit au bout de sa
/// phrase ; les respirations depuis la dernière profonde, et quand viendra la suivante ; le hasard de ses durées.
#[derive(Default)]
pub(super) struct Souffle {
    air: Ressort,
    courbe: Passage,
    inspire: bool,
    prochain: f64,
    activite: Activite,
    plan: Option<Plan>,
    parole: Option<Parole>,
    expiration: Option<(f32, f32)>,
    rire_jusqua: f64,
    souffles: u32,
    profonde_dans: u32,
    hasard: Hasard,
}

impl Souffle {
    /// L'air à cette image (0 : au bout d'une expiration au repos ; 1 : au haut d'une inspiration au repos), Eve à son
    /// activité ; `voix` : quand elle parle, la phrase qui s'entend — où elle en est (en secondes depuis son début) et
    /// ses repères ; rien ne s'entend encore : `None`.
    pub(super) fn avancer(&mut self, m: &Moment, voix: Option<&(f32, Arc<[Repere]>)>) -> f32 {
        if m.activite != self.activite {
            let avant = std::mem::replace(&mut self.activite, m.activite);
            if avant == Activite::Parle {
                // Elle a fini de parler : le rythme reprend après le passage en cours.
                (self.plan, self.parole) = (None, None);
                self.prochain = self.courbe.fin.max(m.temps);
            }
        }
        if m.parle() {
            self.parler(m, voix);
        } else {
            self.respirer(m);
        }
        self.air.vers(self.courbe.en(m.temps + RETARD_DE_L_AIR), W_AIR, m.dt)
    }

    /// Une autre courbe, de l'air du moment à `vers`, en `duree` secondes ; `inspire` : elle inspire.
    fn aller(&mut self, t: f64, vers: f32, duree: f32, inspire: bool) {
        let de = self.courbe.en(t);
        self.courbe = Passage { de, vers, debut: t, fin: t + f64::from(duree.max(0.05)) };
        self.inspire = inspire;
    }

    /// Une durée d'un peu moins à un peu plus (de 85 % à 115 %) : elle ne respire pas comme un métronome.
    fn varier(&mut self, x: f32) -> f32 {
        x * (0.85 + 0.3 * self.hasard.tirer() as f32)
    }

    /// Le rythme de l'activité (au repos, en écoutant, en réfléchissant) : à la fin d'une inspiration, l'expiration —
    /// celle d'un soupir, si elle est demandée —, puis un temps d'arrêt ; à la fin de l'arrêt, une inspiration, de loin
    /// en loin plus profonde. Les durées suivent l'air à prendre ou à rendre : une respiration plus profonde est plus
    /// longue.
    fn respirer(&mut self, m: &Moment) {
        if m.temps < self.prochain {
            return;
        }
        let r = match m.activite {
            Activite::Ecoute => ECOUTE,
            Activite::Reflechit => REFLEXION,
            Activite::Repos | Activite::Parle => REPOS,
        };
        let air = self.courbe.en(m.temps);
        if self.inspire {
            let (vers, duree) = match self.expiration.take() {
                Some(e) => e,
                None => {
                    let vers = 0.06 * self.hasard.tirer() as f32;
                    (vers, self.varier(r.expire) * ((air - vers) / r.ampleur).clamp(0.6, 1.6))
                }
            };
            self.aller(m.temps, vers, duree, false);
            let arret = self.varier(r.arret);
            self.prochain = self.courbe.fin + f64::from(arret);
        } else {
            if self.profonde_dans == 0 {
                self.profonde_dans = 5 + (3.0 * self.hasard.tirer()) as u32;
            }
            self.souffles += 1;
            let (ampleur, lent) = if self.souffles >= self.profonde_dans {
                (self.souffles, self.profonde_dans) = (0, 5 + (3.0 * self.hasard.tirer()) as u32);
                (PROFONDE, PLUS_LENTE)
            } else {
                (1.0, 1.0)
            };
            let vers = r.ampleur * ampleur * (0.9 + 0.2 * self.hasard.tirer() as f32);
            let duree = self.varier(r.inspire * lent) * ((vers - air) / r.ampleur).clamp(0.5, 1.5);
            self.aller(m.temps, vers, duree, true);
            self.prochain = self.courbe.fin + 0.05 + 0.15 * self.hasard.tirer();
        }
    }

    /// Elle parle : où elle en est de sa phrase (rien ne s'entend encore, un groupe de mots, une longue pause, la voix
    /// tue) ; à chaque changement, son souffle ([`Souffle::entrer`]) ; et ce qui se prolonge ([`Souffle::continuer`]).
    fn parler(&mut self, m: &Moment, voix: Option<&(f32, Arc<[Repere]>)>) {
        let (ici, t) = match voix {
            None => {
                self.plan = None;
                (Parole::Avant, 0.0)
            }
            Some((t, reperes)) => {
                if !self.plan.as_ref().is_some_and(|p| Arc::ptr_eq(&p.reperes, reperes)) {
                    // Une autre phrase : tout recommence.
                    (self.plan, self.parole) = (Some(Plan::de(reperes)), None);
                }
                (self.plan.as_ref().map_or(Parole::Groupe(0), |p| p.ou(*t)), *t)
            }
        };
        if self.parole != Some(ici) {
            self.parole = Some(ici);
            self.entrer(m.temps, ici, t);
        } else if m.temps >= self.prochain {
            self.continuer(m.temps, ici);
        }
    }

    /// Le souffle de ce qu'elle commence à dire, à l'instant `temps`, `t` secondes après le début de la phrase.
    fn entrer(&mut self, temps: f64, ici: Parole, t: f32) {
        let air = self.courbe.en(temps);
        match ici {
            Parole::Avant => {
                // Elle va parler : elle inspire, vivement, puis retient son souffle — à moins qu'elle n'en prenne déjà
                // plus (un soupir, le souffle coupé d'une surprise).
                if !(self.inspire && self.courbe.vers >= PAROLE) {
                    let duree = INSPIRATION_AVANT * ((PAROLE - air) / 0.9).clamp(0.5, 1.2);
                    self.aller(temps, PAROLE.max(air), duree, true);
                }
                self.prochain = self.courbe.fin + f64::from(TENUE);
            }
            Parole::Groupe(k) => {
                // Un groupe de mots d'un même souffle : la poitrine redescend doucement jusqu'à la pause qui le suit
                // (ou la fin de la voix). L'expiration d'un soupir n'a plus lieu : la voix a pris son air.
                self.expiration = None;
                let fin = self.plan.as_ref().map_or(f32::INFINITY, |p| p.pauses.get(k).map_or(p.fin, |&(a, _)| a));
                let duree = (fin - t).clamp(0.2, 8.0);
                self.aller(temps, (air - DEBIT * duree).max(FOND.min(air)), duree, false);
                self.prochain = f64::INFINITY;
            }
            Parole::Pause(k) => {
                // Une longue pause : une petite inspiration, le temps qu'elle dure.
                let reprise = self.plan.as_ref().and_then(|p| p.pauses.get(k)).map_or(t + 0.4, |&(_, b)| b);
                let vers = (air + REPRISE).min(ENTRE_DEUX).max(air);
                self.aller(temps, vers, (reprise - t - 0.05).clamp(0.15, 0.42), true);
                self.prochain = f64::INFINITY;
            }
            Parole::Apres => {
                // La voix s'est tue : l'air finit de sortir — tout le temps qu'elle rit, s'il le faut : le rire chasse
                // l'air —, puis elle le reprend ([`Souffle::continuer`]).
                let (vers, duree) = if self.rire_jusqua > temps {
                    ((air - 0.35).max(FOND.min(air) - 0.15), (self.rire_jusqua - temps) as f32)
                } else {
                    ((air - 0.12).max(FOND.min(air)), 0.25)
                };
                self.aller(temps, vers, duree, false);
                self.prochain = self.courbe.fin + 0.05;
            }
        }
    }

    /// Ce qui se prolonge : la voix tarde (elle relâche un peu son souffle, puis inspire de nouveau) ; la voix s'est
    /// tue (elle reprend son air, et le garde jusqu'à la phrase suivante, ou jusqu'à ce qu'elle se taise).
    fn continuer(&mut self, temps: f64, ici: Parole) {
        match ici {
            Parole::Avant if self.inspire => {
                self.aller(temps, ATTENTE, 1.3, false);
                self.prochain = self.courbe.fin + 0.1;
            }
            Parole::Avant => {
                self.aller(temps, PAROLE, INSPIRATION_AVANT * 0.8, true);
                self.prochain = self.courbe.fin + f64::from(TENUE);
            }
            Parole::Apres if !self.inspire => {
                self.aller(temps, ENTRE_DEUX, 0.42, true);
                self.prochain = f64::INFINITY;
            }
            _ => self.prochain = f64::INFINITY,
        }
    }

    /// Elle dit une phrase : sa voix tient son souffle, ni soupir ni surprise ne le prennent.
    fn dit(&self) -> bool {
        matches!(self.parole, Some(Parole::Groupe(_) | Parole::Pause(_) | Parole::Apres))
    }

    /// Un soupir, à l'instant `temps`, de tant d'`air` : une inspiration profonde, un instant retenue, puis une longue
    /// expiration, plus bas qu'au repos — les épaules retombent. Juste avant de parler (l'émotion d'une phrase arrive
    /// avec elle), l'inspiration seule, aussi vive que celle d'avant la voix, qui prend la suite : elle dit sa peine en
    /// rendant cet air. Faux : elle dit une phrase, pas de soupir.
    pub(super) fn soupirer(&mut self, temps: f64, air: f32) -> bool {
        if self.dit() {
            return false;
        }
        let de = self.courbe.en(temps);
        if self.parole == Some(Parole::Avant) {
            self.aller(temps, air.max(de), INSPIRATION_AVANT * ((air - de) / 0.9).clamp(0.5, 1.3), true);
            self.prochain = self.courbe.fin + f64::from(TENUE);
        } else {
            self.aller(temps, air.max(de), INSPIRATION_DU_SOUPIR, true);
            self.prochain = self.courbe.fin + TENUE_DU_SOUPIR;
            self.expiration = Some((AU_BOUT_DU_SOUPIR, EXPIRATION_DU_SOUPIR));
        }
        true
    }

    /// Elle rira au bout de la phrase qu'elle dit, jusqu'à l'instant `jusqua` : l'air sort pendant son rire, elle
    /// n'inspire qu'après.
    pub(super) fn rire(&mut self, jusqua: f64) {
        self.rire_jusqua = jusqua;
    }

    /// Le souffle coupé d'une surprise, à l'instant `temps` : une inspiration vive, retenue une demi-seconde. Faux :
    /// elle dit une phrase.
    pub(super) fn couper(&mut self, temps: f64) -> bool {
        if self.dit() {
            return false;
        }
        let vers = (self.courbe.en(temps) + 0.6).clamp(1.1, 1.45);
        self.aller(temps, vers, 0.22, true);
        self.prochain = self.courbe.fin + 0.55;
        true
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    fn repere(a: f32, marque: Marque) -> Repere {
        Repere { a, marque }
    }

    /// L'air à chaque image, à 60 images par seconde, de `debut` pendant `duree` secondes, Eve à son `activite` ;
    /// `voix` : la phrase qui s'entend depuis l'instant qu'elle dit (et ses repères) — entendue tant que sa durée n'est
    /// pas passée.
    fn filmer(
        s: &mut Souffle,
        (debut, duree): (f64, f64),
        activite: Activite,
        voix: Option<(f64, f64, &Arc<[Repere]>)>,
    ) -> Vec<(f64, f32)> {
        let dt = 1.0 / 60.0;
        (0..(duree / dt).round() as usize)
            .map(|k| {
                let temps = debut + k as f64 * dt;
                let m = Moment { temps, dt: dt as f32, activite, vivant: true, ..Default::default() };
                let entendue = voix
                    .filter(|&(depuis, longueur, _)| temps >= depuis && temps < depuis + longueur)
                    .map(|(depuis, _, r)| ((temps - depuis) as f32, r.clone()));
                (temps, s.avancer(&m, entendue.as_ref()))
            })
            .collect()
    }

    /// Rien ne saute d'une image à l'autre : l'air va toujours en douceur — au plus vite (avant de parler, un soupir),
    /// 0,07 par image, un dixième de pixel au film.
    fn sans_saut(images: &[(f64, f32)]) {
        for w in images.windows(2) {
            assert!((w[1].1 - w[0].1).abs() < 0.07, "à {:.2} s : de {} à {}", w[1].0, w[0].1, w[1].1);
        }
    }

    /// Les hauts de chaque respiration : l'instant et l'air de chaque maximum qui domine sa seconde.
    fn hauts(images: &[(f64, f32)]) -> Vec<(f64, f32)> {
        (30..images.len().saturating_sub(30))
            .filter(|&i| {
                let v = images[i].1;
                v > 0.3 && images[i - 30..=i + 30].iter().all(|x| x.1 <= v) && images[i - 1].1 < v
            })
            .map(|i| images[i])
            .collect()
    }

    #[test]
    fn au_repos_un_rythme_humain_jamais_tout_a_fait_le_meme() {
        // Deux minutes au repos : de onze à dix-sept respirations par minute, chacune d'environ 1, une plus profonde
        // de loin en loin ; des cycles qui ne se ressemblent pas tout à fait ; rien qui saute, rien sous le repos.
        let mut s = Souffle::default();
        let images = filmer(&mut s, (10.0, 120.0), Activite::Repos, None);
        sans_saut(&images);
        let h = hauts(&images);
        assert!((22..=34).contains(&h.len()), "{} respirations en deux minutes : {h:?}", h.len());
        assert!(h.iter().filter(|x| (0.8..1.2).contains(&x.1)).count() * 10 >= h.len() * 7, "{h:?}");
        assert!(h.iter().any(|x| x.1 >= 1.2), "une plus profonde : {h:?}");
        let ecarts: Vec<f64> = h.windows(2).map(|w| w[1].0 - w[0].0).collect();
        let (court, long) = ecarts.iter().fold((f64::MAX, 0.0f64), |(a, b), &e| (a.min(e), b.max(e)));
        assert!(long / court > 1.15, "des cycles de {court:.2} à {long:.2} s");
        assert!(images.iter().all(|x| x.1 > -0.01 && x.1 < 1.6));
    }

    #[test]
    fn en_ecoutant_plus_court_et_moins_profond() {
        let mut repos = Souffle::default();
        let mut ecoute = Souffle::default();
        let a = hauts(&filmer(&mut repos, (0.0, 60.0), Activite::Repos, None));
        let b = hauts(&filmer(&mut ecoute, (0.0, 60.0), Activite::Ecoute, None));
        let moyenne = |h: &[(f64, f32)]| h.iter().map(|x| x.1).sum::<f32>() / h.len() as f32;
        assert!(b.len() >= a.len() + 4, "{} respirations en écoutant, {} au repos", b.len(), a.len());
        assert!(moyenne(&b) < 0.8 * moyenne(&a), "{} {}", moyenne(&b), moyenne(&a));
    }

    #[test]
    fn elle_inspire_avant_de_parler_et_expire_en_parlant() {
        // Au repos six secondes ; puis « parole » une demi-seconde avant le son ; une phrase de quatre secondes, une
        // longue pause de 2,0 à 2,5 s (entre deux phrases), sa dernière syllabe à 3,7 s ; puis le silence.
        let reperes: Arc<[Repere]> = [
            repere(0.9, Marque::Groupe),
            repere(1.9, Marque::Fin),
            repere(2.0, Marque::Pause),
            repere(2.5, Marque::Reprise),
            repere(3.7, Marque::Exclamation),
        ]
        .into();
        let mut s = Souffle::default();
        let mut images = filmer(&mut s, (0.0, 6.0), Activite::Repos, None);
        let avant = images.last().unwrap().1;
        images.extend(filmer(&mut s, (6.0, 0.5), Activite::Parle, None));
        images.extend(filmer(&mut s, (6.5, 4.2), Activite::Parle, Some((6.5, 4.2, &reperes))));
        images.extend(filmer(&mut s, (10.7, 4.0), Activite::Repos, None));
        sans_saut(&images);
        let a = |t: f64| images.iter().find(|x| x.0 >= t).unwrap().1;
        // Avant le son : l'air monte vite, au-dessus d'une inspiration au repos.
        assert!(a(6.5) > 1.05 && a(6.5) >= avant, "{avant} puis {}", a(6.5));
        // Le premier groupe : la poitrine redescend, sans remonter.
        let groupe: Vec<f32> = images.iter().filter(|x| (6.75..8.5).contains(&x.0)).map(|x| x.1).collect();
        assert!(groupe.windows(2).all(|w| w[1] <= w[0] + 1e-4), "elle remonte en parlant");
        assert!(groupe[0] - groupe[groupe.len() - 1] > 0.3, "{:?}", (groupe[0], groupe[groupe.len() - 1]));
        // La longue pause : une petite inspiration ; puis elle redescend jusqu'au bout de la voix.
        assert!(a(9.0) > a(8.5) + 0.2, "{} {}", a(8.5), a(9.0));
        assert!(a(10.3) < a(9.1) - 0.2, "{} {}", a(9.1), a(10.3));
    }

    #[test]
    fn si_la_voix_tarde_elle_ne_retient_pas_son_souffle_indefiniment() {
        let mut s = Souffle::default();
        filmer(&mut s, (0.0, 5.0), Activite::Repos, None);
        let images = filmer(&mut s, (5.0, 6.0), Activite::Parle, None);
        sans_saut(&images);
        // Elle a inspiré, puis relâche un peu, puis inspire de nouveau : jamais retenue plus d'une seconde et demie.
        let haut = images.iter().map(|x| x.1).fold(0.0, f32::max);
        let (mut suite, mut tenue) = (0, 0);
        for x in &images {
            suite = if x.1 > haut - 0.02 { suite + 1 } else { 0 };
            tenue = tenue.max(suite);
        }
        assert!(haut > 1.1 && tenue < 90, "{haut}, tenu {tenue} images");
        assert!(images.iter().skip(150).any(|x| x.1 < haut - 0.3), "elle relâche");
    }

    #[test]
    fn un_soupir_puis_les_epaules_retombent() {
        let mut s = Souffle::default();
        filmer(&mut s, (0.0, 7.0), Activite::Repos, None);
        assert!(s.soupirer(7.0, 1.75));
        let images = filmer(&mut s, (7.0, 6.0), Activite::Repos, None);
        sans_saut(&images);
        let (quand, haut) = images.iter().fold((0.0, 0.0f32), |m, &(t, v)| if v > m.1 { (t, v) } else { m });
        assert!(haut > 1.6 && quand < 8.3, "{haut} à {quand}");
        assert!(images.iter().any(|x| x.0 < 11.0 && x.1 < -0.1), "plus bas qu'au repos");
        // Pendant une phrase, pas de soupir : la voix tient le souffle.
        let reperes: Arc<[Repere]> = [repere(1.0, Marque::Fin)].into();
        let mut d = Souffle::default();
        filmer(&mut d, (0.0, 0.5), Activite::Parle, Some((0.0, 2.0, &reperes)));
        let avant = d.courbe;
        assert!(!d.soupirer(0.5, 1.75) && !d.couper(0.5));
        assert!(d.courbe.vers == avant.vers && d.courbe.debut == avant.debut);
        // Juste avant de parler (« parole » et l'émotion arrivent ensemble), le soupir inspire aussi vite que la voix :
        // à 0,4 s, quand le son vient, la poitrine est déjà bien plus haute qu'avant une phrase ordinaire.
        let mut v = Souffle::default();
        filmer(&mut v, (0.0, 7.0), Activite::Repos, None);
        filmer(&mut v, (7.0, 1.0 / 60.0), Activite::Parle, None);
        assert!(v.soupirer(7.0 + 1.0 / 60.0, 1.75));
        let images = filmer(&mut v, (7.0 + 2.0 / 60.0, 0.4), Activite::Parle, None);
        sans_saut(&images);
        assert!(images.last().unwrap().1 > 1.3, "{}", images.last().unwrap().1);
    }

    #[test]
    fn la_surprise_coupe_le_souffle() {
        let mut s = Souffle::default();
        filmer(&mut s, (0.0, 8.0), Activite::Repos, None);
        let avant = s.courbe.en(8.0);
        assert!(s.couper(8.0));
        let images = filmer(&mut s, (8.0, 1.0), Activite::Repos, None);
        sans_saut(&images);
        let a = |t: f64| images.iter().find(|x| x.0 >= t).unwrap().1;
        assert!(a(8.35) > avant + 0.3 && a(8.35) >= 1.0, "{avant} puis {}", a(8.35));
        assert!((a(8.7) - a(8.4)).abs() < 0.08, "retenu : {} {}", a(8.4), a(8.7));
    }
}
