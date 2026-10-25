//! Les émotions du visage d'Eve : celle du passage qu'elle dit ([`Emotion`], que Claude choisit et que le chef publie
//! quand ce passage se dit), et son air pensif pendant qu'elle réfléchit ; le visage la prend en un peu plus d'une
//! demi-seconde pendant que celle d'avant s'efface (en un quart de seconde quand le ton change pendant qu'elle parle, ou
//! que ses yeux changent) — revenir à celle qui s'effaçait la reprend où elle en était ; et le penchant qu'elles donnent
//! à la tête.
//!
//! Une émotion ne se tient pas comme un masque : quand Eve se tait, elle la garde le temps qu'une personne la garderait
//! ([`tenue`] : un éclat de surprise, une seconde et demie ; une peine, un quart de minute), puis revient au calme ;
//! quand on lui parle, elle écoute, attentive — elle ne garde que la peine, le souci et le sérieux ([`reste_a_l_ecoute`]).
//! Pendant qu'elle réfléchit, en silence ou tout haut, son air pensif va et vient : elle détourne les yeux pour chercher,
//! puis revient vers la personne.

use eve::parole::Emotion;

use super::mimique::{Hasard, Moment, Ressort};
use super::{Activite, Pose, Strate, Tete, Visage};

/// La pulsation de l'émotion qui vient et de celle qui s'en va (aux neuf dixièmes en 0,65 et 0,55 s) ; et d'un
/// changement vif (en un quart de seconde) : quand le ton change pendant qu'Eve parle — chez une personne, d'un mot à
/// l'autre, et deux bouches mêlées se verraient le temps d'un fondu lent —, ou que son regard change (l'air pensif :
/// les yeux vont d'un coup, comme l'œil).
const W_VIENT: f32 = 6.0;
const W_PART: f32 = 7.0;
const W_VIF: f32 = 16.0;

/// Le rythme de l'air pensif qui va et vient : combien de temps il est là, combien il s'absente (en secondes : le moins,
/// et ce que le hasard peut y ajouter).
#[derive(Clone, Copy, PartialEq)]
struct Rythme {
    la: (f64, f64),
    absent: (f64, f64),
}

/// Pendant qu'elle réfléchit en silence (Claude travaille) : pensive 2,5 à 6 s, puis vers la personne 1 à 3 s.
const EN_SILENCE: Rythme = Rythme { la: (2.5, 3.5), absent: (1.0, 2.0) };
/// Pendant qu'elle réfléchit tout haut (une réponse [reflexion]) : elle détourne les yeux le temps de chercher, 0,8 à
/// 1,8 s, puis parle à la personne 2,5 à 5,5 s — des yeux fixés de côté et les sourcils froncés toute une phrase
/// semblaient inquiets.
const TOUT_HAUT: Rythme = Rythme { la: (0.8, 1.0), absent: (2.5, 3.0) };

/// Le temps qu'Eve garde une émotion, une fois tue, en secondes : ce qu'une personne garderait — l'éclat d'une surprise,
/// un rire, une joie, quelques secondes ; une peine ou un souci, bien plus (passer d'une mauvaise nouvelle au sourire de
/// la photo, aussitôt dite, serait froid). Une réflexion tout haut, rien : elle a fini de chercher.
fn tenue(e: Emotion) -> f64 {
    match e {
        Emotion::Surprise => 1.5,
        Emotion::Reflexion | Emotion::Neutre => 0.0,
        Emotion::Amusee => 4.0,
        Emotion::Joie => 5.0,
        Emotion::Serieuse => 8.0,
        Emotion::Empathie | Emotion::Triste => 15.0,
    }
}

/// Les émotions qu'Eve garde en écoutant : la peine, le souci, le sérieux ; les autres s'effacent, elle écoute.
fn reste_a_l_ecoute(e: Emotion) -> bool {
    matches!(e, Emotion::Empathie | Emotion::Triste | Emotion::Serieuse)
}

/// Les émotions, d'une image à l'autre : celle qui vient (ou qui est là) et sa part, celle qui s'efface et la sienne, si
/// leur passage est vif ; l'émotion publiée, depuis quand Eve se tait après elle, et si le visage l'a quittée ; l'air
/// pensif qui va et vient (son rythme, s'il est là, jusqu'à quand), et le hasard de ses durées.
#[derive(Default)]
pub(super) struct Emotions {
    emotion: Option<Emotion>,
    part: Ressort,
    avant: Option<Emotion>,
    part_avant: Ressort,
    vif: bool,
    publiee: Option<Emotion>,
    calme_depuis: Option<f64>,
    quittee: bool,
    pensive: Option<(Rythme, bool)>,
    pensive_jusqua: f64,
    hasard: Hasard,
}

impl Emotions {
    /// Les émotions à cette image : `publiee` (celle du passage qu'Eve dit, que le chef publie), tenue tant qu'elle
    /// parle, puis le temps de sa [`tenue`] ; quittée quand on lui parle, sauf une peine ou un souci ; pendant qu'elle
    /// réfléchit, en silence ou tout haut, son air pensif qui va et vient. Sans sa pose, aucune ; leurs poses, celle qui
    /// s'efface sous celle qui vient.
    pub(super) fn avancer(&mut self, m: &Moment, v: &Visage, publiee: Option<Emotion>, pose: &mut Pose) {
        if publiee != self.publiee {
            (self.publiee, self.calme_depuis, self.quittee) = (publiee, None, false);
        }
        match m.activite {
            Activite::Parle | Activite::Reflechit => self.calme_depuis = None,
            Activite::Ecoute if publiee.is_some_and(|e| !reste_a_l_ecoute(e)) => self.quittee = true,
            Activite::Ecoute | Activite::Repos => {
                self.calme_depuis.get_or_insert(m.temps);
            }
        }
        if let (Some(e), Some(depuis)) = (publiee, self.calme_depuis)
            && m.temps - depuis >= tenue(e)
        {
            self.quittee = true;
        }
        let tenue = publiee.filter(|_| !self.quittee);
        // L'air pensif qui va et vient : pendant qu'elle réfléchit en silence (entre deux, l'émotion de la réponse), et
        // pendant qu'elle réfléchit tout haut, une réponse [reflexion] (entre deux, elle parle à la personne).
        let voulue = match m.activite {
            Activite::Reflechit => self.pensive(m, EN_SILENCE).or(tenue),
            Activite::Parle if tenue == Some(Emotion::Reflexion) => self.pensive(m, TOUT_HAUT),
            _ => {
                self.pensive = None;
                tenue
            }
        };
        let voulue = voulue.filter(|e| v.expressions.iter().any(|x| x.emotion == *e));
        if voulue != self.emotion {
            let pensive = Some(Emotion::Reflexion);
            self.vif = m.parle() || voulue == pensive || self.emotion == pensive;
            if voulue.is_some() && voulue == self.avant {
                std::mem::swap(&mut self.emotion, &mut self.avant);
                std::mem::swap(&mut self.part, &mut self.part_avant);
            } else {
                if self.part.x >= self.part_avant.x {
                    (self.avant, self.part_avant) = (self.emotion, self.part);
                }
                (self.emotion, self.part) = (voulue, Ressort::default());
            }
        }
        let (vient, part) = if self.vif { (W_VIF, W_VIF) } else { (W_VIENT, W_PART) };
        self.part.vers(if self.emotion.is_some() { 1.0 } else { 0.0 }, vient, m.dt);
        self.part_avant.vers(0.0, part, m.dt);
        if self.part_avant.x < 0.004 {
            self.avant = None;
        }
        for (e, part) in [(self.avant, self.part_avant.x), (self.emotion, self.part.x)] {
            if let Some(x) = e.and_then(|e| v.expressions.iter().find(|x| x.emotion == e)) {
                pose.peindre(Strate::Emotion, &x.visage, part);
            }
        }
    }

    /// L'air pensif qui va et vient, à son `rythme` : d'abord là, puis absent — elle revient vers la personne —, puis là
    /// de nouveau, et ainsi de suite ; il repart du début quand le rythme change. Sans les animations de Windows, il reste.
    fn pensive(&mut self, m: &Moment, rythme: Rythme) -> Option<Emotion> {
        let duree = |(min, en_plus): (f64, f64), h: f64| min + en_plus * h;
        let pensive = match self.pensive {
            Some((r, p)) if r == rythme => {
                if m.vivant && m.temps >= self.pensive_jusqua {
                    let h = self.hasard.tirer();
                    self.pensive_jusqua = m.temps + duree(if p { rythme.absent } else { rythme.la }, h);
                    !p
                } else {
                    p
                }
            }
            _ => {
                self.pensive_jusqua = m.temps + duree(rythme.la, self.hasard.tirer());
                true
            }
        };
        self.pensive = Some((rythme, pensive));
        pensive.then_some(Emotion::Reflexion)
    }

    /// L'émotion qui domine : celle qui se voit le plus.
    pub(super) fn dominante(&self) -> Option<Emotion> {
        if self.part.x >= self.part_avant.x { self.emotion } else { self.avant }
    }

    /// Le penchant que les émotions donnent à la tête, chacune selon sa part.
    pub(super) fn penchant(&self) -> Tete {
        [(self.emotion, self.part.x), (self.avant, self.part_avant.x)].into_iter().fold(
            Tete::default(),
            |t, (e, part)| {
                let p = penchant(e);
                Tete { roulis: t.roulis + p.roulis * part, x: t.x + p.x * part, y: t.y + p.y * part }
            },
        )
    }

    /// Les émotions posées : plus aucune ne s'efface, celle qui est là est entière (ou aucune).
    pub(super) fn posees(&self) -> bool {
        self.avant.is_none() && (self.part.x - if self.emotion.is_some() { 1.0 } else { 0.0 }).abs() < 0.005
    }
}

/// Le penchant de la tête que donne une émotion : penchée par empathie, basse de tristesse, levée de surprise, de côté
/// en réfléchissant.
fn penchant(e: Option<Emotion>) -> Tete {
    let (roulis, x, y) = match e {
        Some(Emotion::Empathie) => (0.05, 0.008, 0.008),
        Some(Emotion::Triste) => (-0.02, 0.0, 0.03),
        Some(Emotion::Surprise) => (0.0, 0.0, -0.015),
        Some(Emotion::Reflexion) => (-0.03, -0.008, -0.008),
        _ => (0.0, 0.0, 0.0),
    };
    Tete { roulis, x, y }
}

#[cfg(test)]
mod essais {
    use eframe::egui;

    use super::super::{Calque, Expression};
    use super::*;

    /// Un visage d'essai : un pixel pour la photo et pour chaque calque ; la joie, la tristesse et l'air pensif, sans
    /// bouches.
    fn visage() -> Visage {
        let ctx = egui::Context::default();
        let blanc = std::sync::Arc::new(egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]));
        let pixel = |nom: &str| ctx.load_texture(nom, blanc.clone(), egui::TextureOptions::default());
        let entiere = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
        let calque = |nom: &str| Calque {
            texture: pixel(nom),
            image: blanc.clone(),
            uv: entiere,
            geometrie: Default::default(),
        };
        let expressions = [Emotion::Joie, Emotion::Triste, Emotion::Reflexion]
            .map(|emotion| Expression {
                emotion,
                visage: calque(emotion.nom()),
                bouches: Vec::new(),
                levres_closes: None,
            })
            .into();
        Visage {
            texture: pixel("photo"),
            image: blanc.clone(),
            taille: [1, 1],
            calques: Default::default(),
            expressions,
            fond: None,
            cadre: entiere,
            demande: Default::default(),
            net: None,
            vignettes: Default::default(),
        }
    }

    /// Les émotions avancées de `secondes` depuis `t`, soixante images par seconde, Eve à son `activite`, `publiee`
    /// publiée par le chef.
    fn avancer(e: &mut Emotions, v: &Visage, t: &mut f64, secondes: f64, activite: Activite, publiee: Option<Emotion>) {
        for _ in 0..(secondes * 60.0).round() as usize {
            *t += 1.0 / 60.0;
            let m = Moment { temps: *t, dt: 1.0 / 60.0, activite, vivant: true, ..Default::default() };
            e.avancer(&m, v, publiee, &mut Pose::default());
        }
    }

    #[test]
    fn une_emotion_se_garde_le_temps_qu_il_faut() {
        let v = visage();
        let (joie, triste) = (Some(Emotion::Joie), Some(Emotion::Triste));
        // Elle dit sa joie, puis se tait : la joie reste cinq secondes, puis le calme revient, et elle ne revient pas.
        let (mut e, mut t) = (Emotions::default(), 0.0);
        avancer(&mut e, &v, &mut t, 2.0, Activite::Parle, joie);
        assert_eq!(e.emotion, joie);
        avancer(&mut e, &v, &mut t, 4.8, Activite::Repos, joie);
        assert_eq!(e.emotion, joie, "encore là");
        avancer(&mut e, &v, &mut t, 0.4, Activite::Repos, joie);
        assert_eq!(e.emotion, None, "partie");
        avancer(&mut e, &v, &mut t, 1.5, Activite::Repos, joie);
        assert!(e.posees() && e.dominante().is_none());
        // On lui parle : la joie s'efface aussitôt ; la tristesse reste.
        let mut f = Emotions::default();
        avancer(&mut f, &v, &mut t, 1.0, Activite::Parle, joie);
        avancer(&mut f, &v, &mut t, 0.1, Activite::Ecoute, joie);
        assert_eq!(f.emotion, None);
        let mut g = Emotions::default();
        avancer(&mut g, &v, &mut t, 1.0, Activite::Parle, triste);
        avancer(&mut g, &v, &mut t, 10.0, Activite::Ecoute, triste);
        assert_eq!(g.emotion, triste);
        // Un autre passage pendant qu'elle parle : vif, presque entier en un quart de seconde.
        avancer(&mut g, &v, &mut t, 0.25, Activite::Parle, joie);
        assert!(g.emotion == joie && g.part.x > 0.85, "{}", g.part.x);
    }

    #[test]
    fn l_air_pensif_va_et_vient() {
        // Elle réfléchit : pensive d'abord (deux secondes et demie au moins), puis elle revient vers la personne, et
        // repense. Sans les animations de Windows, elle reste pensive.
        let v = visage();
        let pensive = Some(Emotion::Reflexion);
        let (mut e, mut t) = (Emotions::default(), 0.0);
        let mut vues = Vec::new();
        for _ in 0..20 * 60 {
            avancer(&mut e, &v, &mut t, 1.0 / 60.0, Activite::Reflechit, None);
            vues.push(e.emotion);
        }
        assert!(vues[..150].iter().all(|x| *x == pensive), "pensive d'abord");
        let changements = vues.windows(2).filter(|w| w[0] != w[1]).count();
        assert!((4..=16).contains(&changements), "elle va et vient : {changements} changements en 20 s");
        let (mut f, mut t) = (Emotions::default(), 0.0);
        for _ in 0..20 * 60 {
            t += 1.0 / 60.0;
            let m =
                Moment { temps: t, dt: 1.0 / 60.0, activite: Activite::Reflechit, vivant: false, ..Default::default() };
            f.avancer(&m, &v, None, &mut Pose::default());
            assert_eq!(f.emotion, pensive);
        }
    }

    #[test]
    fn elle_reflechit_tout_haut_en_regardant_la_personne() {
        // Une réponse [reflexion] : en parlant, les yeux de côté d'abord (plus d'une demi-seconde), puis vers la personne
        // (avant trois secondes) ; tue, elle ne garde pas l'air pensif.
        let v = visage();
        let pensive = Some(Emotion::Reflexion);
        let (mut e, mut t) = (Emotions::default(), 0.0);
        avancer(&mut e, &v, &mut t, 0.7, Activite::Parle, pensive);
        assert_eq!(e.emotion, pensive);
        avancer(&mut e, &v, &mut t, 1.5, Activite::Parle, pensive);
        assert_eq!(e.emotion, None, "elle parle à la personne");
        let (mut f, mut t) = (Emotions::default(), 0.0);
        avancer(&mut f, &v, &mut t, 1.0, Activite::Parle, pensive);
        avancer(&mut f, &v, &mut t, 0.1, Activite::Repos, pensive);
        assert_eq!(f.emotion, None);
    }
}
