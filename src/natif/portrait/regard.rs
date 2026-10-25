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
/// haute pour la dernière fois, si sa pause a eu sa part. L'activité d'avant.
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
}

impl Regard {
    /// Le regard à cette image, les `emotions` du visage à la leur : les yeux sous l'émotion (une émotion qui a les siens
    /// les couvre), sauf sous son air pensif, qui garde les yeux de la photo (ses sourcils changent, pas ses paupières) :
    /// le regard qui réfléchit se pose sur lui et y cherche ; le clignement par-dessus tout. Sans les animations de
    /// Windows, les yeux restent sur la personne ; elle cligne encore.
    pub(super) fn avancer(&mut self, m: &Moment, v: &Visage, emotions: &Emotions, h: &mut Hasard, pose: &mut Pose) {
        let [yeux, regards, ..] = &v.calques;
        let phrase = self.ecouter(m);
        let dominante = emotions.dominante();
        // Pensive : son air pensif domine — en réfléchissant, il va et vient, et entre deux elle revient à la personne
        // ([`Emotions`]) ; un visage qui n'a pas cet air cherche des yeux tant qu'elle réfléchit.
        let a_l_air = v.expressions.iter().any(|x| x.emotion == Emotion::Reflexion);
        let pensif = dominante == Some(Emotion::Reflexion) || (m.activite == Activite::Reflechit && !a_l_air);
        let poses = if m.vivant { regards.len().min(DIRECTIONS.len()) } else { 0 };
        self.vivre(m, phrase, pensif, emotions.posees(), poses, h);
        let sur_son_air = self.pensif && matches!(dominante, Some(Emotion::Reflexion) | None);
        self.peindre(m.temps, regards, yeux, sur_son_air, pose);
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
        let (clin, coup_d_oeil) = if fin || phrase.pause || phrase.commence || ecoute {
            let r = h.tirer();
            match () {
                _ if fin => (r < 0.7, false),
                _ if phrase.pause => (r < 0.35, (0.35..0.65).contains(&r)),
                _ if phrase.commence => (false, r < 0.6),
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
        if Ou::de(self.cible) != Ou::Personne {
            // Ailleurs, elle revient à la personne au bout de son coup d'œil, ou plus tôt — sa phrase finit, une émotion
            // vient, elle se met à écouter —, souvent en clignant.
            if t >= self.jusqua || !calmes || fin_proche || m.activite == Activite::Ecoute {
                let pour = tenue(None, m.activite, h);
                let avec_clin = h.tirer() < 0.6;
                self.aller(None, pour, t, avec_clin);
            }
            return;
        }
        // Sur la personne : le coup d'œil ailleurs que sa voix lui donne envie de jeter, pas quand elle finit ni quand une
        // émotion vient.
        let libre = calmes && !fin_proche;
        if libre && coup_d_oeil {
            if let Some(k) = ailleurs(poses, parle, self.cible, h) {
                let pour = tenue(Some(k), m.activite, h);
                self.aller(Some(k), pour, t, false);
                return;
            }
        }
        if t < self.jusqua {
            return;
        }
        // Au bout de chaque regard posé : sur la photo, de loin en loin un coup d'œil ailleurs (jamais en écoutant), sinon
        // un petit bond vers un de ses traits, ou rester ; sur un de ses traits, le retour à elle, ou un autre trait.
        let r = h.tirer();
        let cible = match self.cible {
            None => {
                let (loin, pres) = match m.activite {
                    Activite::Parle => (0.12, 0.6),
                    Activite::Ecoute => (0.0, 0.65),
                    _ => (0.15, 0.6),
                };
                if libre && r < loin {
                    ailleurs(poses, parle, None, h)
                } else if r < pres {
                    sur_elle(poses, None, h)
                } else {
                    None
                }
            }
            Some(_) if r < 0.55 => None,
            ici => sur_elle(poses, ici, h),
        };
        let pour = tenue(cible, m.activite, h);
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

    /// Un clignement tout de suite, s'il peut venir : ni pendant un autre, ni dans la demi-seconde qui le suit ; entier.
    /// Rend s'il vient.
    fn cligner(&mut self, t: f64) -> bool {
        let peut = !self.en_clin && t - self.fin_du_clin >= 0.5;
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
                    (self.depuis, self.jusqua) = (t, t + tenue(None, self.activite, h));
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
            self.clin = t + if deux { 0.05 } else { a + (b - a) * h.tirer() };
            self.duree_du_clin = DUREE_DU_CLIN * (0.85 + 0.3 * h.tirer());
            self.profondeur = if !deux && h.tirer() < 0.15 { 0.6 + 0.25 * h.tirer() as f32 } else { 1.0 };
        }
    }

    /// Les yeux à cette image : la pose d'où ils bondissent, entière, et celle où ils vont, qui se fond en elle ; vers la
    /// personne, celle d'où ils partent s'efface — sous l'émotion, ou sur son air pensif (`sur_son_air`). Puis le
    /// clignement, par-dessus tout ; des yeux ailleurs (pensifs, ou que le clignement ramène) se ferment sans passer par
    /// les yeux mi-clos, qui regardent la personne.
    fn peindre(&self, t: f64, regards: &[Calque], yeux: &[Calque], sur_son_air: bool, pose: &mut Pose) {
        let strate = if sur_son_air { Strate::Emotion } else { Strate::Dessous };
        let e = lisse(self.bond);
        let calque = |k: Option<usize>| k.and_then(|k| regards.get(k));
        match (calque(self.vu), calque(self.cible)) {
            (Some(a), Some(b)) if self.vu != self.cible => {
                pose.peindre(strate, a, 1.0);
                pose.peindre(strate, b, e);
            }
            (Some(a), None) => pose.peindre(strate, a, 1.0 - e),
            (_, Some(b)) => pose.peindre(strate, b, e),
            (None, None) => {}
        }
        if self.en_clin {
            let fermes = fermeture((t - self.clin) / self.duree_du_clin) as f32 * self.profondeur;
            let ailleurs = Ou::de(self.cible) != Ou::Personne;
            let serie = if ailleurs && yeux.len() > 1 { &yeux[1..] } else { yeux };
            pose.serie(Strate::Dessus, serie, fermes, 1.0);
        }
    }
}

/// Combien de temps les yeux restent posés sur `cible`, en secondes, selon ce que fait Eve : sur la personne, plus
/// longtemps au repos ; sur un de ses traits, moins ; ailleurs, le temps d'un coup d'œil en parlant, plus au repos.
fn tenue(cible: Option<usize>, activite: Activite, h: &mut Hasard) -> f64 {
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
    a + (b - a) * h.tirer()
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
/// paraîtrait distraite) ; jamais là où les yeux sont déjà (`ici`).
fn ailleurs(poses: usize, parle: bool, ici: Option<usize>, h: &mut Hasard) -> Option<usize> {
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
    use super::*;

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
    /// ramener), le temps sur la personne, les bonds et les coups d'œil ailleurs, le plus long bond, le plus court regard
    /// posé.
    #[derive(Debug, Default)]
    struct Bilan {
        clins: usize,
        clins_ailleurs: usize,
        sur_elle: f64,
        bonds: usize,
        coups_d_oeil: usize,
        plus_long_bond: f32,
        plus_court_regard: f64,
    }

    /// Les yeux pendant deux minutes de l'activité `a`, à soixante images par seconde (en parlant, sur la voix inventée),
    /// avec `poses` poses du regard, les émotions posées (`calmes`) ou non ; et le regard à la fin.
    fn deux_minutes(a: Activite, pensif: bool, calmes: bool, poses: usize) -> (Bilan, Regard) {
        let (mut r, mut h) = (Regard::default(), Hasard::default());
        let mut b = Bilan { plus_court_regard: f64::MAX, ..Bilan::default() };
        let (mut en_clin, mut cible, mut depuis) = (false, None, 0.0);
        let dt = 1.0 / 60.0;
        for i in 0..7200 {
            let t = 1.0 + i as f64 * dt;
            let (niveau, fin) = if a == Activite::Parle { voix_inventee(t) } else { (None, false) };
            let phrase = r.phrase(t, niveau, fin);
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
            }
            en_clin = r.en_clin;
            if r.cible != cible {
                b.bonds += 1;
                b.coups_d_oeil += usize::from(Ou::de(r.cible) != Ou::Personne);
                b.plus_court_regard = b.plus_court_regard.min(t - depuis);
                (cible, depuis) = (r.cible, t);
            }
            b.sur_elle += if Ou::de(r.cible) == Ou::Personne { dt / 120.0 } else { 0.0 };
            b.plus_long_bond = b.plus_long_bond.max(r.duree_du_bond);
        }
        (b, r)
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
