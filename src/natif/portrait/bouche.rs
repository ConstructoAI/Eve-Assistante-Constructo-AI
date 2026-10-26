//! La bouche du portrait d'Eve : elle suit sa voix, lue un peu en avance ([`crate::voix::bouche_a_venir`]). La mâchoire
//! s'ouvre selon la voyelle et le niveau de la voix ([`crate::voix::piste_de`]) — grande sur un a, petite sur un i ou
//! un ou, et elle se referme sur la consonne —, d'une pose à la suivante en fondu ; les lèvres sont ouvertes, rondes ou
//! étirées selon la syllabe, d'une famille de bouches à l'autre en fondu ; sur m, b, p, elles se ferment — arrondies
//! avant une voyelle ronde —, sur f et v, la lèvre du bas passe sous les dents, la mâchoire refermée avec elles. Quand
//! une émotion domine qui a les siennes, ses bouches à elle — rondes et étirées, et sa lèvre sous les dents, si elle en
//! a —, et ses lèvres closes : Eve parle sans quitter son émotion, et ses syllabes y gardent leur forme.
//!
//! Sans émotion, elle parle avec un sourire calme (« elle est trop souriante quand elle parle ») : le grand sourire de
//! la photo, toutes dents dehors, ne se garde pas en parlant. Pendant qu'elle écoute, réfléchit et parle, son visage
//! calme ([`Calme`] : le sourire adouci, les lèvres closes) se pose sous ses bouches, qui ont ce sourire-là ; entre
//! deux syllabes, la bouche revient à lui. Un moment après qu'elle s'est tue, le sourire de la photo revient,
//! lentement ; une émotion le couvre. Une émotion qui ne sourit pas (la peine, le souci, le sérieux, la surprise, l'air
//! pensif) le retient : quand elle s'efface, le sourire revient par lui, et non d'un coup toutes dents dehors.

use std::time::Duration;

use eve::parole::{Emotion, Forme};

use super::emotions::Emotions;
use super::mimique::{Moment, Ressort, lisse};
use super::{Activite, Calque, Pose, Strate, Visage};

/// Le passage d'une famille de bouches à une autre (d'une voyelle ronde à une ouverte, d'une émotion à l'autre), en
/// secondes ; puis la traîne de la famille d'avant : ce qu'il en reste sous la nouvelle (ce qu'elle ne couvre pas, une
/// bouche à demi ouverte) s'efface en ce temps, au lieu de disparaître d'une image à la suivante ; la fin du passage,
/// traîne comprise (en dixièmes de seconde, comme le passage).
const DUREE_DU_PASSAGE: f32 = 0.1;
const TRAINE_DU_PASSAGE: f32 = 0.05;
const FIN_DU_PASSAGE: f32 = 1.0 + TRAINE_DU_PASSAGE / DUREE_DU_PASSAGE;
/// L'avance de la bouche sur le son : les lèvres d'une vraie personne se placent avant que le son ne sorte, et le
/// ressort de la mâchoire suit sa voix avec un temps de retard. Réglée au film du portrait : la bouche y précède le son
/// d'environ 50 ms (à 60 ms, de 30 ms seulement, qu'une fenêtre à l'écran rattrape presque : elle affiche une image
/// une ou deux trames après l'avoir calculée).
const AVANCE_DE_LA_BOUCHE: Duration = Duration::from_millis(80);
/// Sur une consonne des lèvres, la bouche regarde la voyelle qui suit de ce temps de plus : elle s'y prépare pendant
/// que les lèvres jointes cachent le passage (on ferme les lèvres arrondies avant un « o »).
const ANTICIPATION: Duration = Duration::from_millis(120);
/// Le visage calme : le temps qu'il reste après qu'Eve s'est tue, ou qu'une émotion qui ne sourit pas s'est effacée, en
/// secondes — entre deux phrases, le grand sourire ne revient pas —, et ses pulsations : il vient aux neuf dixièmes en
/// 0,4 s, et s'en va en un peu plus d'une seconde (un sourire qui revient doucement, la phrase finie).
const TENUE_DU_CALME: f64 = 1.2;
const W_CALME_VIENT: f32 = 10.0;
const W_CALME_PART: f32 = 3.5;
/// Le fond des bouches rondes (leurs lèvres closes et arrondies, sous elles) : il vient quand Eve se met à parler et
/// s'en va quand elle se tait, aux neuf dixièmes en 0,2 s.
const W_FOND: f32 = 20.0;

/// La famille des bouches qui parlent : leur genre, d'après la voyelle, et leur visage — celui de la photo, ou celui
/// d'une émotion, qui a ses bouches à elle.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Famille {
    genre: Genre,
    emotion: Option<Emotion>,
}

/// Le genre d'une bouche qui parle : ouverte (a, è), ronde (o, ou, u), étirée (i, é).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Genre {
    #[default]
    Ouverte,
    Ronde,
    Etiree,
}

impl Visage {
    /// Les bouches d'une famille, de plus en plus ouvertes : celles du visage de la photo, ou celles d'une émotion —
    /// ses bouches pour parler, ou ses formes ([`super::Formes`] : rondes, étirées) si elle en a.
    pub(super) fn bouches(&self, f: Famille) -> &[Calque] {
        let [_, _, ouvertes, rondes, etirees, ..] = &self.calques;
        let Some(e) = f.emotion else {
            return match f.genre {
                Genre::Ouverte => ouvertes,
                Genre::Ronde => rondes,
                Genre::Etiree => etirees,
            };
        };
        let Some(x) = self.expressions.iter().find(|x| x.emotion == e) else { return &[] };
        match f.genre {
            Genre::Ouverte => &x.bouches,
            Genre::Ronde => &x.formes.rondes,
            Genre::Etiree => &x.formes.etirees,
        }
    }

    /// Ce que montrent les lèvres d'une famille sur une consonne des lèvres (`Forme::Fermee` ou `Forme::Dents`), parmi
    /// les lèvres jointes du visage : closes (`ferme-1`), closes et arrondies pour une famille ronde (`ferme-2`), la
    /// lèvre du bas sous les dents (`ferme-3`). Une émotion a ses lèvres closes, si sa pose ne les a pas déjà — closes
    /// et arrondies pour ses bouches rondes —, et sa lèvre sous les dents, si elle en a une (ses formes) ; sinon, sa
    /// bouche se referme sur sa pose.
    fn levres(&self, f: Famille, forme: Forme) -> Option<&Calque> {
        let [.., jointes, _] = &self.calques;
        if let Some(e) = f.emotion {
            let x = self.expressions.iter().find(|x| x.emotion == e)?;
            return match forme {
                Forme::Fermee if f.genre == Genre::Ronde => {
                    x.formes.closes_rondes.as_ref().or(x.levres_closes.as_ref())
                }
                Forme::Fermee => x.levres_closes.as_ref(),
                Forme::Dents => x.formes.dents.as_ref(),
                _ => None,
            };
        }
        match (f.genre, forme) {
            (Genre::Ronde, Forme::Fermee) => jointes.get(1).or(jointes.first()),
            (_, Forme::Fermee) => jointes.first(),
            (_, Forme::Dents) => jointes.get(2),
            _ => None,
        }
    }
}

/// La famille des bouches d'une voyelle de genre `genre`, l'émotion `dominante` sur le visage, parmi celles qui
/// existent (`existe`) : celles de l'émotion, si elle en a — de ce genre, sinon ses bouches ouvertes —, sinon celles
/// du visage de la photo, de ce genre s'il les a, sinon ouvertes.
fn famille(genre: Genre, dominante: Option<Emotion>, existe: impl Fn(Famille) -> bool) -> Famille {
    let ouvertes = |emotion| Famille { genre: Genre::Ouverte, emotion };
    let emotion = dominante.filter(|&e| existe(ouvertes(Some(e))));
    let voulue = Famille { genre, emotion };
    if existe(voulue) { voulue } else { ouvertes(emotion) }
}

/// La bouche, d'une image à l'autre : le visage calme, la mâchoire, la voix, les lèvres closes et la lèvre sous les
/// dents, le fond des bouches rondes (sur leurs ressorts), la forme de la dernière voyelle de la phrase, sa famille de
/// bouches, celle d'avant et le passage de l'une à l'autre.
#[derive(Default)]
pub(super) struct Bouche {
    calme: Calme,
    machoire: Ressort,
    voix: Ressort,
    levres_closes: Ressort,
    dents: Ressort,
    fond: Ressort,
    forme: Option<Forme>,
    famille: Famille,
    avant: Famille,
    passage: f32,
}

/// Le visage calme, d'une image à l'autre : sa part (sur son ressort), celle qu'il vise, jusqu'à quand il reste, et
/// s'il est tenu (Eve s'est tue : il partira à la fin de sa tenue).
#[derive(Default)]
struct Calme {
    part: Ressort,
    vise: f32,
    jusqua: f64,
    tenu: bool,
}

impl Calme {
    /// Sa part à cette image : là pendant qu'Eve écoute, réfléchit ou parle, ou qu'une émotion qui ne sourit pas le
    /// `retient`, et [`TENUE_DU_CALME`] après ; puis il s'en va.
    fn avancer(&mut self, m: &Moment, retient: bool) -> f32 {
        let en_conversation = m.activite != Activite::Repos || retient;
        if en_conversation {
            self.jusqua = m.temps + TENUE_DU_CALME;
        }
        self.vise = if m.temps < self.jusqua { 1.0 } else { 0.0 };
        self.tenu = !en_conversation && self.vise > 0.0;
        let w = if self.vise > self.part.x { W_CALME_VIENT } else { W_CALME_PART };
        self.part.vers(self.vise, w, m.dt).clamp(0.0, 1.0)
    }

    /// Posé : il est où il va, et n'en bougera pas de lui-même (aucune tenue ne court).
    fn pose(&self) -> bool {
        (self.part.x - self.vise).abs() < 0.005 && !self.tenu
    }
}

impl Bouche {
    /// La bouche à cette image : d'abord le visage calme, sous les émotions qui le couvrent ; puis la voix qui
    /// s'entendra dans un instant — l'ouverture de la mâchoire et la forme de sa syllabe ; la mâchoire s'ouvre en un
    /// dixième de seconde, se referme en un peu plus ; les lèvres se joignent en 50 ms, et la bouche ouverte s'efface
    /// sous elles. Ses bouches : celles de l'émotion qui domine, si elle en a, sinon celles du visage — de la forme de
    /// la voyelle, si elles l'ont ([`famille`]) ; d'une famille à l'autre en fondu, celle de la voyelle qui suit dès
    /// que les lèvres se joignent ; les rondes, sur leurs lèvres closes et arrondies, tant qu'elle parle.
    pub(super) fn avancer(&mut self, m: &Moment, v: &Visage, emotions: &Emotions, pose: &mut Pose) {
        let dominante = emotions.dominante();
        let retient = dominante.is_some_and(|e| !matches!(e, Emotion::Joie | Emotion::Amusee));
        let calme = self.calme.avancer(m, retient);
        if let Some(c) = &v.calme {
            pose.peindre(Strate::Dessous, c, calme * (1.0 - emotions.couverture()));
        }
        let a = if m.parle() { crate::voix::bouche_a_venir(AVANCE_DE_LA_BOUCHE) } else { None };
        // Tue, elle oublie la dernière voyelle : la phrase suivante ne commence pas avec sa forme.
        if !m.parle() {
            self.forme = None;
        }
        let levres = a.map(|a| a.forme).filter(|f| matches!(f, Forme::Fermee | Forme::Dents));
        let voyelle = if levres.is_some() {
            crate::voix::forme_a_venir(AVANCE_DE_LA_BOUCHE + ANTICIPATION)
        } else {
            a.map(|a| a.forme)
        };
        if let Some(f) = voyelle.filter(|f| !matches!(f, Forme::Fermee | Forme::Dents)) {
            self.forme = Some(f);
        }
        let genre = match self.forme {
            Some(Forme::Ronde) => Genre::Ronde,
            Some(Forme::Etiree) => Genre::Etiree,
            _ => Genre::Ouverte,
        };
        let famille = famille(genre, dominante, |f| !v.bouches(f).is_empty());
        let (machoire, niveau) = a.map_or((0.0, 0.0), |a| (a.machoire, a.niveau));
        let vitesse = |cible: f32, r: &Ressort| if cible > r.x { 38.0 } else { 26.0 };
        self.machoire.vers(machoire, vitesse(machoire, &self.machoire), m.dt);
        self.voix.vers(niveau, vitesse(niveau, &self.voix), m.dt);
        for (r, forme) in [(&mut self.levres_closes, Forme::Fermee), (&mut self.dents, Forme::Dents)] {
            let jointes = levres == Some(forme);
            r.vers(if jointes { 1.0 } else { 0.0 }, if jointes { 50.0 } else { 45.0 }, m.dt);
        }
        let (poids, reste) = self.passer_a(famille, m.dt);
        let o = self.machoire.x;
        let (c, d) = (self.levres_closes.x, self.dents.x);
        // Les lèvres jointes cachent la bouche ouverte : elle s'efface à mesure.
        let ouverte = (1.0 - c - d).max(0.0);
        // Une bouche ronde se pose sur les lèvres closes et arrondies de sa famille, tant qu'Eve parle : entre deux
        // syllabes, les lèvres restent en avant au lieu de revenir au visage calme, dont le coin de la bouche, bien
        // plus loin, se voyait en fantôme à chaque syllabe (le rendu ne le fait pas glisser jusque-là). Celui de la
        // famille d'avant s'efface à mesure que la nouvelle vient, sauf si elle a le sien : ses bouches ne le couvrent
        // pas (les lèvres arrondies revenaient au visage calme, ou la joie à ses dents, d'une image à la suivante).
        let fond = self.fond.vers(if m.parle() { 1.0 } else { 0.0 }, W_FOND, m.dt).clamp(0.0, 1.0);
        let fond_de = |f: Famille| v.levres(f, Forme::Fermee).filter(|_| f.genre == Genre::Ronde);
        let (fond_d_avant, fond_d_ici) = (fond_de(self.avant), fond_de(self.famille));
        if reste > 0.0 {
            if let Some(l) = fond_d_avant {
                pose.peindre(Strate::Bouche, l, fond * if fond_d_ici.is_some() { reste } else { 1.0 - poids });
            }
            pose.serie(Strate::Bouche, v.bouches(self.avant), o, ouverte * reste);
        }
        if let Some(l) = fond_d_ici {
            pose.peindre(Strate::Bouche, l, fond * poids);
        }
        pose.serie(Strate::Bouche, v.bouches(self.famille), o, poids * ouverte);
        // Les lèvres jointes passent d'une famille à l'autre en fondu, comme les bouches ; celles d'une famille qui
        // n'en a pas s'effacent.
        for (forme, part) in [(Forme::Fermee, c), (Forme::Dents, d)] {
            let (avant, ici) = (v.levres(self.avant, forme), v.levres(self.famille, forme));
            if reste > 0.0
                && let Some(l) = avant
            {
                pose.peindre(Strate::Bouche, l, part * if ici.is_some() { reste } else { 1.0 - poids });
            }
            if let Some(l) = ici {
                pose.peindre(Strate::Bouche, l, part * poids);
            }
        }
    }

    /// Passe à la famille de bouches `famille`, `dt` secondes plus tard, et rend son poids (de 0 à 1) et ce qui reste
    /// de celle d'avant (de 1 à 0) : elle se peint dessous, entière, le temps du fondu, puis s'efface sous la nouvelle
    /// pendant sa traîne ([`TRAINE_DU_PASSAGE`]). Une famille qui revient reprend le fondu où il en était ; une
    /// troisième attend qu'il finisse, traîne comprise (un dixième et demi de seconde au plus) : celle qui passait ne
    /// disparaît pas d'un coup.
    fn passer_a(&mut self, famille: Famille, dt: f32) -> (f32, f32) {
        if famille != self.famille {
            if famille == self.avant {
                (self.famille, self.avant, self.passage) = (famille, self.famille, (1.0 - self.passage).max(0.0));
            } else if self.passage >= FIN_DU_PASSAGE || self.avant == self.famille {
                (self.avant, self.famille, self.passage) = (self.famille, famille, 0.0);
            }
        }
        self.passage = (self.passage + dt / DUREE_DU_PASSAGE).min(FIN_DU_PASSAGE);
        if self.avant == self.famille {
            return (1.0, 0.0);
        }
        (lisse(self.passage), 1.0 - lisse((self.passage - 1.0) / (FIN_DU_PASSAGE - 1.0)))
    }

    /// La voix qui ouvre la bouche (de 0 à 1, sur son ressort, comme la mâchoire) : la tête et les sourcils la suivent,
    /// sur une syllabe appuyée.
    pub(super) fn ouverture(&self) -> f32 {
        self.voix.x
    }

    /// La bouche au repos : fermée, sa famille posée, le sourire de la photo revenu.
    pub(super) fn posee(&self) -> bool {
        let jointes = self.levres_closes.x.max(self.dents.x).max(self.fond.x);
        let passee = self.passage >= FIN_DU_PASSAGE;
        passee && self.machoire.x < 0.005 && self.voix.x < 0.005 && jointes < 0.005 && self.calme.pose()
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn une_famille_de_bouches_passe_a_l_autre_sans_a_coup() {
        let mut b = Bouche::default();
        let (ronde, etiree) =
            (Famille { genre: Genre::Ronde, emotion: None }, Famille { genre: Genre::Etiree, emotion: None });
        // D'ouverte à ronde : un fondu d'un dixième de seconde, lent au départ et à l'arrivée, l'ouverte dessous.
        assert_eq!(b.passer_a(ronde, 0.0), (0.0, 1.0));
        assert!((b.passer_a(ronde, 0.05).0 - 0.5).abs() < 1e-6);
        // Une troisième, étirée, attend : la ronde finit son fondu, puis l'ouverte s'efface dessous en un vingtième de
        // seconde, sans disparaître d'un coup…
        let (poids, reste) = b.passer_a(etiree, 0.05);
        assert!((poids - 1.0).abs() < 1e-5 && reste > 0.99 && b.famille == ronde, "{poids} {reste}");
        let (_, reste) = b.passer_a(etiree, 0.025);
        assert!((reste - 0.5).abs() < 1e-3 && b.famille == ronde, "{reste}");
        assert_eq!(b.passer_a(etiree, 0.05), (1.0, 0.0));
        // … puis l'étirée vient, en fondu.
        assert_eq!(b.passer_a(etiree, 0.0), (0.0, 1.0));
        assert!(b.famille == etiree && b.avant == ronde);
        // La ronde qui revient reprend le fondu où il en était : à peine partie, elle est encore là tout entière ; la
        // bouche est posée, sa traîne finie.
        assert!((b.passer_a(ronde, 0.01).0 - 1.0).abs() < 1e-6);
        assert!(b.famille == ronde && !b.posee());
        b.passer_a(ronde, 0.05);
        assert!(b.posee());
    }

    #[test]
    fn une_syllabe_garde_sa_forme_dans_l_emotion() {
        use Genre::{Etiree, Ouverte, Ronde};
        let f = |genre, emotion| Famille { genre, emotion };
        let (joie, triste) = (Some(Emotion::Joie), Some(Emotion::Triste));
        // Le visage a ses trois genres ; la joie, ses bouches et ses rondes, pas d'étirées ; la tristesse, ses bouches
        // seulement ; la surprise, aucune.
        let existe = |x: Famille| match x.emotion {
            None => true,
            Some(Emotion::Joie) => x.genre != Etiree,
            Some(Emotion::Triste) => x.genre == Ouverte,
            Some(_) => false,
        };
        // Sans émotion, la forme de la voyelle ; dans la joie, un o reste rond, un i s'ouvre dans la joie ; dans la
        // tristesse, tout s'ouvre dans la tristesse ; une émotion sans bouches laisse celles du visage.
        assert!(famille(Ronde, None, existe) == f(Ronde, None) && famille(Etiree, None, existe) == f(Etiree, None));
        assert!(famille(Ronde, joie, existe) == f(Ronde, joie) && famille(Etiree, joie, existe) == f(Ouverte, joie));
        assert!(famille(Ronde, triste, existe) == f(Ouverte, triste));
        assert!(famille(Ronde, Some(Emotion::Surprise), existe) == f(Ronde, None));
        // Un visage sans rondes : un o s'ouvre.
        assert!(famille(Ronde, None, |x: Famille| x.genre == Ouverte) == f(Ouverte, None));
    }

    #[test]
    fn le_visage_calme_vient_en_conversation_et_reste_un_moment() {
        // Le visage calme avancé de `secondes`, soixante images par seconde, Eve à son `activite`, une émotion qui ne
        // sourit pas le retenant ou non : sa part.
        let avancer = |c: &mut Calme, t: &mut f64, secondes: f64, activite: Activite, retient: bool| {
            let mut part = 0.0;
            for _ in 0..(secondes * 60.0).round() as usize {
                *t += 1.0 / 60.0;
                let m = Moment { temps: *t, dt: 1.0 / 60.0, activite, vivant: true, ..Default::default() };
                part = c.avancer(&m, retient);
            }
            part
        };
        let (mut c, mut t) = (Calme::default(), 100.0);
        // Au repos, le sourire de la photo ; elle écoute : le visage calme vient, presque entier en une demi-seconde,
        // et reste pendant qu'elle réfléchit et qu'elle parle.
        assert_eq!(avancer(&mut c, &mut t, 1.0, Activite::Repos, false), 0.0);
        assert!(c.pose());
        assert!(avancer(&mut c, &mut t, 0.5, Activite::Ecoute, false) > 0.9);
        assert!(avancer(&mut c, &mut t, 2.0, Activite::Reflechit, false) > 0.99);
        assert!(avancer(&mut c, &mut t, 3.0, Activite::Parle, false) > 0.99);
        // Tue : il reste sa tenue (le sourire ne revient pas entre deux phrases), puis s'en va, doucement.
        assert!(avancer(&mut c, &mut t, 1.0, Activite::Repos, false) > 0.99 && !c.pose());
        let partant = avancer(&mut c, &mut t, 1.0, Activite::Repos, false);
        assert!((0.1..0.6).contains(&partant), "{partant}");
        assert!(avancer(&mut c, &mut t, 2.5, Activite::Repos, false) < 0.01);
        assert!(c.pose());
        // Elle reparle avant la fin de sa tenue : il ne part pas.
        avancer(&mut c, &mut t, 1.0, Activite::Parle, false);
        avancer(&mut c, &mut t, 0.9, Activite::Repos, false);
        assert!(avancer(&mut c, &mut t, 1.0, Activite::Parle, false) > 0.99);
        // Une peine qu'elle garde, tue, dix secondes : il reste dessous ; elle s'efface, il reste encore sa tenue, puis
        // le sourire de la photo revient.
        assert!(avancer(&mut c, &mut t, 10.0, Activite::Repos, true) > 0.99);
        assert!(avancer(&mut c, &mut t, 1.0, Activite::Repos, false) > 0.99);
        assert!(avancer(&mut c, &mut t, 3.0, Activite::Repos, false) < 0.01);
    }
}
