//! Les gestes du visage d'Eve : sa tête et ses sourcils, qui accompagnent ce qu'elle dit et ce qu'elle fait. Quand elle
//! parle, ils suivent sa phrase ([`Repere`]) : un petit hochement sur la dernière syllabe d'un groupe de mots (celle
//! que le français appuie), un hochement et les sourcils sur une exclamation, la tête penchée et les sourcils levés au
//! bout d'une question — tenus un instant, elle attend la réponse —, la tête qui se pose à la fin d'une phrase, et une
//! autre posture quand elle reprend. Quand elle écoute, la tête penchée vers la personne, et un petit hochement quand
//! celle-ci s'arrête un instant ; quand elle réfléchit, la tête un peu relevée, immobile ; au repos, une posture gardée
//! longtemps, puis une autre, et les sourcils de loin en loin. Son émotion leur donne leur élan : plus vifs dans la
//! joie, plus retenus dans la tristesse. Rien ne flotte : chaque mouvement va quelque part et s'y pose.

use std::sync::Arc;
use std::time::Duration;

use eve::parole::Emotion;

use crate::voix::{Marque, Repere};

use super::emotions::Emotions;
use super::mimique::{Hasard, Moment, Ressort};
use super::{Activite, Pose, Strate, Tete, Visage};

/// Les pulsations des ressorts de la tête : une posture rejointe en trois quarts de seconde ; l'inflexion d'une phrase
/// (la tête penchée d'une question, posée d'une fin de phrase) en un peu plus d'une demi-seconde.
const W_TETE: f32 = 5.0;
const W_INFLEXION: f32 = 7.0;
/// Un hochement, en secondes : la tête descend, puis remonte, un peu plus lentement ([`creux`]).
const DESCENTE: f64 = 0.16;
const REMONTEE: f64 = 0.26;
/// L'avance des gestes sur la voix : un hochement part avant la syllabe qu'il souligne, et la touche au plus bas.
const AVANCE: Duration = Duration::from_millis(160);
/// Une question : la tête se penche et les sourcils se lèvent sur ses dernières syllabes — dès une demi-seconde avant
/// la dernière —, et le restent une seconde après elle, tant que la voix ne reprend pas.
const AVANT_LA_QUESTION: f32 = 0.5;
const APRES_LA_QUESTION: f64 = 1.0;
/// Les hochements, en part du rayon du disque (la tête descend d'autant au plus bas) : sur une exclamation, sur la
/// dernière syllabe d'un groupe, sur une syllabe appuyée au milieu d'une phrase, en écoutant.
const HOCHEMENT_D_EXCLAMATION: f32 = 0.022;
const HOCHEMENT_DE_GROUPE: f32 = 0.014;
const HOCHEMENT_D_ACCENT: f32 = 0.008;
const HOCHEMENT_D_ECOUTE: f32 = 0.011;
/// Une pause qui laisse la tête prendre une autre posture à la reprise, en secondes ; et le moins de temps entre deux
/// postures, en parlant.
const LONGUE_PAUSE: f32 = 0.3;
const ENTRE_DEUX_POSTURES: f64 = 1.5;
/// Une pause de la personne qu'Eve écoute, en secondes : sa voix tombe un quart de seconde (en plus du dixième que met
/// le micro à le dire) ; on hoche ainsi entre deux et cinq dixièmes de seconde après que l'autre s'est arrêté.
const PAUSE_DE_LA_PERSONNE: f64 = 0.25;

/// Trois ressorts, un par mouvement de la tête.
#[derive(Default)]
struct Ressorts {
    roulis: Ressort,
    x: Ressort,
    y: Ressort,
}

impl Ressorts {
    fn vers(&mut self, cible: Tete, w: f32, dt: f32) -> Tete {
        Tete {
            roulis: self.roulis.vers(cible.roulis, w, dt),
            x: self.x.vers(cible.x, w, dt),
            y: self.y.vers(cible.y, w, dt),
        }
    }
}

fn somme(a: Tete, b: Tete) -> Tete {
    Tete { roulis: a.roulis + b.roulis, x: a.x + b.x, y: a.y + b.y }
}

/// L'élan que l'émotion qui domine donne aux gestes : leur ampleur (les postures, les hochements, les sourcils) et
/// leur vitesse (la tête qui va d'une posture à l'autre) — plus vifs dans la joie, l'amusement et la surprise, plus
/// retenus et plus lents dans l'empathie et la tristesse (on bouge peu quand on console ou qu'on a de la peine), un
/// peu plus sobres quand elle est sérieuse.
fn elan(emotion: Option<Emotion>) -> (f32, f32) {
    match emotion {
        Some(Emotion::Joie | Emotion::Surprise) => (1.15, 1.1),
        Some(Emotion::Amusee) => (1.25, 1.1),
        Some(Emotion::Empathie) => (0.75, 0.8),
        Some(Emotion::Triste) => (0.6, 0.7),
        Some(Emotion::Serieuse) => (0.85, 1.0),
        Some(Emotion::Reflexion | Emotion::Neutre) | None => (1.0, 1.0),
    }
}

/// Où en est un hochement parti depuis `t` secondes : de 0 (la tête à sa place) à 1 (au plus bas). Il descend en
/// [`DESCENTE`], remonte en [`REMONTEE`], un demi-cosinus chaque fois : il part doucement, ne s'arrête pas net au plus
/// bas et se pose sans à-coup — un mouvement de muscle, pas un choc.
fn creux(t: f64) -> f32 {
    let demi = |x: f64| (0.5 - 0.5 * (std::f64::consts::PI * x).cos()) as f32;
    match t {
        t if t <= 0.0 || t >= DESCENTE + REMONTEE => 0.0,
        t if t < DESCENTE => demi(t / DESCENTE),
        t => demi(1.0 - (t - DESCENTE) / REMONTEE),
    }
}

/// Les gestes, d'une image à l'autre. Les sourcils : leur hauteur (de 0 à 1), jusqu'à quand et à quelle hauteur ils
/// restent levés, quand ils se sont levés la dernière fois, quand ils se lèveront au repos. La tête : sa posture et
/// quand elle en changera, la dernière fois qu'elle en a changé ; l'inflexion de la phrase et jusqu'à quand ; les
/// hochements en cours (depuis quand, de combien), le dernier, le second d'un « oui, oui » s'il en vient un, et quand.
/// En écoutant, la voix de la personne : la plus forte entendue, depuis quand elle parle, jusqu'à quand. Ce que fait
/// Eve, à l'image d'avant, et depuis quand ; l'élan de ses gestes ([`elan`]) ; la phrase qu'elle dit — ses repères,
/// jusqu'où ils sont lus —, en pause, une posture à prendre à la reprise ; l'ouverture de la bouche à l'image d'avant
/// (une syllabe qui monte).
#[derive(Default)]
pub(super) struct Gestes {
    sourcils: Ressort,
    leves_jusqua: f64,
    hauteur: f32,
    derniers_sourcils: f64,
    prochains_sourcils: f64,
    tete: Ressorts,
    posture: Tete,
    prochaine_posture: f64,
    derniere_posture: f64,
    inflechie: Ressorts,
    inflexion: Tete,
    inflexion_jusqua: f64,
    hochements: Vec<(f64, f32)>,
    dernier_hochement: f64,
    prochain_hochement: f64,
    second_hochement: bool,
    voix_max: f32,
    voix_depuis: f64,
    voix_jusqua: f64,
    activite: Activite,
    depuis: f64,
    ampleur: f32,
    vitesse: f32,
    phrase: Option<Arc<[Repere]>>,
    lu: f32,
    en_pause: bool,
    posture_a_la_reprise: bool,
    ouverture_avant: f32,
}

impl Gestes {
    /// Les gestes à cette image, la bouche à son `ouverture`, les `emotions` du visage : les sourcils (sous l'émotion :
    /// une émotion qui a les siens les couvre) et la tête (et le penchant de l'émotion). Sans les animations de
    /// Windows, rien ne bouge.
    pub(super) fn avancer(
        &mut self,
        m: &Moment,
        v: &Visage,
        ouverture: f32,
        emotions: &Emotions,
        h: &mut Hasard,
        pose: &mut Pose,
    ) {
        let [.., sourcils] = &v.calques;
        let phrase = if m.vivant && m.parle() { crate::voix::reperes_a_venir(AVANCE) } else { None };
        let (tete, leves) = self.bouger(m, ouverture, emotions.penchant(), elan(emotions.dominante()), phrase, h);
        pose.serie(Strate::Dessous, sourcils, leves, 1.0);
        if m.vivant {
            pose.tete = tete;
        }
    }

    /// Les gestes à cette image : la tête — sa posture, et le `penchant` de l'émotion ; l'inflexion de la phrase ; le
    /// hochement — et la hauteur des sourcils. Eve à son activité, la bouche à son `ouverture`, l'`elan` que l'émotion
    /// donne à ses gestes ([`elan`]) ; `phrase` : la phrase qu'elle dit, où elle en est et ses repères.
    fn bouger(
        &mut self,
        m: &Moment,
        ouverture: f32,
        penchant: Tete,
        elan: (f32, f32),
        phrase: Option<(f32, Arc<[Repere]>)>,
        h: &mut Hasard,
    ) -> (Tete, f32) {
        let monte = ouverture > 0.7 && self.ouverture_avant <= 0.7;
        (self.ouverture_avant, (self.ampleur, self.vitesse)) = (ouverture, elan);
        if m.vivant {
            if m.activite != self.activite {
                self.changer_d_activite(m, h);
            }
            self.suivre_la_phrase(m, phrase, monte, h);
            self.garder_la_posture(m, monte, h);
            self.ecouter(m, h);
            self.sourciller_au_repos(m, h);
        }
        // Les sourcils montent en un quart de seconde, redescendent en moins d'une demi.
        let voulus = if m.temps < self.leves_jusqua { self.hauteur } else { 0.0 };
        let leves = self.sourcils.vers(voulus, if voulus > self.sourcils.x { 16.0 } else { 9.0 }, m.dt);
        let inflexion = if m.temps < self.inflexion_jusqua { self.inflexion } else { Tete::default() };
        let tete = somme(
            self.tete.vers(somme(self.posture, penchant), W_TETE * self.vitesse, m.dt),
            self.inflechie.vers(inflexion, W_INFLEXION * self.vitesse, m.dt),
        );
        self.hochements.retain(|&(depuis, _)| m.temps - depuis < DESCENTE + REMONTEE);
        (Tete { y: tete.y + self.hochement(m.temps), ..tete }, leves)
    }

    /// Eve change d'activité : quand elle écoute, elle se tourne vers la personne — la tête penchée du côté où elle
    /// l'était, de 2,6 à 3,4 degrés, un peu de ce côté et un peu plus bas, comme on se penche pour mieux entendre — et
    /// lève les sourcils un instant (« je t'écoute ») ; quand elle réfléchit, elle se redresse et relève un peu la
    /// tête, sans plus bouger ; quand elle se met à parler, une posture sur sa première syllabe ; au repos, une posture
    /// bientôt.
    fn changer_d_activite(&mut self, m: &Moment, h: &mut Hasard) {
        (self.activite, self.depuis) = (m.activite, m.temps);
        match m.activite {
            Activite::Ecoute => {
                let cote = if self.posture.roulis == 0.0 { h.signe().signum() } else { self.posture.roulis.signum() };
                let roulis = cote * (0.045 + 0.015 * h.tirer() as f32);
                self.posture = Tete { roulis, x: 0.25 * roulis, y: 0.008 };
                self.prochaine_posture = f64::INFINITY;
                self.lever(0.35, 0.6, m.temps);
                (self.voix_max, self.voix_depuis, self.voix_jusqua) = (0.0, m.temps, f64::NEG_INFINITY);
                self.second_hochement = false;
            }
            Activite::Reflechit => {
                self.posture = Tete { roulis: 0.2 * self.posture.roulis, x: 0.0, y: -0.012 };
                self.prochaine_posture = f64::INFINITY;
            }
            Activite::Parle => self.prochaine_posture = m.temps,
            Activite::Repos => self.prochaine_posture = m.temps + 0.8 + 1.5 * h.tirer(),
        }
    }

    /// La posture qu'elle garde : au repos, trois secondes et demie à huit et demie, puis une autre, à moitié moins
    /// marquée qu'en parlant ; en parlant, une autre sur la première syllabe qui suit une demande (le début d'une
    /// phrase, une reprise après une pause), au plus tard toutes les quatre secondes et demie ; chacune de l'ampleur
    /// que l'émotion donne aux gestes. En écoutant et en réfléchissant, celle qu'elle a prise.
    fn garder_la_posture(&mut self, m: &Moment, monte: bool, h: &mut Hasard) {
        if m.temps < self.prochaine_posture {
            return;
        }
        match m.activite {
            Activite::Repos => {
                self.prendre_une_posture(0.5 * self.ampleur, m.temps, h);
                self.prochaine_posture = m.temps + 3.5 + 5.0 * h.tirer();
            }
            Activite::Parle if monte => {
                self.prendre_une_posture(self.ampleur, m.temps, h);
                self.prochaine_posture = m.temps + 4.5;
            }
            _ => {}
        }
    }

    /// Une autre posture, d'ampleur `ample` : la tête penchée du côté où elle l'est le plus souvent, de l'autre de
    /// temps en temps — jamais pendant qu'une question se défait (les deux mouvements s'ajouteraient : un balancement
    /// de plus de cinq degrés) —, assez pour que cela se voie (de 40 % à 100 % de 1,6 degré), un peu déplacée.
    fn prendre_une_posture(&mut self, ample: f32, temps: f64, h: &mut Hasard) {
        let penchee = self.tete.roulis.x + self.inflechie.roulis.x;
        let avant = if penchee >= 0.0 { 1.0 } else { -1.0 };
        let changer = self.inflechie.roulis.x.abs() < 0.01 && h.tirer() < 0.4;
        let roulis = if changer { -avant } else { avant } * ample * 0.028 * (0.4 + 0.6 * h.tirer() as f32);
        self.posture = Tete { roulis, x: 0.3 * roulis + ample * 0.006 * h.signe(), y: ample * 0.005 * h.signe() };
        self.derniere_posture = temps;
    }

    /// Une autre posture, demandée (en parlant) : elle viendra sur la prochaine syllabe, si la dernière n'est pas trop
    /// récente.
    fn demander_une_posture(&mut self, temps: f64) {
        if temps - self.derniere_posture >= ENTRE_DEUX_POSTURES {
            self.prochaine_posture = self.prochaine_posture.min(temps);
        }
    }

    /// La phrase qu'elle dit (où elle en est, ses repères) : chaque repère qu'elle atteint ([`Gestes::repere`]) ; une
    /// nouvelle phrase demande une autre posture et défait ce que la précédente avait infléchi ; au milieu d'une
    /// phrase, sur une syllabe qui monte, de temps en temps, un hochement à peine — pas juste avant un repère, qui a le
    /// sien.
    fn suivre_la_phrase(&mut self, m: &Moment, phrase: Option<(f32, Arc<[Repere]>)>, monte: bool, h: &mut Hasard) {
        let Some((t, reperes)) = phrase else {
            (self.phrase, self.en_pause) = (None, false);
            return;
        };
        if !self.phrase.as_ref().is_some_and(|p| Arc::ptr_eq(p, &reperes)) {
            (self.phrase, self.lu) = (Some(reperes.clone()), t - m.dt);
            (self.en_pause, self.posture_a_la_reprise) = (false, false);
            self.reprendre(m.temps);
        }
        let avant = std::mem::replace(&mut self.lu, t);
        for (k, r) in reperes.iter().enumerate() {
            let quand = r.a - if r.marque == Marque::Question { AVANT_LA_QUESTION } else { 0.0 };
            if avant < quand && quand <= t {
                self.repere(m, r, &reperes[k + 1..], h);
            }
        }
        let fort = |r: &Repere| matches!(r.marque, Marque::Groupe | Marque::Exclamation | Marque::Fin);
        let repere_proche = reperes.iter().any(|r| fort(r) && r.a > t && r.a - t < 0.6);
        if monte && !self.en_pause && !repere_proche && m.temps - self.dernier_hochement > 1.5 && h.tirer() < 0.3 {
            self.hocher(HOCHEMENT_D_ACCENT, m.temps);
        }
    }

    /// Un repère de la phrase qu'elle atteint ([`Marque`]) ; `suite` : ceux qui le suivent.
    fn repere(&mut self, m: &Moment, r: &Repere, suite: &[Repere], h: &mut Hasard) {
        match r.marque {
            Marque::Groupe => {
                // La dernière syllabe d'un groupe : un petit hochement ; parfois, la voix montant, les sourcils.
                if m.temps - self.dernier_hochement > 0.35 {
                    self.hocher(HOCHEMENT_DE_GROUPE, m.temps);
                }
                if m.temps - self.derniers_sourcils > 2.0 && h.tirer() < 0.35 {
                    self.lever(0.4, 0.35, m.temps);
                }
            }
            Marque::Exclamation => {
                self.hocher(HOCHEMENT_D_EXCLAMATION, m.temps);
                self.lever(0.6, 0.45, m.temps);
            }
            Marque::Question => {
                // Ses dernières syllabes : la tête penchée du côté où elle l'est déjà, un peu relevée, et les sourcils
                // levés, jusqu'à une seconde après elles.
                let cote = if self.posture.roulis >= 0.0 { 1.0 } else { -1.0 };
                let duree = f64::from(AVANT_LA_QUESTION) + APRES_LA_QUESTION;
                self.inflechir(Tete { roulis: cote * 0.03, x: cote * 0.006, y: -0.006 }, m.temps + duree);
                self.lever(0.7, duree, m.temps);
            }
            Marque::Fin => {
                // La voix descend : la tête se pose un peu plus bas, jusqu'à ce qu'elle reprenne.
                self.inflechir(Tete { y: 0.008, ..Tete::default() }, m.temps + 1.2);
            }
            Marque::Pause => {
                // Une longue pause (entre deux phrases, après un deux-points) : une autre posture à la reprise.
                self.en_pause = true;
                let reprise = suite.iter().find(|s| s.marque == Marque::Reprise);
                self.posture_a_la_reprise = reprise.is_some_and(|s| s.a - r.a >= LONGUE_PAUSE);
            }
            Marque::Reprise => {
                self.en_pause = false;
                if std::mem::take(&mut self.posture_a_la_reprise) {
                    self.reprendre(m.temps);
                }
            }
        }
    }

    /// La voix reprend (une nouvelle phrase, ou après une longue pause) : ce que la phrase d'avant avait infléchi (la
    /// tête posée, la question et ses sourcils) se défait, et une autre posture vient avec la première syllabe.
    fn reprendre(&mut self, temps: f64) {
        self.inflexion_jusqua = self.inflexion_jusqua.min(temps + 0.15);
        self.leves_jusqua = self.leves_jusqua.min(temps + 0.15);
        self.demander_une_posture(temps);
    }

    /// En écoutant, un petit hochement quand la personne s'arrête un instant après avoir parlé un peu (sa voix, que le
    /// micro entend : forte, au-delà de 40 % de la plus forte de cette écoute) — comme on le fait pour dire « je te
    /// suis » —, le plus souvent ; de loin en loin si elle parle longtemps sans s'arrêter. Dans le silence, rien : elle
    /// attend.
    fn ecouter(&mut self, m: &Moment, h: &mut Hasard) {
        if m.activite != Activite::Ecoute {
            return;
        }
        if self.second_hochement && m.temps >= self.prochain_hochement {
            self.second_hochement = false;
            self.hocher(HOCHEMENT_D_ECOUTE, m.temps);
        }
        self.voix_max = self.voix_max.max(m.entendu);
        if self.voix_max > 0.1 && m.entendu > 0.4 * self.voix_max {
            if m.temps - self.voix_jusqua > PAUSE_DE_LA_PERSONNE {
                self.voix_depuis = m.temps;
            }
            self.voix_jusqua = m.temps;
            if m.temps - self.dernier_hochement.max(self.voix_depuis) > 5.0 {
                self.acquiescer(m, h);
            }
        } else if m.temps - self.voix_jusqua >= PAUSE_DE_LA_PERSONNE && self.voix_jusqua - self.voix_depuis >= 0.8 {
            // Elle s'arrête après avoir parlé : cette pause est vue.
            self.voix_depuis = self.voix_jusqua;
            if h.tirer() < 0.75 {
                self.acquiescer(m, h);
            }
        }
    }

    /// Un hochement d'écoute (pas deux trop près l'un de l'autre), et parfois un second, comme un « oui, oui ».
    fn acquiescer(&mut self, m: &Moment, h: &mut Hasard) {
        if m.temps - self.dernier_hochement < 1.2 {
            return;
        }
        self.hocher(HOCHEMENT_D_ECOUTE, m.temps);
        if h.tirer() < 0.25 {
            (self.second_hochement, self.prochain_hochement) = (true, m.temps + 0.42);
        }
    }

    /// Au repos, les sourcils levés de loin en loin (toutes les quinze à quarante secondes) — pas tout de suite après
    /// avoir parlé ou écouté, ni juste après un autre levé : ils remonteraient à peine retombés.
    fn sourciller_au_repos(&mut self, m: &Moment, h: &mut Hasard) {
        if self.prochains_sourcils == 0.0 {
            self.prochains_sourcils = m.temps + 12.0;
        }
        let calme = m.temps - self.depuis > 2.0 && m.temps - self.derniers_sourcils > 4.0;
        if m.activite == Activite::Repos && calme && m.temps >= self.prochains_sourcils {
            self.lever(0.6, 0.6, m.temps);
            self.prochains_sourcils = m.temps + 15.0 + 25.0 * h.tirer();
        }
    }

    /// Les sourcils levés à `hauteur` (de 0 à 1 : de la première pose à la dernière ; à l'ampleur que l'émotion donne
    /// aux gestes) pendant `duree` secondes — plus haut ou plus longtemps s'ils le sont déjà.
    fn lever(&mut self, hauteur: f32, duree: f64, temps: f64) {
        let hauteur = (hauteur * self.ampleur).min(1.0);
        if temps < self.leves_jusqua {
            (self.hauteur, self.leves_jusqua) = (self.hauteur.max(hauteur), self.leves_jusqua.max(temps + duree));
        } else {
            (self.hauteur, self.leves_jusqua) = (hauteur, temps + duree);
        }
        self.derniers_sourcils = temps;
    }

    /// La tête infléchie de `inflexion` (en plus de sa posture) jusqu'à `jusqua`.
    fn inflechir(&mut self, inflexion: Tete, jusqua: f64) {
        (self.inflexion, self.inflexion_jusqua) = (inflexion, jusqua);
    }

    /// Un hochement : la tête descend de `ampleur` (une part du rayon, à l'ampleur que l'émotion donne aux gestes),
    /// puis remonte ([`creux`]).
    fn hocher(&mut self, ampleur: f32, temps: f64) {
        self.hochements.push((temps, ampleur * self.ampleur));
        self.dernier_hochement = temps;
    }

    /// Ce dont les hochements en cours font descendre la tête à l'instant `temps` (une part du rayon).
    fn hochement(&self, temps: f64) -> f32 {
        self.hochements.iter().map(|&(depuis, ampleur)| ampleur * creux(temps - depuis)).sum()
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    /// Les gestes de `debut` pendant `duree` secondes, à 60 images par seconde, Eve à son `activite` ; `phrase` : celle
    /// qu'elle dit depuis un instant, et ses repères. La tête et les sourcils à chaque image.
    fn filmer(
        g: &mut Gestes,
        h: &mut Hasard,
        (debut, duree): (f64, f64),
        activite: Activite,
        phrase: Option<(f64, &Arc<[Repere]>)>,
    ) -> Vec<(f64, Tete, f32)> {
        let dt = 1.0 / 60.0;
        (0..(duree / dt).round() as usize)
            .map(|k| {
                let temps = debut + k as f64 * dt;
                let m = Moment { temps, dt: dt as f32, activite, vivant: true, ..Default::default() };
                let p = phrase.map(|(depuis, r)| ((temps - depuis) as f32 + AVANCE.as_secs_f32(), r.clone()));
                // La bouche s'ouvre et se ferme cinq fois par seconde en parlant : des syllabes qui montent.
                let ouverture = if activite == Activite::Parle && (temps * 5.0).fract() < 0.5 { 0.9 } else { 0.1 };
                let (tete, sourcils) = g.bouger(&m, ouverture, Tete::default(), (1.0, 1.0), p, h);
                (temps, tete, sourcils)
            })
            .collect()
    }

    /// Rien ne saute d'une image à l'autre : la tête et les sourcils vont tous en douceur.
    fn sans_saut(images: &[(f64, Tete, f32)]) {
        for w in images.windows(2) {
            let ((_, a, sa), (t, b, sb)) = (w[0], w[1]);
            let saut = (a.roulis - b.roulis).abs().max((a.x - b.x).abs()).max((a.y - b.y).abs());
            assert!(saut < 0.01 && (sa - sb).abs() < 0.08, "à {t:.2} s : {saut} {sa} {sb}");
        }
    }

    fn repere(a: f32, marque: Marque) -> Repere {
        Repere { a, marque }
    }

    /// Une image, pour les messages : son instant, la tête, les sourcils.
    fn ecrire((t, tete, sourcils): (f64, Tete, f32)) -> String {
        format!("à {t:.2} s : roulis {:.4}, x {:.4}, y {:.4} ; sourcils {sourcils:.3}", tete.roulis, tete.x, tete.y)
    }

    #[test]
    fn un_hochement_part_doucement_et_se_pose() {
        // À sa place au départ et à l'arrivée, au plus bas après la descente ; à la première image (un soixantième de
        // seconde), à peine parti — moins de 3 % du chemin, là où un ressort lancé d'un coup en faisait le tiers.
        assert_eq!((creux(0.0), creux(DESCENTE), creux(DESCENTE + REMONTEE)), (0.0, 1.0, 0.0));
        assert!(creux(1.0 / 60.0) < 0.03 && creux(DESCENTE + REMONTEE - 1.0 / 60.0) < 0.02);
        // Il descend, puis remonte, sans jamais repartir en arrière.
        let pas: Vec<f32> = (0..=42).map(|k| creux(k as f64 / 100.0)).collect();
        let bas = pas.iter().position(|&v| v == 1.0).unwrap();
        assert!(pas[..=bas].windows(2).all(|w| w[1] >= w[0]) && pas[bas..].windows(2).all(|w| w[1] <= w[0]));
    }

    #[test]
    fn une_question_penche_la_tete_et_leve_les_sourcils() {
        // « Tu viens ? », sa dernière syllabe à 1,2 s ; Eve parle depuis 10 s, puis se tait à 11,6 s.
        let (mut g, mut h) = (Gestes::default(), Hasard::default());
        filmer(&mut g, &mut h, (0.0, 10.0), Activite::Repos, None);
        let reperes: Arc<[Repere]> = [repere(1.2, Marque::Question)].into();
        let mut images = filmer(&mut g, &mut h, (10.0, 1.6), Activite::Parle, Some((10.0, &reperes)));
        images.extend(filmer(&mut g, &mut h, (11.6, 3.0), Activite::Repos, None));
        sans_saut(&images);
        let a = |t: f64| images.iter().find(|i| i.0 >= t).copied().unwrap();
        // Avant elle, rien ; sur sa dernière syllabe, les sourcils levés (déjà partis une demi-seconde avant), la tête
        // relevée de l'inflexion ; près d'une seconde après, toujours ; deux secondes après, retombés.
        assert!(a(10.5).2 < 0.05, "{}", ecrire(a(10.5)));
        assert!(a(11.2).2 > 0.55 && a(11.95).2 > 0.6, "{} {}", ecrire(a(11.2)), ecrire(a(11.95)));
        assert!(g.inflechie.y.x.abs() < 0.001 && a(13.6).2 < 0.05, "{}", ecrire(a(13.6)));
        let roulis = |t: f64| a(t).1.roulis - images[0].1.roulis;
        assert!(roulis(11.4).abs() > 0.015, "la tête penchée : {}", roulis(11.4));
    }

    #[test]
    fn les_groupes_hochent_et_l_exclamation_plus_fort() {
        // « Deux rendez-vous, une facture, et c'est fini ! » : deux fins de groupes, une exclamation.
        let (mut g, mut h) = (Gestes::default(), Hasard::default());
        let reperes: Arc<[Repere]> =
            [repere(0.8, Marque::Groupe), repere(1.8, Marque::Groupe), repere(3.0, Marque::Exclamation)].into();
        let mut plus_bas = Vec::new();
        let dt = 1.0 / 60.0;
        for k in 0..240 {
            let temps = 5.0 + k as f64 * dt;
            let m = Moment { temps, dt: dt as f32, activite: Activite::Parle, vivant: true, ..Default::default() };
            let p = Some(((temps - 5.0) as f32 + AVANCE.as_secs_f32(), reperes.clone()));
            g.bouger(&m, 0.1, Tete::default(), (1.0, 1.0), p, &mut h);
            plus_bas.push((temps - 5.0, g.hochement(temps)));
        }
        // Le plus bas de chaque hochement, près de sa syllabe (à un dixième de seconde près), et de son ampleur.
        for (a, ampleur) in [(0.8, HOCHEMENT_DE_GROUPE), (1.8, HOCHEMENT_DE_GROUPE), (3.0, HOCHEMENT_D_EXCLAMATION)] {
            let (quand, bas) = plus_bas
                .iter()
                .filter(|(t, _)| (t - a).abs() < 0.45)
                .fold((0.0, 0.0f32), |m, &(t, y)| if y > m.1 { (t, y) } else { m });
            assert!((quand - a).abs() < 0.1, "le plus bas à {quand} pour {a}");
            assert!((bas - ampleur).abs() < 0.15 * ampleur, "{bas} pour {ampleur}");
        }
    }

    #[test]
    fn la_tristesse_retient_les_gestes() {
        // La même fin de groupe, dite avec joie, sans émotion, avec tristesse : un hochement de plus en plus retenu.
        let plus_bas = |emotion| {
            let (mut g, mut h) = (Gestes::default(), Hasard::default());
            let reperes: Arc<[Repere]> = [repere(0.8, Marque::Groupe)].into();
            let dt = 1.0 / 60.0;
            (0..90)
                .map(|k| {
                    let temps = 5.0 + k as f64 * dt;
                    let activite = Activite::Parle;
                    let m = Moment { temps, dt: dt as f32, activite, vivant: true, ..Default::default() };
                    let p = Some(((temps - 5.0) as f32 + AVANCE.as_secs_f32(), reperes.clone()));
                    g.bouger(&m, 0.1, Tete::default(), elan(emotion), p, &mut h);
                    g.hochement(temps)
                })
                .fold(0.0f32, f32::max)
        };
        let (joie, neutre, triste) = (plus_bas(Some(Emotion::Joie)), plus_bas(None), plus_bas(Some(Emotion::Triste)));
        assert!((neutre - HOCHEMENT_DE_GROUPE).abs() < 0.001, "{neutre}");
        assert!(joie > 1.1 * neutre && triste < 0.65 * neutre, "{joie} {neutre} {triste}");
    }

    #[test]
    fn elle_ecoute_penchee_et_hoche_aux_pauses() {
        // La personne parle de 0,3 à 1,7 s, de 2,2 à 3,4 s et de 3,9 à 5 s (sa voix, cinq syllabes par seconde), puis
        // se tait.
        let pauses = [1.7, 3.4, 5.0];
        let voix = |t: f64| {
            let parle = [(0.3, 1.7), (2.2, 3.4), (3.9, 5.0)].iter().any(|&(a, b)| (a..b).contains(&t));
            if parle { 0.5 + 0.3 * (t * 5.0).fract() as f32 } else { 0.02 }
        };
        let (mut g, mut h) = (Gestes::default(), Hasard::default());
        filmer(&mut g, &mut h, (0.0, 5.0), Activite::Repos, None);
        let (mut images, mut hochements) = (Vec::new(), Vec::new());
        let dt = 1.0 / 60.0;
        for k in 0..480 {
            let (temps, avant) = (5.0 + k as f64 * dt, g.dernier_hochement);
            let m =
                Moment { temps, dt: dt as f32, activite: Activite::Ecoute, vivant: true, entendu: voix(temps - 5.0) };
            let (tete, sourcils) = g.bouger(&m, 0.0, Tete::default(), (1.0, 1.0), None, &mut h);
            if g.dernier_hochement != avant {
                hochements.push(temps - 5.0);
            }
            images.push((temps, tete, sourcils));
        }
        sans_saut(&images);
        // La tête penchée (plus de 2,5 degrés) et un peu plus bas, tenue ; les sourcils levés un instant, au début.
        let fin = images.last().unwrap().1;
        assert!(fin.roulis.abs() > 0.044 && fin.y > 0.0, "{}", ecrire(*images.last().unwrap()));
        assert!(images.iter().take(30).any(|i| i.2 > 0.15) && images.last().unwrap().2 < 0.01);
        // Ses hochements : chacun de deux à sept dixièmes de seconde après qu'elle s'est arrêtée (ou le second d'un
        // « oui, oui ») ; aucun pendant qu'elle parle, ni dans le silence qui suit.
        assert!(!hochements.is_empty());
        for &t in &hochements {
            let apres_une_pause = pauses.iter().any(|&p| (p + 0.2..p + 0.7).contains(&t));
            let second = hochements.iter().any(|&u| (t - u - 0.42).abs() < 0.03);
            assert!(apres_une_pause || second, "{hochements:?}");
        }
    }

    #[test]
    fn dans_le_silence_elle_attend() {
        // Elle écoute, et personne ne parle : la tête penchée, sans un hochement.
        let (mut g, mut h) = (Gestes::default(), Hasard::default());
        let images = filmer(&mut g, &mut h, (0.0, 8.0), Activite::Ecoute, None);
        assert_eq!(g.dernier_hochement, 0.0);
        assert!(images.last().unwrap().1.roulis.abs() > 0.044);
    }

    #[test]
    fn elle_reflechit_la_tete_relevee_sans_bouger() {
        let (mut g, mut h) = (Gestes::default(), Hasard::default());
        filmer(&mut g, &mut h, (0.0, 4.0), Activite::Ecoute, None);
        let images = filmer(&mut g, &mut h, (4.0, 8.0), Activite::Reflechit, None);
        sans_saut(&images);
        // Relevée et redressée en une seconde et demie, puis immobile.
        let posee = &images[90..];
        assert!(posee.iter().all(|i| i.1.y < -0.009 && i.1.roulis.abs() < 0.02), "{}", ecrire(posee[0]));
        let bouge = posee.windows(2).map(|w| (w[1].1.y - w[0].1.y).abs() + (w[1].1.roulis - w[0].1.roulis).abs());
        assert!(posee[60..].windows(2).all(|w| (w[1].1.y - w[0].1.y).abs() < 1e-4), "elle bouge encore");
        assert!(bouge.fold(0.0f32, f32::max) < 0.002);
    }

    #[test]
    fn au_repos_les_sourcils_attendent_le_calme() {
        // Le levé du repos est dû à 12 s ; Eve parle de 10 s à 14 s et finit sur une exclamation (ses sourcils levés à
        // 13,35 s). Revenue au repos, elle ne les relève pas tant qu'ils viennent de retomber : pas avant 17,35 s.
        let (mut g, mut h) = (Gestes::default(), Hasard::default());
        filmer(&mut g, &mut h, (0.0, 10.0), Activite::Repos, None);
        let reperes: Arc<[Repere]> = [repere(3.5, Marque::Exclamation)].into();
        let parle = filmer(&mut g, &mut h, (10.0, 4.0), Activite::Parle, Some((10.0, &reperes)));
        let repos = filmer(&mut g, &mut h, (14.0, 6.0), Activite::Repos, None);
        assert!(parle.iter().any(|i| i.2 > 0.5), "l'exclamation lève les sourcils");
        let calme = repos.iter().filter(|i| (14.6..17.3).contains(&i.0));
        assert!(calme.clone().all(|i| i.2 < 0.05), "{}", calme.map(|&i| ecrire(i)).collect::<Vec<_>>().join("\n"));
        assert!(repos.iter().any(|i| i.2 > 0.5), "puis le levé du repos");
    }

    #[test]
    fn au_repos_la_posture_se_garde() {
        // Au repos, en vingt secondes : deux à six postures, chacune gardée trois secondes et demie au moins.
        let (mut g, mut h) = (Gestes::default(), Hasard::default());
        let mut changements = Vec::new();
        let dt = 1.0 / 60.0;
        for k in 0..1200 {
            let temps = 2.0 + k as f64 * dt;
            let avant = g.derniere_posture;
            let m = Moment { temps, dt: dt as f32, activite: Activite::Repos, vivant: true, ..Default::default() };
            g.bouger(&m, 0.0, Tete::default(), (1.0, 1.0), None, &mut h);
            if g.derniere_posture != avant {
                changements.push(temps);
            }
        }
        assert!((2..=6).contains(&changements.len()), "{changements:?}");
        assert!(changements.windows(2).all(|w| w[1] - w[0] >= 3.5), "{changements:?}");
    }
}
