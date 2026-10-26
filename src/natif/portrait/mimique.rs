//! La mimique du portrait d'Eve : ce que fait son visage d'une image à l'autre, partie par partie — ses émotions
//! ([`super::emotions`]), sa bouche ([`super::bouche`]), les gestes de sa tête et de ses sourcils ([`super::gestes`]), son
//! buste et son souffle ([`super::corps`]), ses cheveux ([`super::cheveux`]), son regard ([`super::regard`]) —, chacune
//! sur ses ressorts ([`Ressort`]), assemblées en une pose ([`super::Pose`]).

use eve::parole::Emotion;

use super::bouche::Bouche;
use super::cheveux::Cheveux;
use super::corps::Buste;
use super::emotions::Emotions;
use super::gestes::Gestes;
use super::regard::Regard;
use super::{Activite, Pose, Visage};

/// Ce que sait le visage à cette image : l'heure (celle de la fenêtre, en secondes) et le temps depuis la précédente ;
/// ce que fait Eve ; si les animations de Windows sont permises ; la voix de la personne que le micro entend (de 0 à 1 ;
/// 0 quand Eve n'écoute pas) ; où regardent ses yeux, à l'image d'avant (sa tête les suit :
/// [`super::regard::Regard::direction`]).
#[derive(Clone, Copy, Default)]
pub(super) struct Moment {
    pub(super) temps: f64,
    pub(super) dt: f32,
    pub(super) activite: Activite,
    pub(super) vivant: bool,
    pub(super) entendu: f32,
    pub(super) yeux: [f32; 2],
}

impl Moment {
    /// Eve parle : sa voix s'entend.
    pub(super) fn parle(&self) -> bool {
        self.activite == Activite::Parle
    }
}

/// Un ressort amorti au plus juste (critique) : sa valeur `x` va vers sa cible sans la dépasser, part doucement et se
/// pose sans à-coup, comme un muscle — là où une approche exponentielle part d'un coup ; `v` : sa vitesse, gardée d'une
/// image à l'autre.
#[derive(Clone, Copy, Default)]
pub(super) struct Ressort {
    pub(super) x: f32,
    pub(super) v: f32,
}

impl Ressort {
    /// Avance de `dt` secondes vers `cible`, à la pulsation `w` (par seconde : aux neuf dixièmes du chemin en 3,9/`w`
    /// secondes), par la solution exacte du ressort — juste quel que soit le pas ; rend sa valeur.
    pub(super) fn vers(&mut self, cible: f32, w: f32, dt: f32) -> f32 {
        let e = (-w * dt).exp();
        let d = self.x - cible;
        let a = self.v + w * d;
        self.x = cible + (d + a * dt) * e;
        self.v = (self.v - w * a * dt) * e;
        self.x
    }
}

/// Le hasard du visage (un xorshift), que ses parties se partagent : il ne bouge pas comme un métronome.
pub(super) struct Hasard(u64);

impl Default for Hasard {
    fn default() -> Hasard {
        Hasard(0x9E37_79B9_7F4A_7C15)
    }
}

impl Hasard {
    /// Un nombre de 0 à 1, différent à chaque fois.
    pub(super) fn tirer(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x % 10_000) as f64 / 10_000.0
    }

    /// Un nombre de −1 à 1.
    pub(super) fn signe(&mut self) -> f32 {
        2.0 * self.tirer() as f32 - 1.0
    }
}

/// Un passage adouci, de 0 à 1 : lent au départ et à l'arrivée.
pub(super) fn lisse(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// La mimique entière : ses parties, le hasard qu'elles se partagent, l'heure de la dernière image, la voix de la
/// personne que le micro entend.
#[derive(Default)]
pub(super) struct Mimique {
    emotions: Emotions,
    bouche: Bouche,
    gestes: Gestes,
    buste: Buste,
    cheveux: Cheveux,
    regard: Regard,
    hasard: Hasard,
    vu_a: f64,
    entendu: f32,
}

impl Mimique {
    /// La voix de la personne que le micro entend (de 0 à 1 ; 0 quand Eve n'écoute pas) : en l'écoutant, Eve hoche à
    /// ses pauses.
    pub(super) fn entendre(&mut self, niveau: f32) {
        self.entendu = niveau;
    }

    /// La pose du visage `v` à l'heure `temps` (celle de la fenêtre), Eve à son `activite`, l'émotion de sa réponse
    /// `emotion` : les émotions d'abord (celle qui domine donne ses bouches et sa tête ; elles couvrent le visage
    /// calme), puis la bouche, les gestes (la tête suit la bouche), le buste, les cheveux (ils suivent la tête), le
    /// regard (sur le visage d'une émotion, le haut du visage : ses yeux, et ses sourcils levés avec ceux des gestes).
    /// `vivant` : les animations de Windows sont permises — sans elles, ni souffle, ni regard de côté, ni tête qui
    /// bouge, et la pose dit quand la redessiner (au prochain clignement, une fois tout posé).
    pub(super) fn pose(
        &mut self,
        v: &Visage,
        temps: f64,
        activite: Activite,
        vivant: bool,
        emotion: Option<Emotion>,
    ) -> Pose {
        let dt = (temps - self.vu_a).clamp(0.0, 0.1) as f32;
        self.vu_a = temps;
        let m = Moment { temps, dt, activite, vivant, entendu: self.entendu, yeux: self.regard.direction() };
        let mut pose = Pose::default();
        let h = &mut self.hasard;
        self.emotions.avancer(&m, v, emotion, &mut pose);
        self.bouche.avancer(&m, v, &self.emotions, &mut pose);
        self.gestes.avancer(&m, v, self.bouche.ouverture(), &self.emotions, h, &mut pose);
        self.buste.avancer(&m, &self.emotions, &mut pose);
        self.cheveux.avancer(&m, v, &mut pose);
        self.regard.avancer(&m, v, &self.emotions, self.gestes.sourcils(), h, &mut pose);
        if !vivant && !m.parle() && self.emotions.posees() && self.bouche.posee() {
            pose.apres = self.regard.prochain_clin(temps);
        }
        pose
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn un_ressort_se_pose_sans_a_coup() {
        // Vers 1, à la pulsation 10 : il ne dépasse jamais, il y est presque en une demi-seconde ; le même chemin à grands
        // ou à petits pas.
        let (mut a, mut b) = (Ressort::default(), Ressort::default());
        let mut avant = 0.0;
        for _ in 0..50 {
            let x = a.vers(1.0, 10.0, 0.01);
            assert!(x >= avant && x <= 1.0, "{x}");
            avant = x;
        }
        assert!(a.x > 0.95 && a.x < 1.0, "{}", a.x);
        for _ in 0..5 {
            b.vers(1.0, 10.0, 0.1);
        }
        assert!((a.x - b.x).abs() < 1e-4, "{} et {}", a.x, b.x);
        // Il part doucement : au premier centième de seconde, moins d'un centième du chemin, là où une approche
        // exponentielle de même vitesse en aurait déjà fait près d'un dixième.
        assert!(Ressort::default().vers(1.0, 10.0, 0.01) < 0.01);
    }

    #[test]
    fn le_hasard_reste_entre_ses_bornes() {
        let mut h = Hasard::default();
        for _ in 0..1000 {
            let (t, s) = (h.tirer(), h.signe());
            assert!((0.0..1.0).contains(&t) && (-1.0..=1.0).contains(&s), "{t} {s}");
        }
        assert_eq!((lisse(-1.0), lisse(0.5), lisse(2.0)), (0.0, 0.5, 1.0));
    }
}
