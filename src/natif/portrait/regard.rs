//! Le regard du portrait d'Eve : des yeux vivants, qui ne flottent jamais. Comme l'œil, ils se posent, restent posés un
//! moment, puis bondissent d'un coup vers ce qu'ils regardent ensuite (une saccade : une à trois images) — la personne
//! d'abord (la photo la regarde) : de ses yeux à sa bouche ; ailleurs un moment, en haut en cherchant une idée, et
//! retour. Ce que fait Eve les guide : en écoutant, elle ne quitte pas la personne ; en parlant, elle s'en écarte souvent
//! au début d'une phrase ou à une pause, et y revient avant la fin ; en réfléchissant, ses yeux cherchent — en haut d'un
//! côté puis de l'autre, en bas — sur son air pensif ; au repos, elle regarde parfois autour d'elle. Elle cligne au
//! rythme d'une personne — plus en parlant, moins en écoutant —, souvent aux pauses de sa voix et à la fin d'une
//! phrase ; et quand ses yeux reviennent de loin, le clignement cache le bond.
//!
//! Les poses des yeux fermés regardent la personne : un clignement pendant que les yeux sont ailleurs ramènerait l'iris
//! au milieu le temps de se fermer, puis le renverrait ailleurs en se rouvrant. Le clignement ramène donc les yeux à la
//! personne, le bond caché pendant qu'ils sont fermés (des yeux qui viennent de partir, il les attend un instant).
//!
//! Les yeux vivent aussi sous une émotion. Une émotion qui change les yeux de la photo (plissés de rire, baissés de
//! peine, grands ouverts de surprise) a les siens — ses regards, dans les mêmes directions, et ses yeux qui se ferment,
//! faits sur son visage ; ils se posent sur lui, couche par couche comme les visages ([`Emotions::couches`]), chacun à
//! la part de son émotion : le même regard et le même clignement, sur chaque visage. Ils passent par-dessus tout, ses
//! bouches comprises — grande ouverte, une bouche d'émotion change aussi un peu ses yeux. Une émotion qui couvre les
//! sourcils de la photo (la peine, le souci, l'air pensif ; le rire et le sérieux en partie, qui en laissaient un
//! fantôme) a de même ses sourcils levés : sous elle, ils se lèvent avec ceux de la photo ([`super::gestes`]), le haut
//! de son visage peint avant ses yeux.
//!
//! Et l'émotion qui domine donne au regard son humeur ([`Humeur`]) : la peine baisse les yeux et cligne lentement, les
//! paupières lourdes ; l'empathie et le sérieux tiennent le regard sur la personne ; la surprise le fige, une seconde
//! sans cligner ; le rire le détourne, souvent vers le bas.

use std::time::Duration;

use eve::parole::Emotion;

use super::emotions::Emotions;
use super::mimique::{Hasard, Moment, lisse};
use super::{Activite, Calque, Pose, Strate, Visage};

/// Où regarde chaque pose du regard (`portrait-regard-1`…, dans l'ordre du catalogue `outils\poses_du_portrait.json`),
/// en unités de LivePortrait (à peu près des degrés ; x vers la droite de l'écran, y vers le haut) : ailleurs — de côté,
/// un peu puis franchement, et vers le bas —, puis sur la personne — vers ses yeux, sa bouche —, puis en haut, de côté.
/// La photo regarde la personne, droit devant. Un essai vérifie que le catalogue dit la même chose.
const DIRECTIONS: [[f32; 2]; 10] = [
    [-10.0, 0.0],
    [10.0, 0.0],
    [0.0, -12.0],
    [-20.0, 0.0],
    [20.0, 0.0],
    [-5.0, 0.0],
    [5.0, 0.0],
    [0.0, -8.0],
    [8.0, 10.0],
    [-8.0, 10.0],
];
/// Le regard de son air pensif (`portrait-emotion-reflexion`, en haut à droite de l'écran) : en réfléchissant, les yeux
/// vont à la pose la plus proche de lui ; son air pensif vient et s'en va sans que l'iris ne bouge.
const PENSIF: [f32; 2] = [8.0, 10.0];
/// La durée d'un clignement, en secondes, d'une fois à l'autre un peu plus ou un peu moins.
const DUREE_DU_CLIN: f64 = 0.3;
/// La voix est haute (une syllabe) au-dessus de ce niveau ; en dessous depuis [`PAUSE`] secondes, elle fait une pause.
const HAUTE: f32 = 0.3;
const PAUSE: f64 = 0.2;
/// La phrase finit bientôt : dans ce temps, elle aura fini.
const FIN_PROCHE: Duration = Duration::from_millis(600);
/// Le temps le plus court d'un regard posé, en secondes : l'œil ne refait pas une saccade avant.
const REPOS_DE_L_OEIL: f64 = 0.15;

/// Où sont les yeux : sur la personne (à quelques degrés d'elle au plus : ses yeux, sa bouche), ailleurs (un peu de
/// côté, vers le bas), loin (franchement de côté), en haut.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ou {
    Personne,
    Ailleurs,
    Loin,
    Haut,
}

impl Ou {
    /// Où sont les yeux qui regardent vers `cible` : une pose du regard (son rang), ou la personne (`None` : la photo).
    fn de(cible: Option<usize>) -> Ou {
        let [x, y] = direction(cible);
        match x.hypot(y) {
            d if d <= 8.5 => Ou::Personne,
            _ if y > 5.0 => Ou::Haut,
            d if d <= 14.0 => Ou::Ailleurs,
            _ => Ou::Loin,
        }
    }
}

/// La direction d'une cible du regard : celle de sa pose ([`DIRECTIONS`]), ou droit devant (`None` : la photo).
fn direction(cible: Option<usize>) -> [f32; 2] {
    cible.and_then(|k| DIRECTIONS.get(k)).copied().unwrap_or([0.0, 0.0])
}

/// Ce que fait la voix à cette image : une phrase commence ; elle fait une pause (une fois par pause) ; elle finira
/// dans un instant ; elle vient de finir.
#[derive(Clone, Copy, Default)]
struct Phrase {
    commence: bool,
    pause: bool,
    fin_proche: bool,
    finie: bool,
}

/// Le regard, d'une image à l'autre. Les yeux : la pose où ils sont (`None` : la photo, sur la personne), celle où ils
/// vont, l'avance du bond (de 0 à 1) et sa durée, depuis quand ils y vont et jusqu'à quand ils y restent ; s'ils
/// réfléchissent. Le clignement : quand commence le prochain (ou celui en cours), s'il est en cours, sa durée, jusqu'où
/// il ferme les yeux, quand finissait le dernier, le bond qu'il cache. La voix : une phrase s'entendait, quand elle était
/// haute pour la dernière fois, si sa pause a eu sa part. L'activité d'avant. L'émotion qui domine, l'humeur qu'elle
/// donne au regard, et jusqu'à quand elle le fige.
#[derive(Default)]
pub(super) struct Regard {
    vu: Option<usize>,
    cible: Option<usize>,
    bond: f32,
    duree_du_bond: f32,
    depuis: f64,
    jusqua: f64,
    pensif: bool,
    clin: f64,
    en_clin: bool,
    duree_du_clin: f64,
    profondeur: f32,
    fin_du_clin: f64,
    bond_cache: Option<Option<usize>>,
    entendait: bool,
    haute_a: f64,
    pause_vue: bool,
    activite: Activite,
    pret: bool,
    emue: Option<Emotion>,
    humeur: Humeur,
    fige_jusqua: f64,
}

/// L'humeur du regard, que lui donne l'émotion qui domine : la part de ses coups d'œil ailleurs (une part de ceux
/// d'office), et celle qui va vers le bas ; combien de temps les yeux restent sur la personne, et la part des petits
/// bonds vers ses traits (des parts de ceux d'office) ; l'intervalle et la durée de ses clignements (des parts) ; et
/// combien de temps elle fige le regard quand elle vient, sans cligner (en secondes).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Humeur {
    coups_d_oeil: f64,
    en_bas: f64,
    tenue: f64,
    vers_elle: f64,
    intervalle: f64,
    duree: f64,
    fige: f64,
}

impl Default for Humeur {
    fn default() -> Humeur {
        Humeur { coups_d_oeil: 1.0, en_bas: 0.0, tenue: 1.0, vers_elle: 1.0, intervalle: 1.0, duree: 1.0, fige: 0.0 }
    }
}

impl Humeur {
    /// L'humeur du regard sous l'émotion `e`. La peine baisse les yeux : ils restent bas un peu plus longtemps, se
    /// relèvent un peu moins souvent vers la personne (assez pour ne pas se figer : au film, un seul coup d'œil en sept
    /// secondes de peine paraissait éteint), s'en vont plutôt vers le bas, et elle cligne lentement, les paupières
    /// lourdes. L'empathie et le sérieux
    /// tiennent le regard sur la personne : moins de coups d'œil, plus longtemps sur elle — l'empathie cligne
    /// doucement, le sérieux moins souvent. La surprise fige le regard sur la personne, une seconde sans cligner, puis
    /// moins qu'à l'ordinaire. On rit en détournant les yeux, souvent vers le bas, en clignant plus. La joie, l'air
    /// pensif (qui a son regard à lui) et le calme : le regard d'office.
    fn de(e: Option<Emotion>) -> Humeur {
        let d = Humeur::default();
        match e {
            Some(Emotion::Triste) => {
                Humeur { coups_d_oeil: 0.8, en_bas: 0.7, tenue: 1.15, vers_elle: 0.8, duree: 1.35, ..d }
            }
            Some(Emotion::Empathie) => {
                Humeur { coups_d_oeil: 0.5, en_bas: 0.4, tenue: 1.3, vers_elle: 0.8, intervalle: 1.15, duree: 1.2, ..d }
            }
            Some(Emotion::Serieuse) => Humeur { coups_d_oeil: 0.5, tenue: 1.4, vers_elle: 0.7, intervalle: 1.3, ..d },
            Some(Emotion::Surprise) => {
                Humeur { coups_d_oeil: 0.0, tenue: 1.5, vers_elle: 0.3, intervalle: 1.4, fige: 1.1, ..d }
            }
            Some(Emotion::Amusee) => Humeur { coups_d_oeil: 1.3, en_bas: 0.4, tenue: 0.85, intervalle: 0.85, ..d },
            _ => d,
        }
    }
}

impl Regard {
    /// Le regard à cette image, les `emotions` du visage à la leur, les sourcils levés de `leves` (de 0 à 1, par les
    /// gestes) : les yeux de la photo, et sur le visage de chaque émotion, le haut du sien ([`Regard::peindre`]) ; son
    /// air pensif garde les yeux de la photo (ses sourcils changent, pas ses paupières) : le regard qui réfléchit s'y
    /// pose et y cherche. Sans les animations de Windows, les yeux restent sur la personne ; elle cligne encore.
    pub(super) fn avancer(
        &mut self,
        m: &Moment,
        v: &Visage,
        emotions: &Emotions,
        leves: f32,
        h: &mut Hasard,
        pose: &mut Pose,
    ) {
        let [_, regards, ..] = &v.calques;
        let phrase = self.ecouter(m);
        let dominante = emotions.dominante();
        // Pensive : son air pensif domine — en réfléchissant, il va et vient, et entre deux elle revient à la personne
        // ([`Emotions`]) ; un visage qui n'a pas cet air cherche des yeux tant qu'elle réfléchit.
        let a_l_air = v.expressions.iter().any(|x| x.emotion == Emotion::Reflexion);
        let pensif = dominante == Some(Emotion::Reflexion) || (m.activite == Activite::Reflechit && !a_l_air);
        let poses = if m.vivant { regards.len().min(DIRECTIONS.len()) } else { 0 };
        self.emouvoir(m.temps, dominante);
        self.vivre(m, phrase, pensif, emotions.posees(), poses, h);
        self.peindre(m.temps, v, emotions.couches(), leves, pose);
    }

    /// L'émotion qui domine à cette image donne son humeur au regard ([`Humeur`]) ; une qui vient de saisir Eve et le
    /// fige (la surprise) retient ses clignements un instant.
    fn emouvoir(&mut self, t: f64, dominante: Option<Emotion>) {
        if dominante != self.emue {
            self.emue = dominante;
            self.humeur = Humeur::de(dominante);
            if self.humeur.fige > 0.0 {
                self.fige_jusqua = t + self.humeur.fige;
                if !self.en_clin {
                    self.clin = self.clin.max(self.fige_jusqua);
                }
            }
        }
    }

    /// Dans combien de secondes elle clignera.
    pub(super) fn prochain_clin(&self, temps: f64) -> f64 {
        if self.en_clin { 0.0 } else { (self.clin - temps).max(0.0) }
    }

    /// La voix à cette image, d'après ce qui s'en entend ([`crate::voix::bouche_du_moment`]) et ce qui s'en entendra
    /// dans un instant ([`crate::voix::forme_a_venir`] : rien, la phrase aura fini).
    fn ecouter(&mut self, m: &Moment) -> Phrase {
        let maintenant = if m.parle() { crate::voix::bouche_du_moment().map(|a| a.niveau) } else { None };
        let fin_proche = maintenant.is_some() && crate::voix::forme_a_venir(FIN_PROCHE).is_none();
        self.phrase(m.temps, maintenant, fin_proche)
    }

    /// La phrase à l'heure `t`, la voix à son `niveau` (`None` : rien ne s'entend), qui finira dans un instant ou non.
    fn phrase(&mut self, t: f64, niveau: Option<f32>, fin_proche: bool) -> Phrase {
        let (commence, finie) = (niveau.is_some() && !self.entendait, niveau.is_none() && self.entendait);
        self.entendait = niveau.is_some();
        if niveau.is_none_or(|n| n >= HAUTE) {
            (self.haute_a, self.pause_vue) = (t, false);
        }
        let pause = !self.pause_vue && t - self.haute_a >= PAUSE;
        self.pause_vue |= pause;
        Phrase { commence, pause, fin_proche, finie }
    }

    /// Ce que font les yeux à cette image : où ils vont, quand ils clignent. `pensif` : elle réfléchit, ou son air pensif
    /// domine ; `calmes` : ses émotions sont posées (aucune ne vient ni ne s'en va) ; `poses` : les poses du regard dont
    /// la direction est connue (aucune : les yeux restent sur la personne).
    fn vivre(&mut self, m: &Moment, phrase: Phrase, pensif: bool, calmes: bool, poses: usize, h: &mut Hasard) {
        let t = m.temps;
        if !self.pret {
            self.pret = true;
            (self.clin, self.duree_du_clin, self.profondeur, self.bond, self.jusqua) =
                (t + 2.0, DUREE_DU_CLIN, 1.0, 1.0, t + 1.0);
        }
        let ecoute = m.activite == Activite::Ecoute && self.activite != Activite::Ecoute;
        self.activite = m.activite;
        // Son air pensif garde les yeux là où il regarde tant qu'il s'efface : l'iris ne bouge pas sous lui.
        self.pensif = pensif || (self.pensif && !calmes);
        // À la fin d'une phrase, elle cligne souvent ; à une pause, elle cligne (une fois sur trois) ou jette un coup d'œil
        // ailleurs (une fois sur trois) ; au début d'une phrase, un coup d'œil ailleurs, trois fois sur cinq ; en se
        // mettant à écouter, elle cligne une fois sur deux.
        let fin = phrase.finie || (phrase.pause && phrase.fin_proche);
        let c = self.humeur.coups_d_oeil;
        let (clin, coup_d_oeil) = if fin || phrase.pause || phrase.commence || ecoute {
            let r = h.tirer();
            match () {
                _ if fin => (r < 0.7, false),
                _ if phrase.pause => (r < 0.35, (0.35..0.35 + 0.3 * c).contains(&r)),
                _ if phrase.commence => (false, r < 0.6 * c),
                _ => (r < 0.5, false),
            }
        } else {
            (false, false)
        };
        if clin {
            self.cligner(t);
        }
        self.regarder(m, phrase.fin_proche, coup_d_oeil, calmes, poses, h);
        self.avancer_le_clin(t, h);
        self.bond = (self.bond + m.dt / self.duree_du_bond.max(0.001)).min(1.0);
        if self.bond >= 1.0 {
            self.vu = self.cible;
        }
    }

    /// Où vont les yeux à cette image : `fin_proche`, sa phrase finit dans un instant ; `coup_d_oeil`, sa voix lui en
    /// donne envie (au début d'une phrase, à une pause).
    fn regarder(
        &mut self,
        m: &Moment,
        fin_proche: bool,
        coup_d_oeil: bool,
        calmes: bool,
        poses: usize,
        h: &mut Hasard,
    ) {
        let t = m.temps;
        if poses == 0 {
            if self.cible.is_some() || self.bond_cache.is_some() {
                self.aller(None, 1.0, t, false);
            }
            return;
        }
        // En réfléchissant, les yeux quittent la personne pour là où regarde son air pensif, et y restent tant qu'il vient
        // ou s'en va ; posé, ils cherchent — en haut d'un côté puis de l'autre, en bas, de côté —, chaque regard un moment.
        // Pensive en parlant, elle regarde aussi la personne (ses petits regards, sur son air pensif), une fois sur deux.
        if self.pensif {
            let pensive = plus_proche(PENSIF, poses, Ou::Haut);
            if !calmes || self.cible.is_none() {
                if pensive != self.cible {
                    self.aller(pensive, 1.0, t, false);
                }
            } else if t >= self.jusqua && t - self.depuis >= REPOS_DE_L_OEIL {
                let r = h.tirer();
                let (personne, la_sienne, en_haut) =
                    if m.activite == Activite::Parle { (0.5, 0.7, 0.85) } else { (0.0, 0.35, 0.7) };
                let cible = match () {
                    _ if r < personne => sur_elle(poses, self.cible, h),
                    _ if r < la_sienne => pensive,
                    _ if r < en_haut => au_hasard(poses, self.cible, h, |o| o == Ou::Haut),
                    _ => au_hasard(poses, self.cible, h, |o| o == Ou::Ailleurs),
                };
                let pour = 0.8 + 1.6 * h.tirer();
                self.aller(cible.or(pensive), pour, t, false);
            }
            return;
        }
        // Un regard posé l'est au moins un instant, comme l'œil entre deux saccades.
        if t - self.depuis < REPOS_DE_L_OEIL {
            return;
        }
        let parle = m.activite == Activite::Parle;
        let humeur = self.humeur;
        if Ou::de(self.cible) != Ou::Personne {
            // Ailleurs, elle revient à la personne au bout de son coup d'œil, ou plus tôt — sa phrase finit, une émotion
            // vient, elle se met à écouter —, souvent en clignant.
            if t >= self.jusqua || !calmes || fin_proche || m.activite == Activite::Ecoute {
                let pour = tenue(None, m.activite, &humeur, h);
                let avec_clin = h.tirer() < 0.6;
                self.aller(None, pour, t, avec_clin);
            }
            return;
        }
        // Sur la personne : le coup d'œil ailleurs que sa voix lui donne envie de jeter, pas quand elle finit ni quand une
        // émotion vient.
        let libre = calmes && !fin_proche;
        if libre && coup_d_oeil {
            if let Some(k) = ailleurs(poses, parle, self.cible, humeur.en_bas, h) {
                let pour = tenue(Some(k), m.activite, &humeur, h);
                self.aller(Some(k), pour, t, false);
                return;
            }
        }
        if t < self.jusqua {
            return;
        }
        // Au bout de chaque regard posé : sur la photo, de loin en loin un coup d'œil ailleurs (jamais en écoutant), sinon
        // un petit bond vers un de ses traits, ou rester ; sur un de ses traits, le retour à elle, ou un autre trait.
        // Son humeur en change la part.
        let r = h.tirer();
        let cible = match self.cible {
            None => {
                let (loin, pres) = match m.activite {
                    Activite::Parle => (0.12, 0.6),
                    Activite::Ecoute => (0.0, 0.65),
                    _ => (0.15, 0.6),
                };
                let (loin, pres) = (loin * humeur.coups_d_oeil, pres * humeur.vers_elle);
                if libre && r < loin {
                    ailleurs(poses, parle, None, humeur.en_bas, h)
                } else if r < pres {
                    sur_elle(poses, None, h)
                } else {
                    None
                }
            }
            Some(_) if r < 0.55 => None,
            ici => sur_elle(poses, ici, h),
        };
        let pour = tenue(cible, m.activite, &humeur, h);
        self.aller(cible, pour, t, false);
    }

    /// Les yeux vont à `cible` et y restent `pour` secondes ; `avec_clin` : elle cligne en même temps (si elle le peut),
    /// et le bond se fait les yeux fermés. Un bond qui attendait les yeux fermés va à la nouvelle cible.
    fn aller(&mut self, cible: Option<usize>, pour: f64, t: f64, avec_clin: bool) {
        self.jusqua = t + pour;
        if self.bond_cache.is_some() {
            self.bond_cache = Some(cible);
        } else if cible != self.cible {
            self.depuis = t;
            if avec_clin && self.cligner(t) {
                self.bond_cache = Some(cible);
            } else {
                self.bondir(cible, false);
            }
        }
    }

    /// Le bond des yeux vers `cible`, d'où ils sont (un bond en cours s'achève d'un coup) : une à trois images, d'autant
    /// plus long qu'il va loin (deux centièmes de seconde, plus un peu plus d'un millième par degré). Une pose se fond
    /// dans l'autre : plus le bond est court, moins les deux iris se voient à la fois — l'œil, lui, passe en un éclair.
    /// Les yeux fermés (`cache`), immédiat.
    fn bondir(&mut self, cible: Option<usize>, cache: bool) {
        let ([x0, y0], [x1, y1]) = (direction(self.cible), direction(cible));
        (self.vu, self.cible) = (if cache { cible } else { self.cible }, cible);
        self.bond = if self.vu == cible { 1.0 } else { 0.0 };
        self.duree_du_bond = 0.018 + 0.0012 * (x1 - x0).hypot(y1 - y0);
    }

    /// Un clignement tout de suite, s'il peut venir : ni pendant un autre, ni dans la demi-seconde qui le suit, ni
    /// quand une émotion fige le regard ; entier. Rend s'il vient.
    fn cligner(&mut self, t: f64) -> bool {
        let peut = !self.en_clin && t - self.fin_du_clin >= 0.5 && t >= self.fige_jusqua;
        if peut {
            (self.clin, self.profondeur) = (self.clin.min(t), 1.0);
        }
        peut
    }

    /// Le clignement à cette image. Il commence à son heure, et des yeux ailleurs ou posés sur un trait de la personne
    /// reviennent à elle pendant qu'il les ferme — sauf des yeux qui viennent de partir : il les attend un peu. Les yeux
    /// presque fermés, le bond qu'il cache se fait. Fini, le suivant est tiré au rythme de ce que fait Eve, parfois tout de
    /// suite (deux de suite), parfois à demi.
    fn avancer_le_clin(&mut self, t: f64, h: &mut Hasard) {
        if !self.en_clin && t >= self.clin {
            let libres = self.pensif || self.bond_cache.is_some() || self.cible.is_none();
            if !libres && Ou::de(self.cible) != Ou::Personne && t - self.depuis < 0.35 {
                self.clin = t + 0.2;
            } else {
                if !libres {
                    (self.bond_cache, self.profondeur) = (Some(None), 1.0);
                    (self.depuis, self.jusqua) = (t, t + tenue(None, self.activite, &self.humeur, h));
                }
                (self.en_clin, self.clin) = (true, t);
            }
        }
        if !self.en_clin {
            return;
        }
        let p = (t - self.clin) / self.duree_du_clin;
        if fermeture(p) >= 0.85 || p >= 1.0 {
            if let Some(cible) = self.bond_cache.take() {
                self.bondir(cible, true);
            }
        }
        if p >= 1.0 {
            (self.en_clin, self.fin_du_clin) = (false, t);
            let deux = h.tirer() < 0.1;
            let (a, b) = intervalle(self.activite);
            let humeur = self.humeur;
            self.clin = t + if deux { 0.05 } else { (a + (b - a) * h.tirer()) * humeur.intervalle };
            self.duree_du_clin = DUREE_DU_CLIN * (0.85 + 0.3 * h.tirer()) * humeur.duree;
            self.profondeur = if !deux && h.tirer() < 0.15 { 0.6 + 0.25 * h.tirer() as f32 } else { 1.0 };
        }
    }

    /// Les yeux à cette image, couche par couche comme les visages (`couches` : les émotions, dans l'ordre où leurs
    /// visages se peignent, chacune à sa part). Ceux de la photo d'abord : son regard sous les visages — une émotion
    /// qui change les yeux le couvre —, son clignement par-dessus tout, sauf là où une émotion a les siens. Puis, sur
    /// le visage de chaque émotion, à sa part et par-dessus tout, le haut du sien ([`haut_de`]) : ses sourcils levés de
    /// `leves`, comme ceux de la photo ; ses yeux à elle, ou sur son air pensif ceux de la photo. Partout le même
    /// regard, le même clignement, les mêmes sourcils.
    fn peindre(&self, t: f64, v: &Visage, couches: [(Option<Emotion>, f32); 2], leves: f32, pose: &mut Pose) {
        let [yeux, regards, ..] = &v.calques;
        let couches = couches.map(|(e, part)| haut_de(v, e, part.clamp(0.0, 1.0)));
        self.peindre_le_regard(Strate::Dessous, regards, 1.0, pose);
        let reste = couches.iter().filter(|y| y.siens).map(|y| 1.0 - y.part).product();
        self.peindre_le_clin(t, yeux, reste, pose);
        for y in &couches {
            pose.serie(Strate::Dessus, y.sourcils, leves, y.part);
            self.peindre_le_regard(Strate::Dessus, y.regards, y.part, pose);
            self.peindre_le_clin(t, y.fermes, y.part, pose);
        }
    }

    /// Le regard, d'une série de `regards` (dans les directions de [`DIRECTIONS`]), à la part `part` : la pose d'où les
    /// yeux bondissent, entière, et celle où ils vont, qui se fond en elle ; vers la personne, celle d'où ils partent
    /// s'efface.
    fn peindre_le_regard(&self, strate: Strate, regards: &[Calque], part: f32, pose: &mut Pose) {
        let e = lisse(self.bond);
        let calque = |k: Option<usize>| k.and_then(|k| regards.get(k));
        match (calque(self.vu), calque(self.cible)) {
            (Some(a), Some(b)) if self.vu != self.cible => {
                pose.peindre(strate, a, part);
                pose.peindre(strate, b, e * part);
            }
            (Some(a), None) => pose.peindre(strate, a, (1.0 - e) * part),
            (_, Some(b)) => pose.peindre(strate, b, e * part),
            (None, None) => {}
        }
    }

    /// Le clignement, d'une série de `yeux` qui se ferment, par-dessus tout, à la part `part` ; des yeux ailleurs
    /// (pensifs, ou que le clignement ramène) se ferment sans passer par les yeux mi-clos, qui regardent la personne.
    fn peindre_le_clin(&self, t: f64, yeux: &[Calque], part: f32, pose: &mut Pose) {
        if self.en_clin {
            let fermes = fermeture((t - self.clin) / self.duree_du_clin) as f32 * self.profondeur;
            let ailleurs = Ou::de(self.cible) != Ou::Personne;
            let serie = if ailleurs && yeux.len() > 1 { &yeux[1..] } else { yeux };
            pose.serie(Strate::Dessus, serie, fermes, part);
        }
    }

    /// Où regardent les yeux à cette image, en unités de LivePortrait (à peu près des degrés ; x vers la droite de
    /// l'écran, y vers le haut ; la personne : droit devant) — d'où ils bondissent vers où ils vont : la tête les suit
    /// ([`super::gestes`]).
    pub(super) fn direction(&self) -> [f32; 2] {
        let ([x0, y0], [x1, y1], e) = (direction(self.vu), direction(self.cible), lisse(self.bond));
        [x0 + (x1 - x0) * e, y0 + (y1 - y0) * e]
    }
}

/// Le haut d'une couche du visage, à poser sur elle : ses sourcils levés, ses regards (dans les directions de
/// [`DIRECTIONS`]) et ses yeux qui se ferment, sa part, et si ses yeux sont à elle (ceux de la photo ne s'y posent
/// plus).
struct Haut<'a> {
    sourcils: &'a [Calque],
    regards: &'a [Calque],
    fermes: &'a [Calque],
    part: f32,
    siens: bool,
}

/// Le haut du visage de l'émotion `e`, à sa `part` : ses sourcils levés s'il en a (elle couvre ceux de la photo, que
/// les gestes lèvent dessous) ; ses yeux s'il en a (sa pose change ceux de la photo) — sur son air pensif, ceux de la
/// photo, que le regard qui réfléchit y pose ; sinon aucun : ses yeux sont ceux de la photo, dessous, ou ceux de son
/// visage (un portrait d'avant ses yeux : ils restent tels quels, la photo y clignant).
fn haut_de(v: &Visage, e: Option<Emotion>, part: f32) -> Haut<'_> {
    let [yeux, regards, ..] = &v.calques;
    let x = e.and_then(|e| v.expressions.iter().find(|x| x.emotion == e));
    let pensive = x.is_some_and(|x| x.emotion == Emotion::Reflexion);
    let (sourcils, ses_regards, ses_yeux): (&[Calque], &[Calque], &[Calque]) =
        x.map_or((&[], &[], &[]), |x| (&x.sourcils, &x.regards, &x.yeux));
    let ceux_de_la_photo = |siens: &[Calque]| siens.is_empty() && pensive;
    Haut {
        sourcils,
        regards: if ceux_de_la_photo(ses_regards) { regards } else { ses_regards },
        fermes: if ceux_de_la_photo(ses_yeux) { yeux } else { ses_yeux },
        part,
        siens: !ses_yeux.is_empty(),
    }
}

/// Combien de temps les yeux restent posés sur `cible`, en secondes, selon ce que fait Eve : sur la personne, plus
/// longtemps au repos ; sur un de ses traits, moins ; ailleurs, le temps d'un coup d'œil en parlant, plus au repos. Son
/// `humeur` allonge ou raccourcit le temps sur la personne.
fn tenue(cible: Option<usize>, activite: Activite, humeur: &Humeur, h: &mut Hasard) -> f64 {
    let parle = activite == Activite::Parle;
    let (a, b) = match (cible, Ou::de(cible)) {
        (None, _) => match activite {
            Activite::Parle => (0.7, 2.0),
            Activite::Ecoute => (0.6, 1.6),
            _ => (1.0, 3.0),
        },
        (_, Ou::Personne) if parle => (0.3, 0.9),
        (_, Ou::Personne) => (0.45, 1.3),
        _ if parle => (0.45, 1.2),
        _ => (0.8, 2.2),
    };
    (a + (b - a) * h.tirer()) * if cible.is_none() { humeur.tenue } else { 1.0 }
}

/// L'intervalle entre deux clignements, en secondes, selon ce que fait Eve : une personne cligne plus en parlant, moins
/// en écoutant, attentive ; une quinzaine de fois par minute au repos. En réfléchissant, moins encore : sous son air
/// pensif, les yeux fermés (qui regardent la personne) ramènent l'iris au milieu le temps du clignement.
fn intervalle(activite: Activite) -> (f64, f64) {
    match activite {
        Activite::Parle => (2.4, 5.0),
        Activite::Ecoute => (3.2, 6.5),
        Activite::Reflechit => (3.5, 7.0),
        Activite::Repos => (2.6, 5.8),
    }
}

/// Un coup d'œil ailleurs, au hasard parmi les `poses` premières : en parlant, pas loin (franchement de côté, elle
/// paraîtrait distraite) ; jamais là où les yeux sont déjà (`ici`). Une part `en_bas` d'entre eux vont vers le bas
/// (l'humeur d'une peine, d'un rire).
fn ailleurs(poses: usize, parle: bool, ici: Option<usize>, en_bas: f64, h: &mut Hasard) -> Option<usize> {
    if en_bas > 0.0 && h.tirer() < en_bas {
        let bas = (0..poses).filter(|&k| Some(k) != ici && Ou::de(Some(k)) == Ou::Ailleurs && DIRECTIONS[k][1] < -10.0);
        if let Some(k) = bas.min_by(|&a, &b| DIRECTIONS[a][1].total_cmp(&DIRECTIONS[b][1])) {
            return Some(k);
        }
    }
    au_hasard(poses, ici, h, |o| matches!(o, Ou::Ailleurs | Ou::Haut) || (o == Ou::Loin && !parle))
}

/// Un trait de la personne (ses yeux, sa bouche), au hasard parmi les `poses` premières ; jamais là où les yeux sont
/// déjà (`ici`).
fn sur_elle(poses: usize, ici: Option<usize>, h: &mut Hasard) -> Option<usize> {
    au_hasard(poses, ici, h, |o| o == Ou::Personne)
}

/// Une des `poses` premières que `garder` retient (d'après où elle regarde), au hasard, sauf `ici` ; aucune : `None`.
fn au_hasard(poses: usize, ici: Option<usize>, h: &mut Hasard, garder: impl Fn(Ou) -> bool) -> Option<usize> {
    let gardees: Vec<usize> = (0..poses).filter(|&k| Some(k) != ici && garder(Ou::de(Some(k)))).collect();
    let i = (h.tirer() * gardees.len() as f64) as usize;
    gardees.get(i.min(gardees.len().saturating_sub(1))).copied()
}

/// La pose qui regarde `ou`, parmi les `poses` premières, la plus proche de la direction `d` ; aucune : `None`.
fn plus_proche(d: [f32; 2], poses: usize, ou: Ou) -> Option<usize> {
    let ecart = |k: usize| (DIRECTIONS[k][0] - d[0]).hypot(DIRECTIONS[k][1] - d[1]);
    (0..poses).filter(|&k| Ou::de(Some(k)) == ou).min_by(|&a, &b| ecart(a).total_cmp(&ecart(b)))
}

/// La fermeture des yeux (0 : ouverts, 1 : fermés) à `p` (de 0 à 1) d'un clignement : ils se ferment vite, restent
/// fermés un instant, se rouvrent plus lentement — chaque mouvement adouci au départ et à l'arrivée.
fn fermeture(p: f64) -> f64 {
    let lisse = |t: f64| t * t * (3.0 - 2.0 * t);
    match p {
        p if !(0.0..1.0).contains(&p) => 0.0,
        p if p < 0.3 => lisse(p / 0.3),
        p if p < 0.45 => 1.0,
        p => 1.0 - lisse((p - 0.45) / 0.55),
    }
}

#[cfg(test)]
mod essais {
    use std::collections::HashMap;
    use std::sync::Arc;

    use eframe::egui;

    use super::super::Expression;
    use super::*;

    /// Un visage d'essai, un pixel par calque, et le nom de chacun : la photo, ses yeux qui se ferment (`yeux-1`…) et
    /// ses regards (`regard-1`…) ; la tristesse, qui a les siens (`triste-regard-1`…, `triste-yeux-1`…) et ses sourcils
    /// levés (`triste-sourcils-1`…) ; la joie, qui n'en a pas ; l'air pensif.
    fn visage_d_essai() -> (Visage, HashMap<egui::TextureId, String>) {
        let ctx = egui::Context::default();
        let blanc = Arc::new(egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]));
        let entiere = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
        let mut noms = HashMap::new();
        let mut serie = |nom: &str, n: usize| -> Vec<Calque> {
            (1..=n)
                .map(|k| {
                    let nom = format!("{nom}-{k}");
                    let texture = ctx.load_texture(nom.clone(), blanc.clone(), egui::TextureOptions::default());
                    noms.insert(texture.id(), nom);
                    Calque { texture, image: blanc.clone(), uv: entiere, geometrie: Default::default() }
                })
                .collect()
        };
        let photo = serie("photo", 1).remove(0);
        let mut calques: [Vec<Calque>; 7] = Default::default();
        (calques[0], calques[1]) = (serie("yeux", 3), serie("regard", DIRECTIONS.len()));
        let mut expressions = Vec::new();
        for (emotion, siens) in [(Emotion::Triste, true), (Emotion::Joie, false), (Emotion::Reflexion, false)] {
            let nom = emotion.nom();
            let visage = serie(nom, 1).remove(0);
            let (regards, yeux, sourcils) = if siens {
                let regards = serie(&format!("{nom}-regard"), DIRECTIONS.len());
                (regards, serie(&format!("{nom}-yeux"), 3), serie(&format!("{nom}-sourcils"), 3))
            } else {
                (Vec::new(), Vec::new(), Vec::new())
            };
            let (bouches, levres_closes) = (Vec::new(), None);
            expressions.push(Expression { emotion, visage, bouches, levres_closes, regards, yeux, sourcils });
        }
        let visage = Visage {
            texture: photo.texture.clone(),
            image: blanc.clone(),
            taille: [1, 1],
            calques,
            expressions,
            calme: None,
            cheveux: None,
            fond: None,
            cadre: entiere,
            demande: Default::default(),
            net: None,
            vignettes: Default::default(),
        };
        (visage, noms)
    }

    #[test]
    fn sous_une_emotion_ses_yeux_vivent_sur_son_visage() {
        let (v, noms) = visage_d_essai();
        // Ce que les yeux peignent, strate par strate, les sourcils levés de `leves` : chaque calque par son nom, et sa
        // part (au centième).
        let peindre_leves = |r: &Regard, couches, leves| {
            let mut p = Pose::default();
            r.peindre(0.4, &v, couches, leves, &mut p);
            let peints = |s: Strate| -> Vec<(String, f32)> {
                p.couches[s as usize]
                    .iter()
                    .map(|c| (noms[&c.texture].clone(), (c.part * 100.0).round() / 100.0))
                    .collect()
            };
            (peints(Strate::Dessous), peints(Strate::Dessus))
        };
        let peindre = |r: &Regard, couches| peindre_leves(r, couches, 0.0);
        let vus = |calques: &[(&str, f32)]| -> Vec<(String, f32)> {
            calques.iter().map(|&(n, p)| (n.to_string(), p)).collect()
        };
        // Les yeux de côté, fermés au plus fort d'un clignement (au temps 0,4 d'un clignement d'une seconde).
        let fermes = Regard {
            vu: Some(0),
            cible: Some(0),
            bond: 1.0,
            en_clin: true,
            duree_du_clin: 1.0,
            profondeur: 1.0,
            ..Regard::default()
        };
        // Sans émotion : le regard de la photo, sous les visages ; son clignement par-dessus tout — de côté, sans les
        // yeux mi-clos, qui regardent la personne.
        let sans = [(None, 0.0), (None, 0.0)];
        assert_eq!(peindre(&fermes, sans), (vus(&[("regard-1", 1.0)]), vus(&[("yeux-3", 1.0)])));
        // La tristesse, entière : ses yeux à elle, par-dessus tout, et plus ceux de la photo.
        let triste = [(None, 0.0), (Some(Emotion::Triste), 1.0)];
        let siens = vus(&[("triste-regard-1", 1.0), ("triste-yeux-3", 1.0)]);
        assert_eq!(peindre(&fermes, triste), (vus(&[("regard-1", 1.0)]), siens.clone()));
        // La joie n'a pas les siens (ses yeux sont ceux de la photo) : comme sans émotion.
        let joie = [(None, 0.0), (Some(Emotion::Joie), 1.0)];
        assert_eq!(peindre(&fermes, joie).1, vus(&[("yeux-3", 1.0)]));
        // Son air pensif garde ceux de la photo : ils se posent sur lui.
        let pensive = [(None, 0.0), (Some(Emotion::Reflexion), 1.0)];
        assert_eq!(peindre(&fermes, pensive).1, vus(&[("yeux-3", 1.0), ("regard-1", 1.0), ("yeux-3", 1.0)]));
        // La tristesse qui vient, à moitié, sur la joie qui s'efface : le clignement de la photo à moitié, puis les
        // yeux de la tristesse à moitié.
        let passage = [(Some(Emotion::Joie), 0.5), (Some(Emotion::Triste), 0.5)];
        let moitie = vus(&[("yeux-3", 0.5), ("triste-regard-1", 0.5), ("triste-yeux-3", 0.5)]);
        assert_eq!(peindre(&fermes, passage).1, moitie);
        // Les yeux ouverts, à mi-chemin d'un bond de côté vers la personne : sous la tristesse, le même bond, dans ses
        // yeux à elle.
        let bond = Regard { vu: Some(0), cible: Some(5), bond: 0.5, ..Regard::default() };
        let attendu =
            (vus(&[("regard-1", 1.0), ("regard-6", 0.5)]), vus(&[("triste-regard-1", 1.0), ("triste-regard-6", 0.5)]));
        assert_eq!(peindre(&bond, triste), attendu);
        // Les sourcils levés à mi-hauteur sous la tristesse : les siens, d'abord (le premier, et la moitié du
        // deuxième), puis ses yeux par-dessus ; sous la joie, rien (ceux de la photo se lèvent dessous).
        let leves = peindre_leves(&fermes, triste, 0.5).1;
        let sous = vus(&[("triste-sourcils-1", 1.0), ("triste-sourcils-2", 0.5)]);
        assert_eq!(leves, [sous, siens].concat());
        assert_eq!(peindre_leves(&fermes, joie, 0.5).1, vus(&[("yeux-3", 1.0)]));
    }

    #[test]
    fn elle_cligne_vite_et_rouvre_lentement() {
        // Les yeux se ferment vite, restent fermés, se rouvrent plus lentement ; ouverts hors du clignement.
        let f = [-0.1, 0.0, 0.15, 0.3, 0.4, 0.725, 1.0].map(fermeture);
        let attendu = [0.0, 0.0, 0.5, 1.0, 1.0, 0.5, 0.0];
        assert!(f.iter().zip(attendu).all(|(a, b)| (a - b).abs() < 1e-9), "{f:?}");
    }

    #[test]
    fn les_directions_sont_celles_du_catalogue() {
        // Le catalogue des poses (il n'est que dans le dépôt de développement : ailleurs, rien à vérifier) dit où regarde
        // chaque pose du regard ; le programme le sait par DIRECTIONS : les deux disent la même chose, pose par pose.
        let chemin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("outils/poses_du_portrait.json");
        let Ok(texte) = std::fs::read_to_string(&chemin) else { return };
        let catalogue: serde_json::Value = serde_json::from_str(&texte).unwrap();
        for (k, [x, y]) in DIRECTIONS.iter().enumerate() {
            let nom = format!("portrait-regard-{}", k + 1);
            let pose = catalogue.get(&nom).unwrap_or_else(|| panic!("{nom} manque au catalogue"));
            let reglage = |r: &str| pose.get(r).and_then(serde_json::Value::as_f64).unwrap_or(0.0) as f32;
            assert_eq!([reglage("regard_x"), reglage("regard_y")], [*x, *y], "{nom}");
        }
        let suivante = format!("portrait-regard-{}", DIRECTIONS.len() + 1);
        assert!(catalogue.get(&suivante).is_none(), "{suivante} : sa direction manque à DIRECTIONS");
        // Les yeux d'une émotion : ses réglages, et ceux de ses yeux — ses regards dans les mêmes directions, chacune
        // dite (une émotion qui baisse les yeux les relève pour regarder la personne), ses yeux qui se ferment, son
        // ouverture fois 0,7, 0,35 et 0.
        let sans = |pose: &serde_json::Value, reglages: &[&str]| {
            let mut p = pose.as_object().cloned().unwrap_or_default();
            p.retain(|r, _| !reglages.contains(&r.as_str()));
            p
        };
        for e in Emotion::TOUTES {
            let nom = format!("portrait-emotion-{}", e.nom());
            let Some(visage) = catalogue.get(&nom) else { continue };
            let mut siens = 0;
            for (k, [x, y]) in DIRECTIONS.iter().enumerate() {
                let Some(pose) = catalogue.get(format!("{nom}-regard-{}", k + 1)) else { continue };
                let reglage = |r: &str| pose.get(r).and_then(serde_json::Value::as_f64).map(|v| v as f32);
                assert_eq!([reglage("regard_x"), reglage("regard_y")], [Some(*x), Some(*y)], "{nom}-regard-{}", k + 1);
                assert_eq!(sans(pose, &["regard_x", "regard_y"]), sans(visage, &["regard_x", "regard_y"]), "{nom}");
                siens += 1;
            }
            let ouverture = visage.get("oeil").and_then(serde_json::Value::as_f64).unwrap_or(1.0);
            for (k, f) in [0.7, 0.35, 0.0].iter().enumerate() {
                let Some(pose) = catalogue.get(format!("{nom}-yeux-{}", k + 1)) else { continue };
                let oeil = pose.get("oeil").and_then(serde_json::Value::as_f64).unwrap_or(-1.0);
                assert!((oeil - ouverture * f).abs() < 1e-3, "{nom}-yeux-{} : {oeil}", k + 1);
                assert_eq!(sans(pose, &["oeil"]), sans(visage, &["oeil"]), "{nom}");
                siens += 1;
            }
            // Tous ou aucun : un regard qui manquerait sous l'émotion ramènerait ses yeux à ceux de son visage.
            assert!(siens == 0 || siens == DIRECTIONS.len() + 3, "{nom} : {siens} poses des yeux");
            // Ses sourcils levés : ceux de son visage, plus autant que ceux de la photo ; les trois, ou aucun.
            let sourcil = |p: &serde_json::Value| p.get("sourcil").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
            let mut leves = 0;
            for k in 1..=3 {
                let Some(pose) = catalogue.get(format!("{nom}-sourcils-{k}")) else { continue };
                let photo = catalogue.get(format!("portrait-sourcils-{k}")).map_or(0.0, sourcil);
                assert!((sourcil(pose) - sourcil(visage) - photo).abs() < 1e-6, "{nom}-sourcils-{k}");
                assert_eq!(sans(pose, &["sourcil"]), sans(visage, &["sourcil"]), "{nom}-sourcils-{k}");
                leves += 1;
            }
            assert!(leves == 0 || leves == 3, "{nom} : {leves} poses des sourcils");
        }
        // Où regarde chacune : les coups d'œil, les traits de la personne, le haut ; la pensive est en haut à droite.
        let ou: Vec<Ou> = (0..DIRECTIONS.len()).map(|k| Ou::de(Some(k))).collect();
        assert_eq!(&ou[..5], [Ou::Ailleurs, Ou::Ailleurs, Ou::Ailleurs, Ou::Loin, Ou::Loin]);
        assert_eq!(&ou[5..], [Ou::Personne, Ou::Personne, Ou::Personne, Ou::Haut, Ou::Haut]);
        assert_eq!(plus_proche(PENSIF, DIRECTIONS.len(), Ou::Haut), Some(8));
        assert_eq!(plus_proche(PENSIF, 5, Ou::Haut), None);
    }

    /// Une voix inventée : des phrases de cinq secondes et demie, une seconde de silence entre elles, deux pauses dans
    /// chacune et sa fin plus basses. Son niveau à l'heure `t` (`None` : rien ne s'entend), et si elle finit bientôt.
    fn voix_inventee(t: f64) -> (Option<f32>, bool) {
        let p = t % 6.5;
        let basse = (1.5..1.9).contains(&p) || (3.2..3.5).contains(&p) || p >= 5.1;
        if p < 5.5 { (Some(if basse { 0.05 } else { 0.9 }), p >= 4.9) } else { (None, false) }
    }

    /// Ce que font les yeux en deux minutes : les clignements (et ceux qui ont commencé les yeux ailleurs sans les
    /// ramener, et leur durée en tout), le temps sur la personne (sur la photo seule, ou un de ses traits), les bonds
    /// et les coups d'œil ailleurs (et ceux qui vont vers le bas), le plus long bond, le plus court regard posé.
    #[derive(Debug, Default)]
    struct Bilan {
        clins: usize,
        clins_ailleurs: usize,
        duree_des_clins: f64,
        sur_elle: f64,
        sur_la_photo: f64,
        bonds: usize,
        coups_d_oeil: usize,
        vers_le_bas: usize,
        plus_long_bond: f32,
        plus_court_regard: f64,
    }

    /// Les yeux pendant deux minutes de l'activité `a`, à soixante images par seconde (en parlant, sur la voix inventée),
    /// avec `poses` poses du regard, les émotions posées (`calmes`) ou non ; et le regard à la fin.
    fn deux_minutes(a: Activite, pensif: bool, calmes: bool, poses: usize) -> (Bilan, Regard) {
        deux_minutes_sous(None, a, pensif, calmes, poses)
    }

    /// De même, l'émotion `emotion` dominant le visage.
    fn deux_minutes_sous(
        emotion: Option<Emotion>,
        a: Activite,
        pensif: bool,
        calmes: bool,
        poses: usize,
    ) -> (Bilan, Regard) {
        let (mut r, mut h) = (Regard::default(), Hasard::default());
        let mut b = Bilan { plus_court_regard: f64::MAX, ..Bilan::default() };
        let (mut en_clin, mut cible, mut depuis) = (false, None, 0.0);
        let dt = 1.0 / 60.0;
        for i in 0..7200 {
            let t = 1.0 + i as f64 * dt;
            let (niveau, fin) = if a == Activite::Parle { voix_inventee(t) } else { (None, false) };
            let phrase = r.phrase(t, niveau, fin);
            r.emouvoir(t, emotion);
            r.vivre(
                &Moment { temps: t, dt: dt as f32, activite: a, vivant: true, ..Default::default() },
                phrase,
                pensif,
                calmes,
                poses,
                &mut h,
            );
            if r.en_clin && !en_clin {
                b.clins += 1;
                b.clins_ailleurs += usize::from(Ou::de(r.cible) != Ou::Personne && !r.pensif && r.bond_cache.is_none());
                b.duree_des_clins += r.duree_du_clin;
            }
            en_clin = r.en_clin;
            if r.cible != cible {
                b.bonds += 1;
                b.coups_d_oeil += usize::from(Ou::de(r.cible) != Ou::Personne);
                b.vers_le_bas += usize::from(direction(r.cible)[1] < -10.0);
                b.plus_court_regard = b.plus_court_regard.min(t - depuis);
                (cible, depuis) = (r.cible, t);
            }
            b.sur_elle += if Ou::de(r.cible) == Ou::Personne { dt / 120.0 } else { 0.0 };
            b.sur_la_photo += if r.cible.is_none() { dt / 120.0 } else { 0.0 };
            b.plus_long_bond = b.plus_long_bond.max(r.duree_du_bond);
        }
        (b, r)
    }

    #[test]
    fn l_emotion_donne_son_humeur_au_regard() {
        // Deux minutes de parole sous chaque émotion (le regard), et deux minutes de repos (les clignements
        // d'eux-mêmes, sans ceux que la voix appelle).
        let n = DIRECTIONS.len();
        let parle = |e| deux_minutes_sous(e, Activite::Parle, false, true, n).0;
        let repos = |e| deux_minutes_sous(e, Activite::Repos, false, true, n).0.clins;
        let (calme, triste, empathie) = (parle(None), parle(Some(Emotion::Triste)), parle(Some(Emotion::Empathie)));
        let (serieuse, surprise) = (parle(Some(Emotion::Serieuse)), parle(Some(Emotion::Surprise)));
        let amusee = parle(Some(Emotion::Amusee));
        let vus = format!(
            "calme {calme:?}\ntriste {triste:?}\nempathie {empathie:?}\nsérieuse {serieuse:?}\nsurprise {surprise:?}\n\
             amusée {amusee:?}"
        );
        let duree = |b: &Bilan| b.duree_des_clins / b.clins.max(1) as f64;
        let en_bas = |b: &Bilan| b.vers_le_bas as f64 / b.coups_d_oeil.max(1) as f64;
        // La peine baisse les yeux : ses coups d'œil vont surtout vers le bas (bien plus qu'au calme), ses yeux restent
        // plus longtemps sur son visage (qui regarde en bas), et elle cligne lentement.
        assert!(en_bas(&triste) > 0.6 && en_bas(&triste) > 1.5 * en_bas(&calme), "{vus}");
        assert!(triste.sur_la_photo > calme.sur_la_photo && duree(&triste) > 1.2 * duree(&calme), "{vus}");
        // L'empathie et le sérieux tiennent le regard : moins de coups d'œil ; la surprise, aucun ; le rire, plus.
        assert!(10 * empathie.coups_d_oeil < 7 * calme.coups_d_oeil, "{vus}");
        assert!(10 * serieuse.coups_d_oeil < 7 * calme.coups_d_oeil, "{vus}");
        assert!(surprise.coups_d_oeil == 0 && amusee.coups_d_oeil > calme.coups_d_oeil, "{vus}");
        // D'eux-mêmes, le sérieux et la surprise clignent moins, le rire plus ; jamais les yeux ailleurs.
        let (c, s, z, a) =
            (repos(None), repos(Some(Emotion::Serieuse)), repos(Some(Emotion::Surprise)), repos(Some(Emotion::Amusee)));
        assert!(s < c && z < c && a > c, "au repos : calme {c}, sérieuse {s}, surprise {z}, amusée {a}");
        let toutes = [&calme, &triste, &empathie, &serieuse, &surprise, &amusee];
        assert!(toutes.iter().all(|b| b.clins_ailleurs == 0), "{vus}");
        // La surprise qui vient fige le regard : pas un clignement pendant un peu plus d'une seconde, même à la fin
        // d'une phrase ; puis il reprend.
        let mut r = Regard { pret: true, clin: 10.2, fin_du_clin: 5.0, ..Regard::default() };
        r.emouvoir(10.0, Some(Emotion::Surprise));
        assert!(r.clin >= 11.1 && !r.cligner(10.6), "{} {}", r.clin, r.fige_jusqua);
        assert!(r.cligner(11.2));
    }

    #[test]
    fn ses_yeux_vivent_comme_ceux_d_une_personne() {
        let (parle, _) = deux_minutes(Activite::Parle, false, true, DIRECTIONS.len());
        let (ecoute, _) = deux_minutes(Activite::Ecoute, false, true, DIRECTIONS.len());
        let (repos, _) = deux_minutes(Activite::Repos, false, true, DIRECTIONS.len());
        let vus = format!("en parlant {parle:?}\nen écoutant {ecoute:?}\nau repos {repos:?}");
        // Elle cligne au rythme d'une personne (par minute : la moitié de deux minutes) — plus en parlant, moins en
        // écoutant — et jamais les yeux ailleurs.
        let par_minute = |b: &Bilan| b.clins as f64 / 2.0;
        assert!((16.0..=32.0).contains(&par_minute(&parle)), "{vus}");
        assert!((8.0..=18.0).contains(&par_minute(&ecoute)) && par_minute(&ecoute) < par_minute(&parle), "{vus}");
        assert!((10.0..=22.0).contains(&par_minute(&repos)), "{vus}");
        assert!([&parle, &ecoute, &repos].iter().all(|b| b.clins_ailleurs == 0), "{vus}");
        // Elle regarde surtout la personne : en écoutant, sans jamais la quitter ; en parlant, avec des coups d'œil
        // ailleurs ; ses yeux bougent souvent, d'un bond, et restent posés entre deux.
        assert!(ecoute.coups_d_oeil == 0 && ecoute.bonds > 40, "{vus}");
        assert!((0.7..0.95).contains(&parle.sur_elle) && parle.coups_d_oeil >= 15 && parle.bonds > 60, "{vus}");
        assert!(repos.sur_elle > 0.8 && repos.coups_d_oeil >= 3 && repos.bonds > 25, "{vus}");
        for b in [&parle, &ecoute, &repos] {
            assert!(b.plus_long_bond <= 0.07 && b.plus_court_regard >= 0.06, "{vus}");
        }
        // En réfléchissant, tant que son air pensif vient (ou s'en va), ses yeux restent là où il regarde, en haut ; posé,
        // ils cherchent, ailleurs que sur la personne, chaque regard un moment ; elle cligne encore, moins.
        let (venue, r) = deux_minutes(Activite::Reflechit, true, false, DIRECTIONS.len());
        assert!(r.cible == Some(8) && venue.bonds == 1 && venue.clins > 10, "{venue:?}");
        let (pensive, _) = deux_minutes(Activite::Reflechit, true, true, DIRECTIONS.len());
        assert!(pensive.sur_elle < 0.01 && pensive.bonds > 30 && pensive.plus_court_regard >= 0.15, "{pensive:?}");
        assert!(pensive.clins > 10 && pensive.clins < parle.clins, "{pensive:?}");
        // Pensive en parlant, elle regarde aussi la personne, une fois sur deux à peu près.
        let (pensive, _) = deux_minutes(Activite::Parle, true, true, DIRECTIONS.len());
        assert!((0.25..0.75).contains(&pensive.sur_elle) && pensive.bonds > 30, "{pensive:?}");
        // Sans poses du regard (ou sans les animations de Windows), ses yeux ne bougent pas ; elle cligne encore.
        let (fixe, _) = deux_minutes(Activite::Parle, false, true, 0);
        assert!(fixe.bonds == 0 && fixe.clins > 20, "{fixe:?}");
    }
}
